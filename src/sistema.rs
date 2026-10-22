//! O que a barra pede ao Windows: esconder a barra dele, abrir o Iniciar e os painéis,
//! preferências no registro e início junto com o Windows.

use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const CHAVE: PCWSTR = w!("Software\\barra");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("barra");
/// Preferência: esconder a barra do Windows enquanto a barra roda.
pub const PREF_ESCONDER: PCWSTR = w!("EsconderBarraWindows");
/// Estado da barra do Windows (ABM_GETSTATE) antes de esconder, para voltar igual.
const ESTADO_ANTES: PCWSTR = w!("EstadoBarraWindows");

pub fn ler(valor: PCWSTR) -> Option<u32> {
    let mut v = 0u32;
    let mut tam = 4u32;
    let ok = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, CHAVE, valor, RRF_RT_REG_DWORD, None, Some(&mut v as *mut _ as *mut c_void), Some(&mut tam))
    };
    ok.is_ok().then_some(v)
}

pub fn gravar(valor: PCWSTR, v: Option<u32>) {
    unsafe {
        let mut k = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, CHAVE, None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None).is_err() {
            return;
        }
        match v {
            Some(v) => _ = RegSetValueExW(k, valor, None, REG_DWORD, Some(&v.to_le_bytes())),
            None => _ = RegDeleteValueW(k, valor),
        }
        _ = RegCloseKey(k);
    }
}

pub fn inicia_com_windows() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, RRF_RT_REG_SZ, None, None, None).is_ok() }
}

pub fn definir_inicio(ligar: bool) {
    unsafe {
        let mut k = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None).is_err() {
            return;
        }
        if ligar {
            let Ok(exe) = std::env::current_exe() else { return };
            let cmd: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain([0]).collect();
            let bytes = std::slice::from_raw_parts(cmd.as_ptr() as *const u8, cmd.len() * 2);
            _ = RegSetValueExW(k, RUN_VALUE, None, REG_SZ, Some(bytes));
        } else {
            _ = RegDeleteValueW(k, RUN_VALUE);
        }
        _ = RegCloseKey(k);
    }
}

/// Barra do Explorer (pula a `Shell_TrayWnd` invisível da própria barra).
fn barra_windows() -> Option<HWND> {
    crate::bandeja::barra_do_explorer()
}

/// Barras do Windows nos outros monitores.
fn barras_secundarias() -> Vec<HWND> {
    let mut v = Vec::new();
    let mut depois: Option<HWND> = None;
    unsafe {
        while let Ok(h) = FindWindowExW(None, depois, w!("Shell_SecondaryTrayWnd"), PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            v.push(h);
            depois = Some(h);
        }
    }
    v
}

fn estado_appbar(h: HWND, novo: Option<u32>) -> u32 {
    let mut abd = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: h, ..Default::default() };
    unsafe {
        match novo {
            Some(s) => {
                abd.lParam = LPARAM(s as isize);
                SHAppBarMessage(ABM_SETSTATE, &mut abd) as u32
            }
            None => SHAppBarMessage(ABM_GETSTATE, &mut abd) as u32,
        }
    }
}

/// Esconde a barra do Windows. Antes passa ela para "ocultar automaticamente", para o Windows
/// liberar a faixa que ela reserva embaixo da tela.
pub fn esconder_barra_windows() {
    let Some(tw) = barra_windows() else { return };
    if ler(ESTADO_ANTES).is_none() {
        gravar(ESTADO_ANTES, Some(estado_appbar(tw, None)));
    }
    if estado_appbar(tw, None) & ABS_AUTOHIDE == 0 {
        estado_appbar(tw, Some(ABS_AUTOHIDE));
    }
    unsafe {
        _ = ShowWindow(tw, SW_HIDE);
        for h in barras_secundarias() {
            _ = ShowWindow(h, SW_HIDE);
        }
    }
}

/// O Windows mostra a barra dele de novo em algumas situações (Explorer reiniciando, monitor
/// novo). Chamado pelo timer quando a preferência está ligada.
pub fn manter_escondida() {
    let Some(tw) = barra_windows() else { return };
    let visivel = unsafe { IsWindowVisible(tw).as_bool() } || barras_secundarias().iter().any(|&h| unsafe { IsWindowVisible(h).as_bool() });
    if visivel {
        esconder_barra_windows();
    }
}

/// Volta a barra do Windows como estava antes de esconder.
pub fn mostrar_barra_windows() {
    let Some(tw) = barra_windows() else { return };
    if let Some(antes) = ler(ESTADO_ANTES) {
        estado_appbar(tw, Some(antes));
        gravar(ESTADO_ANTES, None);
    }
    unsafe {
        _ = ShowWindow(tw, SW_SHOWNA);
        for h in barras_secundarias() {
            _ = ShowWindow(h, SW_SHOWNA);
        }
    }
}

pub fn barra_windows_escondida() -> bool {
    ler(ESTADO_ANTES).is_some()
}


pub fn abrir_uri(uri: PCWSTR) {
    unsafe { ShellExecuteW(None, w!("open"), uri, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
}

/// Central de notificações com o calendário (o mesmo do Win+N).
pub fn abrir_notificacoes() {
    abrir_uri(w!("ms-actioncenter:"));
}
