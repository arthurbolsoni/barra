//! Registro da barra como AppBar: o Windows desconta a faixa dela da área útil, então janela
//! maximizada e as zonas do snap-zones param acima dela.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::BOOL;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::janelas::{classe, cloaked};

/// Janela em tela cheia por cima no monitor da barra: a barra fica no fundo, atrás dela.
static TELA_CHEIA: AtomicBool = AtomicBool::new(false);

/// Camada da barra: acima de tudo, ou no fundo enquanto há tela cheia.
fn camada() -> HWND {
    if TELA_CHEIA.load(Ordering::Relaxed) {
        HWND_BOTTOM
    } else {
        HWND_TOPMOST
    }
}

fn dados(h: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: h, ..Default::default() }
}

pub fn registrar(h: HWND, msg: u32) {
    let mut abd = dados(h);
    abd.uCallbackMessage = msg;
    unsafe { SHAppBarMessage(ABM_NEW, &mut abd) };
}

pub fn remover(h: HWND) {
    let mut abd = dados(h);
    unsafe { SHAppBarMessage(ABM_REMOVE, &mut abd) };
}

/// Monitor principal (rcMonitor).
pub fn monitor() -> RECT {
    unsafe {
        let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        _ = GetMonitorInfoW(mon, &mut mi);
        mi.rcMonitor
    }
}

/// Pede a faixa de cima do monitor principal com `altura` px e põe a janela nela.
pub fn posicionar(h: HWND, altura: i32) -> RECT {
    let m = monitor();
    let mut abd = dados(h);
    abd.uEdge = ABE_TOP;
    abd.rc = RECT { left: m.left, right: m.right, top: m.top, bottom: m.top + altura };
    unsafe {
        SHAppBarMessage(ABM_QUERYPOS, &mut abd);
        // o Windows pode ter descido a faixa (outra barra em cima); a altura é a nossa
        abd.rc.bottom = abd.rc.top + altura;
        SHAppBarMessage(ABM_SETPOS, &mut abd);
        let r = abd.rc;
        _ = SetWindowPos(h, Some(camada()), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE);
        r
    }
}

/// Avisos que o Windows manda à AppBar.
pub fn ativada(h: HWND) {
    let mut abd = dados(h);
    unsafe { SHAppBarMessage(ABM_ACTIVATE, &mut abd) };
}

pub fn movida(h: HWND) {
    let mut abd = dados(h);
    unsafe { SHAppBarMessage(ABM_WINDOWPOSCHANGED, &mut abd) };
}

struct Busca {
    monitor: RECT,
    pid: u32,
    tela_cheia: bool,
}

/// Janelas da frente para trás: a primeira que é de programa e está no monitor decide.
unsafe extern "system" fn olhar(h: HWND, lp: LPARAM) -> BOOL {
    let b = &mut *(lp.0 as *mut Busca);
    if !IsWindowVisible(h).as_bool() || cloaked(h) {
        return TRUE;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
    // janelas da própria barra, e camadas por cima que não são programa (menus, dicas,
    // controles do vídeo, sobreposições que deixam o clique passar)
    if pid == b.pid || ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TRANSPARENT.0) != 0 {
        return TRUE;
    }
    let mut r = RECT::default();
    _ = GetWindowRect(h, &mut r);
    let m = b.monitor;
    if r.right <= m.left || r.left >= m.right || r.bottom <= m.top || r.top >= m.bottom {
        return TRUE;
    }
    let c = classe(h);
    if c == "Progman" || c == "WorkerW" {
        return FALSE;
    }
    let cobre = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
    // maximizada com barra de título passa da borda do monitor, mas não é tela cheia
    let estilo = GetWindowLongW(h, GWL_STYLE) as u32;
    let maximizada = IsZoomed(h).as_bool() && estilo & WS_CAPTION.0 == WS_CAPTION.0;
    if cobre && !maximizada {
        b.tela_cheia = true;
        return FALSE;
    }
    // janela sempre visível pequena (calculadora, vídeo em janelinha) por cima da tela cheia
    // não a tira de tela cheia
    if ex & WS_EX_TOPMOST.0 != 0 {
        return TRUE;
    }
    FALSE
}

/// Aplicativo em tela cheia (jogo, vídeo, F11) por cima no monitor da barra: a barra desce
/// para trás dele; saiu de tela cheia, fechou ou outra janela veio por cima: a barra volta.
/// O ABN_FULLSCREENAPP do Windows não chegava (VLC em tela cheia, barra do Windows escondida),
/// então a conta é feita aqui: quando muda a janela da frente, nos avisos do shell e a cada
/// segundo.
pub fn conferir_tela_cheia(h: HWND) {
    let mut b = Busca { monitor: monitor(), pid: unsafe { GetCurrentProcessId() }, tela_cheia: false };
    unsafe { _ = EnumWindows(Some(olhar), LPARAM(&mut b as *mut _ as isize)) };
    TELA_CHEIA.store(b.tela_cheia, Ordering::Relaxed);
    // compara com a camada em que a barra está de fato, não com a conta anterior
    let no_topo = unsafe { GetWindowLongW(h, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST.0 != 0;
    if no_topo == b.tela_cheia {
        unsafe { _ = SetWindowPos(h, Some(camada()), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }
}
