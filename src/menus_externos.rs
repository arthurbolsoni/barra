//! Menus Win32 dos outros programas (bandeja, clique direito, menu de sistema) no modo claro
//! ou escuro do tema. A barra instala um gancho global e o Windows carrega a barra_menus.dll
//! em cada programa com janela; a DLL chama o SetPreferredAppMode do uxtheme lá dentro.
//! Só alcança programas de 64 bits que não rodam como administrador.

use std::cell::Cell;
use std::os::windows::ffi::OsStrExt;

use windows::core::{s, PCWSTR};
use windows::Win32::Foundation::{HMODULE, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::*;

const PADRAO: u32 = 0;
const ESCURO: u32 = 2;
const CLARO: u32 = 3;

type DefinirModo = unsafe extern "system" fn(u32);

thread_local! {
    static DLL: Cell<Option<(HMODULE, DefinirModo)>> = const { Cell::new(None) };
    static GANCHO: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static ATUAL: Cell<u32> = const { Cell::new(PADRAO) };
}

/// A DLL fica ao lado do barra.exe.
fn dll() -> Option<(HMODULE, DefinirModo)> {
    if let Some(d) = DLL.get() {
        return Some(d);
    }
    let caminho = std::env::current_exe().ok()?.with_file_name("barra_menus.dll");
    let w: Vec<u16> = caminho.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        let m = LoadLibraryW(PCWSTR(w.as_ptr())).ok()?;
        let definir: DefinirModo = std::mem::transmute(GetProcAddress(m, s!("definir_modo"))?);
        DLL.set(Some((m, definir)));
        Some((m, definir))
    }
}

/// Cada programa aplica o modo novo na próxima mensagem que tirar da fila; uma mensagem
/// vazia para todas as janelas faz isso acontecer agora.
fn avisar_todos() {
    unsafe { _ = PostMessageW(Some(HWND_BROADCAST), WM_NULL, WPARAM(0), LPARAM(0)) };
}

/// Liga ou troca o modo: menus escuros em tema escuro, claros em tema claro. Chamado a cada
/// atualização da barra; só age quando algo mudou.
pub fn ligar(tema_claro: bool) {
    let modo = if tema_claro { CLARO } else { ESCURO };
    if GANCHO.get().is_some() && ATUAL.get() == modo {
        return;
    }
    let Some((m, definir)) = dll() else { return };
    ATUAL.set(modo);
    unsafe { definir(modo) };
    if GANCHO.get().is_none() {
        let Some(gancho) = (unsafe { GetProcAddress(m, s!("gancho")) }) else { return };
        let gancho: HOOKPROC = Some(unsafe { std::mem::transmute(gancho) });
        if let Ok(g) = unsafe { SetWindowsHookExW(WH_GETMESSAGE, gancho, Some(m.into()), 0) } {
            GANCHO.set(Some(g));
        }
    }
    avisar_todos();
}

/// Devolve o modo padrão a todos os programas e tira o gancho.
pub fn desligar() {
    let Some(g) = GANCHO.take() else { return };
    ATUAL.set(PADRAO);
    if let Some((_, definir)) = DLL.get() {
        unsafe { definir(PADRAO) };
        avisar_todos();
        // tempo para os programas tirarem a mensagem da fila antes de o gancho sair
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    unsafe { _ = UnhookWindowsHookEx(g) };
}
