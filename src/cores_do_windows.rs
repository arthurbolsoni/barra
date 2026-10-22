//! Cores do Windows no tema: cor de destaque e modo claro ou escuro, para os programas que
//! seguem o Windows (Configurações, Explorador, apps WinUI, navegadores) se adaptarem.
//!
//! A cor de destaque vai pelo SetUserColorPreference da uxtheme (ordinal 122), o mesmo caminho
//! das Configurações: o Windows gera a paleta e os programas abertos trocam na hora. O modo vai
//! pelo registro (AppsUseLightTheme, SystemUsesLightTheme) com o aviso ImmersiveColorSet.
//! A política "Forçar uma cor de fundo e de destaque" (PersonalColors_* em HKLM) faz o Windows
//! ignorar o pedido; ela sai enquanto o tema está aplicado, o que exige a barra como administrador.
//!
//! Antes de mudar qualquer coisa, os valores de antes vão para HKCU\Software\barra\CoresDoWindows
//! e voltam na saída. A cópia só some depois de devolvida: se a barra cair, a próxima saída devolve.

use std::cell::Cell;
use std::ffi::c_void;

use windows::core::{w, PCSTR, PCWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};

use crate::sistema;
use crate::tema::Tema;

/// Preferência: cores do Windows no tema (padrão: sim).
pub const PREF: PCWSTR = w!("CoresDoWindows");
const COPIA: PCWSTR = w!("Software\\barra\\CoresDoWindows");
const POLITICA: PCWSTR = w!("Software\\Policies\\Microsoft\\Windows\\Personalization");
const DESTAQUE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent");
const DWM: PCWSTR = w!("Software\\Microsoft\\Windows\\DWM");
const MODO: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");

/// O que muda quando o tema é aplicado, pela barra ou pelo Windows a pedido dela. Os nomes não
/// se repetem, então a cópia usa o próprio nome do valor.
const VALORES: [(HKEY, PCWSTR, PCWSTR); 10] = [
    (HKEY_LOCAL_MACHINE, POLITICA, w!("PersonalColors_Background")),
    (HKEY_LOCAL_MACHINE, POLITICA, w!("PersonalColors_Accent")),
    (HKEY_CURRENT_USER, DESTAQUE, w!("AccentPalette")),
    (HKEY_CURRENT_USER, DESTAQUE, w!("AccentColorMenu")),
    (HKEY_CURRENT_USER, DESTAQUE, w!("StartColorMenu")),
    (HKEY_CURRENT_USER, DWM, w!("AccentColor")),
    (HKEY_CURRENT_USER, DWM, w!("ColorizationColor")),
    (HKEY_CURRENT_USER, DWM, w!("ColorizationAfterglow")),
    (HKEY_CURRENT_USER, MODO, w!("AppsUseLightTheme")),
    (HKEY_CURRENT_USER, MODO, w!("SystemUsesLightTheme")),
];
/// Posições de StartColorMenu e AccentColorMenu em VALORES.
const I_INICIO: usize = 4;
const I_DESTAQUE: usize = 3;

/// IMMERSIVE_COLOR_PREFERENCE: cor do Iniciar e cor de destaque, em COLORREF.
#[repr(C)]
#[derive(Default)]
struct Preferencia {
    inicio: u32,
    destaque: u32,
}

#[derive(Clone, Copy)]
struct Uxtheme {
    ler: unsafe extern "system" fn(*mut Preferencia, bool) -> i32,
    gravar: unsafe extern "system" fn(*const Preferencia, bool) -> i32,
    /// RefreshImmersiveColorPolicyState: relê a política neste processo.
    reler_politica: unsafe extern "system" fn(),
}

thread_local! {
    static LIGADAS: Cell<bool> = Cell::new(sistema::ler(PREF) != Some(0));
    /// Tema já aplicado ao Windows ("": nenhum).
    static ATUAL: Cell<&'static str> = const { Cell::new("") };
    static UXTHEME: Cell<Option<Uxtheme>> = const { Cell::new(None) };
}

fn uxtheme() -> Option<Uxtheme> {
    if let Some(u) = UXTHEME.get() {
        return Some(u);
    }
    unsafe {
        let m = LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let u = Uxtheme {
            ler: std::mem::transmute(GetProcAddress(m, windows::core::s!("GetUserColorPreference"))?),
            gravar: std::mem::transmute(GetProcAddress(m, PCSTR(122 as *const u8))?),
            reler_politica: std::mem::transmute(GetProcAddress(m, PCSTR(104 as *const u8))?),
        };
        UXTHEME.set(Some(u));
        Some(u)
    }
}

/// 0xRRGGBB do tema para COLORREF (0x00BBGGRR).
fn colorref(c: u32) -> u32 {
    (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16) & 0xFF
}

/// Valor como está no registro: tipo e bytes (None: não existe).
fn ler_valor(raiz: HKEY, chave: PCWSTR, nome: PCWSTR) -> Option<(REG_VALUE_TYPE, Vec<u8>)> {
    let mut tipo = REG_VALUE_TYPE::default();
    let mut dados = [0u8; 256];
    let mut tam = dados.len() as u32;
    let r = unsafe {
        RegGetValueW(raiz, chave, nome, RRF_RT_ANY | RRF_NOEXPAND, Some(&mut tipo), Some(dados.as_mut_ptr() as *mut c_void), Some(&mut tam))
    };
    (r == ERROR_SUCCESS).then(|| (tipo, dados[..tam as usize].to_vec()))
}

fn gravar_valor(raiz: HKEY, chave: PCWSTR, nome: PCWSTR, valor: Option<(REG_VALUE_TYPE, &[u8])>) {
    unsafe {
        match valor {
            Some((tipo, dados)) => _ = RegSetKeyValueW(raiz, chave, nome, tipo.0, Some(dados.as_ptr() as *const c_void), dados.len() as u32),
            None => _ = RegDeleteKeyValueW(raiz, chave, nome),
        }
    }
}

fn ler_dword(raiz: HKEY, chave: PCWSTR, nome: PCWSTR) -> Option<u32> {
    ler_valor(raiz, chave, nome).filter(|(t, d)| *t == REG_DWORD && d.len() == 4).map(|(_, d)| u32::from_le_bytes([d[0], d[1], d[2], d[3]]))
}

fn copia_existe() -> bool {
    let mut k = HKEY::default();
    let ok = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, COPIA, None, KEY_READ, &mut k) } == ERROR_SUCCESS;
    if ok {
        unsafe { _ = RegCloseKey(k) };
    }
    ok
}

/// Guarda os valores de antes, uma vez só: a cópia de uma sessão que caiu vale mais que o
/// estado atual, que já é o do tema. Valor que não existia vira REG_NONE vazio.
fn guardar() {
    if copia_existe() {
        return;
    }
    for (raiz, chave, nome) in VALORES {
        match ler_valor(raiz, chave, nome) {
            Some((tipo, dados)) => gravar_valor(HKEY_CURRENT_USER, COPIA, nome, Some((tipo, &dados))),
            None => gravar_valor(HKEY_CURRENT_USER, COPIA, nome, Some((REG_NONE, &[]))),
        }
    }
}

/// Avisa todas as janelas que as cores mudaram (o mesmo aviso das Configurações).
fn avisar() {
    let texto = w!("ImmersiveColorSet");
    unsafe { SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(texto.as_ptr() as isize), SMTO_ABORTIFHUNG, 1000, None) };
}

pub fn ligadas() -> bool {
    LIGADAS.get()
}

/// Liga ou desliga; desligar devolve as cores de antes na hora.
pub fn ligar(sim: bool) {
    LIGADAS.set(sim);
    if !sim {
        restaurar();
    }
}

/// Aplica a cor de destaque e o modo do tema. Chamado a cada atualização da barra; só age
/// quando o tema muda.
pub fn aplicar(tema: &'static Tema) {
    if !LIGADAS.get() || ATUAL.get() == tema.nome {
        return;
    }
    ATUAL.set(tema.nome);
    guardar();
    let claro = tema.claro as u32;
    let mut mudou = false;
    for nome in [w!("AppsUseLightTheme"), w!("SystemUsesLightTheme")] {
        if ler_dword(HKEY_CURRENT_USER, MODO, nome) != Some(claro) {
            gravar_valor(HKEY_CURRENT_USER, MODO, nome, Some((REG_DWORD, &claro.to_le_bytes())));
            mudou = true;
        }
    }
    if let Some(u) = uxtheme() {
        for (raiz, chave, nome) in &VALORES[..2] {
            gravar_valor(*raiz, *chave, *nome, None);
        }
        let mut p = Preferencia::default();
        unsafe {
            (u.reler_politica)();
            let cor = colorref(tema.destaque);
            if (u.ler)(&mut p, true) >= 0 && p.destaque & 0xFF_FFFF != cor {
                p.destaque = cor;
                (u.gravar)(&p, true); // avisa os programas por conta própria
            }
        }
    }
    if mudou {
        avisar();
    }
}

/// Saída, desligamento, pânico e opção desligada: devolve as cores de antes, política inclusive.
pub fn restaurar() {
    ATUAL.set("");
    if !copia_existe() {
        return;
    }
    let antes: Vec<_> = VALORES.iter().map(|&(_, _, nome)| ler_valor(HKEY_CURRENT_USER, COPIA, nome).filter(|(t, _)| *t != REG_NONE)).collect();
    if let Some(u) = uxtheme() {
        // primeiro a cor de antes pelo caminho das Configurações, que faz os programas abertos
        // relerem a paleta; logo depois os bytes exatos de antes, que é o que eles leem
        let dword = |i: usize| antes[i].as_ref().filter(|(_, d)| d.len() == 4).map(|(_, d)| u32::from_le_bytes([d[0], d[1], d[2], d[3]]) & 0xFF_FFFF);
        if let (Some(inicio), Some(destaque)) = (dword(I_INICIO), dword(I_DESTAQUE)) {
            unsafe { (u.gravar)(&Preferencia { inicio, destaque }, true) };
        }
    }
    for ((raiz, chave, nome), valor) in VALORES.iter().zip(&antes) {
        gravar_valor(*raiz, *chave, *nome, valor.as_ref().map(|(t, d)| (*t, d.as_slice())));
    }
    unsafe { _ = RegDeleteTreeW(HKEY_CURRENT_USER, COPIA) };
    if let Some(u) = uxtheme() {
        unsafe { (u.reler_politica)() };
    }
    avisar();
}
