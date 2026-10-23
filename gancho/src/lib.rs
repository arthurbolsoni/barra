//! DLL dos ganchos globais que a barra instala (SetWindowsHookEx). O Windows a carrega em cada
//! programa com janela; ela carrega lá dentro a barra_menus.dll da versão em uso e repassa os
//! ganchos para ela.
//!
//! O Windows guarda cada DLL de gancho global numa tabela de 32 posições por sessão, e a
//! posição só vaga quando todos os programas que carregaram a DLL a soltam, o que um programa
//! parado não faz. Com a barra_menus.dll direto no gancho, cada versão nova ocupava mais uma
//! posição até a tabela encher, e o SetWindowsHookEx passava a falhar (ERROR_MOD_NOT_FOUND).
//! Esta DLL quase nunca muda, e a barra a usa sempre pelo mesmo caminho enquanto ela não muda:
//! uma posição só. A barra_menus.dll entra por LoadLibrary, fora da tabela.
//!
//! Nada aqui pode entrar em pânico nem travar: roda dentro de outros programas.

use std::panic::catch_unwind;
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::CallNextHookEx;

const TAMANHO: usize = 520;

/// Caminho da barra_menus.dll em uso, terminado em 0. Memória comum a todos os processos.
#[link_section = ".barra"]
static CAMINHO: [AtomicU16; TAMANHO] = [const { AtomicU16::new(0) }; TAMANHO];

/// Sobe a cada caminho novo; ímpar enquanto a barra escreve. 0: nenhum caminho ainda.
#[link_section = ".barra"]
static SEQUENCIA: AtomicU32 = AtomicU32::new(0);

/// Chamado pela barra: a barra_menus.dll que os programas devem usar.
///
/// # Safety
/// `caminho` aponta para um texto terminado em 0.
#[no_mangle]
pub unsafe extern "system" fn definir_implementacao(caminho: *const u16) {
    let s = SEQUENCIA.fetch_add(1, Ordering::AcqRel) + 1;
    let mut fim = false;
    for (i, c) in CAMINHO.iter().enumerate() {
        let v = if fim || caminho.is_null() || i == TAMANHO - 1 { 0 } else { *caminho.add(i) };
        fim |= v == 0;
        c.store(v, Ordering::Relaxed);
    }
    SEQUENCIA.store(s + 1, Ordering::Release);
}

// ---------- neste processo ----------

/// Caminho já carregado neste processo (a sequência dele); u32::MAX: nenhum.
static CARREGADA: AtomicU32 = AtomicU32::new(u32::MAX);
/// Ganchos da barra_menus.dll carregada, na ordem de `NOMES`; 0: sem.
static FUNCOES: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];
static CARREGANDO: Mutex<()> = Mutex::new(());

const NOMES: [&std::ffi::CStr; 3] = [c"gancho", c"gancho_retorno", c"gancho_cbt"];

type Gancho = unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT;

/// Carrega a barra_menus.dll do caminho da sequência `s`. Se não carregar (programa sem acesso
/// à pasta, por exemplo), fica sem até a barra mudar o caminho.
fn carregar(s: u32) {
    if s % 2 == 1 {
        return; // a barra está escrevendo
    }
    // outra linha de execução carregando, ou esta mesma reentrando: fica para a próxima mensagem
    let Ok(_trava) = CARREGANDO.try_lock() else { return };
    if CARREGADA.load(Ordering::Acquire) == s {
        return;
    }
    let mut caminho = [0u16; TAMANHO];
    for (d, c) in caminho.iter_mut().zip(&CAMINHO) {
        *d = c.load(Ordering::Relaxed);
    }
    if SEQUENCIA.load(Ordering::Acquire) != s {
        return; // mudou durante a leitura
    }
    caminho[TAMANHO - 1] = 0;
    let modulo = (caminho[0] != 0).then(|| unsafe { LoadLibraryW(PCWSTR(caminho.as_ptr())) }.ok()).flatten();
    for (f, nome) in FUNCOES.iter().zip(NOMES) {
        let p = modulo.and_then(|m| unsafe { GetProcAddress(m, PCSTR(nome.as_ptr() as *const u8)) });
        f.store(p.map_or(0, |p| p as usize), Ordering::Release);
    }
    CARREGADA.store(s, Ordering::Release);
}

fn funcao(i: usize) -> Option<Gancho> {
    let s = SEQUENCIA.load(Ordering::Acquire);
    if CARREGADA.load(Ordering::Acquire) != s {
        carregar(s);
    }
    match FUNCOES[i].load(Ordering::Acquire) {
        0 => None,
        f => Some(unsafe { std::mem::transmute::<usize, Gancho>(f) }),
    }
}

/// A barra_menus.dll chama o próximo gancho; sem ela, chama aqui.
fn repassar(i: usize, code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match catch_unwind(|| funcao(i)).ok().flatten() {
        Some(f) => unsafe { f(code, wp, lp) },
        None => unsafe { CallNextHookEx(None, code, wp, lp) },
    }
}

/// WH_GETMESSAGE.
#[no_mangle]
pub extern "system" fn gancho(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    repassar(0, code, wp, lp)
}

/// WH_CALLWNDPROCRET.
#[no_mangle]
pub extern "system" fn gancho_retorno(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    repassar(1, code, wp, lp)
}

/// WH_CBT.
#[no_mangle]
pub extern "system" fn gancho_cbt(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    repassar(2, code, wp, lp)
}
