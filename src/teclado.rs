//! Gancho de teclado: Alt+Tab e Win+Tab abrem a visão geral.
//!
//! O gancho roda no laço de mensagens mas não toca no estado: só posta para a barra.
//! Janela de programa rodando como administrador em foco não passa pelo gancho, e aí o
//! Alt+Tab do Windows aparece; jogo ou vídeo em tela cheia também fica com o do Windows.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_ALT_TAB: u32 = WM_APP + 20;
pub const WM_WIN_TAB: u32 = WM_APP + 21;
pub const WM_ALT_SOLTO: u32 = WM_APP + 22;
pub const WM_ESC_ALT: u32 = WM_APP + 23;
/// Ctrl+Win+←/→: grupo anterior/seguinte (wparam 1 = seguinte).
pub const WM_GRUPO_VIZINHO: u32 = WM_APP + 24;

/// Para onde postar.
pub static DESTINO: AtomicIsize = AtomicIsize::new(0);
/// Visão geral aberta (ou para abrir) pelo Alt+Tab: soltar o Alt escolhe.
pub static MODO_ALT: AtomicBool = AtomicBool::new(false);
pub static ALT_TAB: AtomicBool = AtomicBool::new(true);
/// Visão geral aberta: o gancho manda as teclas para ela mesmo sem o foco.
pub static VISAO_ABERTA: AtomicBool = AtomicBool::new(false);
pub static VISAO: AtomicIsize = AtomicIsize::new(0);
/// Painel de rede ou volume aberto (ele não pega o foco): o Esc vai para ele.
pub static PAINEL_ABERTO: AtomicBool = AtomicBool::new(false);
pub static PAINEL: AtomicIsize = AtomicIsize::new(0);
/// Só para teste (BARRA_TESTE=1): aceita teclas simuladas por SendInput.
static ACEITA_SIMULADA: AtomicBool = AtomicBool::new(false);

fn apertada(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

/// Janela em tela cheia na frente (jogo, vídeo, F11).
fn tela_cheia_na_frente() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        // a visão geral também cobre o monitor inteiro: janela da própria barra não conta
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if fg.is_invalid() || IsZoomed(fg).as_bool() || pid == windows::Win32::System::Threading::GetCurrentProcessId() {
            return false;
        }
        let classe = crate::janelas::classe(fg);
        if classe == "Progman" || classe == "WorkerW" {
            return false;
        }
        let mut r = RECT::default();
        _ = GetWindowRect(fg, &mut r);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        _ = GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi);
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

fn postar(msg: u32, wp: usize) {
    let d = HWND(DESTINO.load(Ordering::Relaxed) as *mut _);
    unsafe { _ = PostMessageW(Some(d), msg, WPARAM(wp), LPARAM(0)) };
}

unsafe extern "system" fn gancho(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let injetada = k.flags.0 & LLKHF_INJECTED.0 != 0 && !ACEITA_SIMULADA.load(Ordering::Relaxed);
        let desce = wp.0 as u32 == WM_KEYDOWN || wp.0 as u32 == WM_SYSKEYDOWN;
        let vk = VIRTUAL_KEY(k.vkCode as u16);
        if !injetada {
            // Ctrl+Win+←/→: no lugar da troca de área de trabalho virtual do Windows
            if (vk == VK_LEFT || vk == VK_RIGHT) && desce && apertada(VK_CONTROL) && (apertada(VK_LWIN) || apertada(VK_RWIN)) && !tela_cheia_na_frente() {
                postar(WM_GRUPO_VIZINHO, (vk == VK_RIGHT) as usize);
                return LRESULT(1);
            }
            if vk == VK_TAB && desce && !apertada(VK_CONTROL) {
                let alt = k.flags.0 & LLKHF_ALTDOWN.0 != 0;
                let win = apertada(VK_LWIN) || apertada(VK_RWIN);
                if (alt != win) && !tela_cheia_na_frente() {
                    if alt && ALT_TAB.load(Ordering::Relaxed) {
                        postar(WM_ALT_TAB, apertada(VK_SHIFT) as usize);
                        return LRESULT(1);
                    }
                    if win {
                        postar(WM_WIN_TAB, 0);
                        return LRESULT(1);
                    }
                }
            }
            // visão geral aberta sem o foco (o Windows às vezes não deixa trocar): as teclas vão
            // direto para ela, senão o Enter e os números cairiam na janela que estava na frente
            let visao = HWND(VISAO.load(Ordering::Relaxed) as *mut _);
            if VISAO_ABERTA.load(Ordering::Relaxed) && desce && GetForegroundWindow() != visao && encaminhar(visao, vk, k.scanCode) {
                return LRESULT(1);
            }
            if PAINEL_ABERTO.load(Ordering::Relaxed) && vk == VK_ESCAPE && desce {
                let p = HWND(PAINEL.load(Ordering::Relaxed) as *mut _);
                _ = PostMessageW(Some(p), WM_KEYDOWN, WPARAM(VK_ESCAPE.0 as usize), LPARAM(0));
                return LRESULT(1);
            }
            if MODO_ALT.load(Ordering::Relaxed) {
                if matches!(vk, VK_MENU | VK_LMENU | VK_RMENU) && !desce {
                    postar(WM_ALT_SOLTO, 0);
                }
                if vk == VK_ESCAPE && desce {
                    postar(WM_ESC_ALT, 0);
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// Instala o gancho numa linha de execução só dele. No laço da barra ele atrasava enquanto a
/// visão geral abria (miniaturas, pintura da tela inteira); passando do tempo limite, o Windows
/// pula o gancho e o Tab vai para o Alt+Tab nativo.
pub fn instalar(destino: HWND) {
    DESTINO.store(destino.0 as isize, Ordering::Relaxed);
    ACEITA_SIMULADA.store(std::env::var_os("BARRA_TESTE").is_some(), Ordering::Relaxed);
    std::thread::spawn(|| unsafe {
        let inst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let Ok(_g) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(gancho), Some(inst.into()), 0) else { return };
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}

/// Aperta e solta uma tecla sem uso (0xE8) enquanto o Win ou o Alt ainda está apertado:
/// assim, soltar o Win não abre o Iniciar e soltar o Alt não ativa o menu do programa.
pub fn mascarar() {
    let tecla = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0xE8),
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                ..Default::default()
            },
        },
    };
    unsafe { SendInput(&[tecla(false), tecla(true)], size_of::<INPUT>() as i32) };
}

/// Traz a janela para a frente mesmo quando o Windows bloqueia a troca de foco (o processo
/// que recebeu a tecla foi o da janela em foco, não a barra).
pub fn forcar_primeiro_plano(h: HWND) {
    unsafe {
        if IsIconic(h).as_bool() {
            _ = ShowWindow(h, SW_RESTORE);
        }
        if SetForegroundWindow(h).as_bool() && GetForegroundWindow() == h {
            return;
        }
        let fg = GetForegroundWindow();
        let deles = GetWindowThreadProcessId(fg, None);
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        let nosso = GetCurrentThreadId();
        let ligou = deles != 0 && deles != nosso && AttachThreadInput(nosso, deles, true).as_bool();
        _ = BringWindowToTop(h);
        _ = SetForegroundWindow(h);
        if ligou {
            _ = AttachThreadInput(nosso, deles, false);
        }
        if GetForegroundWindow() == h {
            return;
        }
        // último recurso: um Alt simulado libera a troca de foco (o Windows deixa trocar quando
        // vê o Alt, por causa dos menus); a tecla vai e volta antes de qualquer janela reagir
        let alt = |up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MENU, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
        };
        let ja_apertado = apertada(VK_MENU);
        if !ja_apertado {
            SendInput(&[alt(false)], size_of::<INPUT>() as i32);
        }
        _ = SetForegroundWindow(h);
        if !ja_apertado {
            SendInput(&[alt(true)], size_of::<INPUT>() as i32);
        }
    }
}

/// Repassa uma tecla para a visão geral: teclas de comando como WM_KEYDOWN, o resto como o
/// caractere que ela digita no layout atual (filtro, 1–9, +, -). Alt, Win, Ctrl e Shift
/// sozinhos passam direto.
unsafe fn encaminhar(visao: HWND, vk: VIRTUAL_KEY, scan: u32) -> bool {
    if matches!(vk, VK_MENU | VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_SHIFT | VK_LSHIFT | VK_RSHIFT) {
        return false;
    }
    if matches!(vk, VK_ESCAPE | VK_RETURN | VK_TAB | VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_BACK | VK_DELETE | VK_F2) {
        _ = PostMessageW(Some(visao), WM_KEYDOWN, WPARAM(vk.0 as usize), LPARAM(0));
        return true;
    }
    let mut estado = [0u8; 256];
    if apertada(VK_SHIFT) {
        estado[VK_SHIFT.0 as usize] = 0x80;
    }
    if GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0 {
        estado[VK_CAPITAL.0 as usize] = 0x01;
    }
    let layout = GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), None));
    let mut buf = [0u16; 4];
    let n = ToUnicodeEx(vk.0 as u32, scan, &estado, &mut buf, 4, Some(layout));
    if n == 1 {
        _ = PostMessageW(Some(visao), WM_CHAR, WPARAM(buf[0] as usize), LPARAM(0));
        return true;
    }
    false
}
