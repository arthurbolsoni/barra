//! Carregada pelo Windows em cada programa com janela, pelo gancho global que a barra instala
//! (SetWindowsHookEx). Põe os menus Win32 do programa (bandeja, clique direito, menu de
//! sistema) no modo claro ou escuro do tema da barra, pelas funções não documentadas do
//! uxtheme que o próprio Windows usa no Explorer: SetPreferredAppMode (ordinal 135) e
//! FlushMenuThemes (136). Mesma técnica do mod "Dark mode context menus" do Windhawk.

use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{w, PCSTR};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows::Win32::UI::WindowsAndMessaging::CallNextHookEx;

/// Modos do SetPreferredAppMode.
pub const PADRAO: u32 = 0;
pub const ESCURO: u32 = 2;
pub const CLARO: u32 = 3;

/// Modo pedido pela barra. Fica numa seção compartilhada: a barra escreve uma vez e todos
/// os processos leem o mesmo valor.
#[link_section = ".barra"]
static MODO: AtomicU32 = AtomicU32::new(PADRAO);

/// Modo já aplicado neste processo (u32::MAX: nenhum).
static APLICADO: AtomicU32 = AtomicU32::new(u32::MAX);

/// Chamado pela barra para trocar o modo de todos os programas.
#[no_mangle]
pub extern "system" fn definir_modo(modo: u32) {
    MODO.store(modo, Ordering::Relaxed);
}

/// Gancho WH_GETMESSAGE: na primeira mensagem do processo, e depois de cada troca de modo,
/// aplica o modo pedido. Fora isso só passa adiante.
#[no_mangle]
pub extern "system" fn gancho(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let modo = MODO.load(Ordering::Relaxed);
    if matches!(modo, PADRAO | ESCURO | CLARO) && APLICADO.swap(modo, Ordering::Relaxed) != modo {
        aplicar(modo);
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

fn aplicar(modo: u32) {
    unsafe {
        let Ok(ux) = LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) else { return };
        let (Some(preferir), Some(esvaziar)) = (GetProcAddress(ux, PCSTR(135 as *const u8)), GetProcAddress(ux, PCSTR(136 as *const u8))) else {
            return;
        };
        let preferir: extern "system" fn(u32) -> u32 = std::mem::transmute(preferir);
        let esvaziar: extern "system" fn() = std::mem::transmute(esvaziar);
        preferir(modo);
        esvaziar();
    }
}
