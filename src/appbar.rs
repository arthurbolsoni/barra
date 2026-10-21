//! Registro da barra como AppBar: o Windows desconta a faixa dela da área útil, então janela
//! maximizada e as zonas do snap-zones param acima dela.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

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

/// Pede a faixa de baixo do monitor principal com `altura` px e põe a janela nela.
pub fn posicionar(h: HWND, altura: i32) -> RECT {
    let m = monitor();
    let mut abd = dados(h);
    abd.uEdge = ABE_BOTTOM;
    abd.rc = RECT { left: m.left, right: m.right, top: m.bottom - altura, bottom: m.bottom };
    unsafe {
        SHAppBarMessage(ABM_QUERYPOS, &mut abd);
        // o Windows pode ter subido a faixa (outra barra embaixo); a altura é a nossa
        abd.rc.top = abd.rc.bottom - altura;
        SHAppBarMessage(ABM_SETPOS, &mut abd);
        let r = abd.rc;
        _ = SetWindowPos(h, Some(HWND_TOPMOST), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE);
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

/// Aplicativo em tela cheia abriu (jogo, vídeo): a barra desce para trás dele; fechou: volta.
pub fn tela_cheia(h: HWND, aberta: bool) {
    let depois = if aberta { HWND_BOTTOM } else { HWND_TOPMOST };
    unsafe { _ = SetWindowPos(h, Some(depois), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
}
