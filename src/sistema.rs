//! O que a barra pede ao Windows: esconder a barra dele, abrir o Iniciar e os painéis,
//! preferências no registro e início junto com o Windows.

use std::ffi::c_void;

use windows::core::{w, PCWSTR, PWSTR};
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

/// Tarefa agendada que abre a barra elevada no logon. Pela chave Run ela abriria sem elevação,
/// e aí o Windows (UIPI) não deixa o gancho de teclado ver o Alt+Tab e o Win+Tab com um
/// programa elevado em primeiro plano, nem os ganchos do tema entrarem nos programas elevados.
const TAREFA: &str = "barra";

fn schtasks(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("schtasks")
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .is_ok_and(|s| s.status.success())
}

fn tem_tarefa() -> bool {
    schtasks(&["/Query", "/TN", TAREFA])
}

fn tem_run() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, RRF_RT_REG_SZ, None, None, None).is_ok() }
}

/// Cria (ou refaz) a tarefa do logon. Só funciona com a barra elevada.
fn criar_tarefa() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let usuario = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    // sem limite de tempo (o padrão do schtasks para a tarefa em 72 h), na bateria também e
    // com prioridade normal (o padrão de tarefa é abaixo do normal)
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{u}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{u}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{e}</Command></Exec></Actions>
</Task>
"#,
        u = esc(&usuario),
        e = esc(&exe.display().to_string())
    );
    let caminho = std::env::temp_dir().join("barra-tarefa.xml");
    let bytes: Vec<u8> = [0xFEFFu16].into_iter().chain(xml.encode_utf16()).flat_map(u16::to_le_bytes).collect();
    if std::fs::write(&caminho, bytes).is_err() {
        return false;
    }
    let ok = schtasks(&["/Create", "/TN", TAREFA, "/XML", &caminho.to_string_lossy(), "/F"]);
    _ = std::fs::remove_file(&caminho);
    ok
}

/// Interface XAML do Explorador do Windows 11 (abas, barra de comandos): anulada no HKCU, o
/// Explorador volta ao layout com a faixa de opções, sem abas, todo em Win32 e DirectUI.
const EXPLORADOR_XAML: [PCWSTR; 2] = [
    w!("Software\\Classes\\CLSID\\{2aa9162e-c906-4dd9-ad0b-3d24a8eef5a0}"),
    w!("Software\\Classes\\CLSID\\{6480100b-5a83-4d1e-9f69-8ae5a88e9a33}"),
];

/// Explorador clássico, sem abas (vale para as janelas abertas depois).
pub fn explorador_classico() -> bool {
    EXPLORADOR_XAML.iter().all(|c| unsafe {
        let mut k = HKEY::default();
        let ok = RegOpenKeyExW(HKEY_CURRENT_USER, *c, None, KEY_READ, &mut k).is_ok();
        if ok {
            _ = RegCloseKey(k);
        }
        ok
    })
}

pub fn definir_explorador_classico(ligar: bool) {
    if ligar {
        faixa_recolhida();
    }
    for c in EXPLORADOR_XAML {
        unsafe {
            if ligar {
                // InprocServer32 com o valor padrão vazio: a classe não carrega
                let mut k = HKEY::default();
                let sub = format!("{}\\InprocServer32\0", c.to_string().unwrap_or_default());
                let sub: Vec<u16> = sub.encode_utf16().collect();
                if RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None).is_ok() {
                    _ = RegSetValueExW(k, None, None, REG_SZ, Some(&[0, 0]));
                    _ = RegCloseKey(k);
                }
            } else {
                _ = RegDeleteTreeW(HKEY_CURRENT_USER, c);
            }
        }
    }
}

/// Faixa de opções do Explorador recolhida (só a fileira das abas), como o Ctrl+F1 deixa.
fn faixa_recolhida() {
    unsafe {
        let mut k = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Ribbon"), None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None).is_ok() {
            _ = RegSetValueExW(k, w!("MinimizedStateTabletModeOff"), None, REG_DWORD, Some(&1u32.to_le_bytes()));
            _ = RegCloseKey(k);
        }
    }
}

pub fn inicia_com_windows() -> bool {
    tem_run() || tem_tarefa()
}

/// Liga ou desliga o início com o Windows. Elevada, usa a tarefa agendada; sem elevação, a
/// chave Run (criar tarefa elevada pede administrador).
pub fn definir_inicio(ligar: bool) {
    if !ligar {
        if tem_tarefa() {
            schtasks(&["/Delete", "/TN", TAREFA, "/F"]);
        }
        definir_run(false);
    } else if unsafe { IsUserAnAdmin() }.as_bool() && criar_tarefa() {
        definir_run(false);
    } else {
        definir_run(true);
    }
}

/// Ao abrir elevada: troca a chave Run pela tarefa agendada.
pub fn migrar_inicio() {
    if !unsafe { IsUserAnAdmin() }.as_bool() {
        return;
    }
    if tem_run() && criar_tarefa() {
        definir_run(false);
    }
}

fn definir_run(ligar: bool) {
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


/// Navegador padrão: o executável que abre links https.
fn navegador_padrao() -> Option<String> {
    unsafe {
        let mut n = 0u32;
        _ = AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, w!("https"), w!("open"), None, &mut n);
        let mut b = vec![0u16; n.max(1) as usize];
        AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, w!("https"), w!("open"), Some(PWSTR(b.as_mut_ptr())), &mut n).ok().ok()?;
        let t = String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
        (!t.is_empty()).then_some(t)
    }
}

/// Abre uma janela nova do navegador padrão. Quem abre é o Explorer: a barra roda como
/// administrador, e o navegador aberto por ela também rodaria.
pub fn abrir_navegador() {
    match navegador_padrao() {
        Some(exe) => _ = std::process::Command::new("explorer.exe").arg(exe).spawn(),
        None => abrir_uri(w!("https:")),
    }
}

pub fn abrir_uri(uri: PCWSTR) {
    unsafe { ShellExecuteW(None, w!("open"), uri, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
}

/// Põe `texto` na área de transferência (CF_UNICODETEXT).
pub fn copiar(dono: HWND, texto: &str) -> bool {
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    const CF_UNICODETEXT: u32 = 13;
    let u: Vec<u16> = texto.encode_utf16().chain([0]).collect();
    unsafe {
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, u.len() * 2) else { return false };
        let p = GlobalLock(mem) as *mut u16;
        if p.is_null() {
            _ = GlobalFree(Some(mem));
            return false;
        }
        std::ptr::copy_nonoverlapping(u.as_ptr(), p, u.len());
        _ = GlobalUnlock(mem);
        if OpenClipboard(Some(dono)).is_err() {
            _ = GlobalFree(Some(mem));
            return false;
        }
        _ = EmptyClipboard();
        // com sucesso, a memória passa a ser do sistema
        let ok = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).is_ok();
        if !ok {
            _ = GlobalFree(Some(mem));
        }
        _ = CloseClipboard();
        ok
    }
}
