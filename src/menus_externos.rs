//! Menus Win32 dos outros programas (bandeja, clique direito, menu de sistema, barra de menus
//! clássica) e botões da barra de título no tema. A barra instala ganchos globais e o Windows carrega a barra_menus.dll em
//! cada programa com janela; a DLL faz o resto lá dentro (ver menus/src/lib.rs).
//! Só alcança programas de 64 bits que não rodam como administrador.
//!
//! A DLL fica presa nos programas até eles fecharem, então a barra carrega uma cópia com a
//! versão no nome (em %LOCALAPPDATA%\barra\menus): o arquivo do build nunca fica travado.

use std::cell::Cell;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use windows::core::{s, PCSTR, PCWSTR};
use windows::Win32::Foundation::{HMODULE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::tema::Tema;

const PADRAO: u32 = 0;
const ESCURO: u32 = 2;
const CLARO: u32 = 3;

type DefinirTema = unsafe extern "system" fn(u32, *const u32);
type Gancho = unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT;

thread_local! {
    static DLL: Cell<Option<(HMODULE, DefinirTema)>> = const { Cell::new(None) };
    static GANCHOS: Cell<Option<[HHOOK; 3]>> = const { Cell::new(None) };
    /// Tema já passado para a DLL.
    static ATUAL: Cell<&'static str> = const { Cell::new("") };
}

fn pasta() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("barra").join("menus"))
}

/// Copia a DLL do lado do barra.exe para a pasta das cópias, com data e tamanho no nome, e
/// apaga as cópias antigas que nenhum programa segura mais.
fn copia() -> Option<PathBuf> {
    let origem = std::env::current_exe().ok()?.with_file_name("barra_menus.dll");
    let meta = std::fs::metadata(&origem).ok()?;
    let data = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let dir = pasta()?;
    std::fs::create_dir_all(&dir).ok()?;
    let destino = dir.join(format!("barra_menus-{data}-{}.dll", meta.len()));
    if !destino.exists() {
        std::fs::copy(&origem, &destino).ok()?;
    }
    if let Ok(lista) = std::fs::read_dir(&dir) {
        for e in lista.flatten() {
            if e.path() != destino {
                _ = std::fs::remove_file(e.path()); // falha enquanto algum programa usa: fica
            }
        }
    }
    Some(destino)
}

fn dll() -> Option<(HMODULE, DefinirTema)> {
    if let Some(d) = DLL.get() {
        return Some(d);
    }
    let caminho = copia()?;
    let w: Vec<u16> = caminho.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        let m = LoadLibraryW(PCWSTR(w.as_ptr())).ok()?;
        let definir: DefinirTema = std::mem::transmute(GetProcAddress(m, s!("definir_tema"))?);
        DLL.set(Some((m, definir)));
        Some((m, definir))
    }
}

/// Cada programa aplica o tema novo na próxima mensagem que tirar da fila; uma mensagem
/// vazia para todas as janelas faz isso acontecer agora.
fn avisar_todos() {
    unsafe { _ = PostMessageW(Some(HWND_BROADCAST), WM_NULL, WPARAM(0), LPARAM(0)) };
}

/// Cores na ordem que a DLL espera: fundo, fundo_claro, selecao, mudo, texto, texto_forte,
/// destaque, apagado (texto misturado 30% com o fundo, como na barra), vermelho.
fn cores(t: &Tema) -> [u32; 9] {
    let mistura = |a: u32, b: u32, f: f32| {
        let canal = |s: u32| {
            let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
            ((x + (y - x) * f).round() as u32) << s
        };
        canal(16) | canal(8) | canal(0)
    };
    [t.fundo, t.fundo_claro, t.selecao, t.mudo, t.texto, t.texto_forte, t.destaque, mistura(t.texto, t.fundo, 0.3), t.vermelho]
}

/// Liga ou troca o tema. Chamado a cada atualização da barra; só age quando algo mudou.
pub fn ligar(tema: &'static Tema) {
    if GANCHOS.get().is_some() && ATUAL.get() == tema.nome {
        return;
    }
    let Some((m, definir)) = dll() else { return };
    let c = cores(tema);
    unsafe { definir(if tema.claro { CLARO } else { ESCURO }, c.as_ptr()) };
    ATUAL.set(tema.nome);
    if GANCHOS.get().is_none() {
        let instalar = |tipo: WINDOWS_HOOK_ID, nome: PCSTR| unsafe {
            let f: Gancho = std::mem::transmute(GetProcAddress(m, nome)?);
            SetWindowsHookExW(tipo, Some(f), Some(m.into()), 0).ok()
        };
        let ganchos = (instalar(WH_GETMESSAGE, s!("gancho")), instalar(WH_CALLWNDPROCRET, s!("gancho_retorno")), instalar(WH_CBT, s!("gancho_cbt")));
        match ganchos {
            (Some(a), Some(b), Some(c)) => GANCHOS.set(Some([a, b, c])),
            (a, b, c) => {
                for h in [a, b, c].into_iter().flatten() {
                    unsafe { _ = UnhookWindowsHookEx(h) };
                }
            }
        }
    }
    avisar_todos();
}

/// Devolve o padrão do Windows a todos os programas e tira os ganchos. A DLL continua nos
/// programas que já a usaram, só passando as mensagens adiante.
pub fn desligar() {
    let Some(g) = GANCHOS.take() else { return };
    ATUAL.set("");
    if let Some((_, definir)) = DLL.get() {
        unsafe { definir(PADRAO, std::ptr::null()) };
        avisar_todos();
        // tempo para os programas tirarem a mensagem da fila antes de os ganchos saírem
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    for h in g {
        unsafe { _ = UnhookWindowsHookEx(h) };
    }
}
