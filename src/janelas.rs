//! Quais janelas viram botão na barra (o mesmo critério da barra do Windows), título e ícone.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Fica de fora como na barra do Windows: área de trabalho e janelas de sistema sem botão.
const CLASSES_FORA: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

pub fn classe(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

pub fn titulo(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

/// Escondida pelo DWM: outra área de trabalho virtual, ou app da loja suspenso.
pub fn cloaked(h: HWND) -> bool {
    let mut v = 0u32;
    unsafe { _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut v as *mut _ as *mut _, 4) };
    v != 0
}

/// Janela que ganha botão na barra.
pub fn de_tarefa(h: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(h).as_bool() {
            return false;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        let app = ex & WS_EX_APPWINDOW.0 != 0;
        if !app {
            if ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_NOACTIVATE.0 != 0 {
                return false;
            }
            if GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
                return false;
            }
        }
        if cloaked(h) || GetWindowTextLengthW(h) == 0 {
            return false;
        }
        !CLASSES_FORA.contains(&classe(h).as_str())
    }
}

unsafe extern "system" fn juntar(h: HWND, lp: LPARAM) -> BOOL {
    let v = &mut *(lp.0 as *mut Vec<isize>);
    if de_tarefa(h) {
        v.push(h.0 as isize);
    }
    TRUE
}

/// Janelas com botão, da frente para trás.
pub fn listar() -> Vec<isize> {
    let mut v: Vec<isize> = Vec::new();
    unsafe { _ = EnumWindows(Some(juntar), LPARAM(&mut v as *mut _ as isize)) };
    v
}

pub fn hwnd(h: isize) -> HWND {
    HWND(h as *mut _)
}

pub fn existe(h: isize) -> bool {
    unsafe { IsWindow(Some(hwnd(h))).as_bool() }
}

fn caminho_do_processo(h: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n);
        _ = CloseHandle(proc);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

thread_local! {
    static ICONES_EXE: RefCell<HashMap<String, HICON>> = RefCell::new(HashMap::new());
}

/// Ícone do executável no tamanho pedido, pelo shell (fica nítido em qualquer tamanho).
fn icone_do_exe(h: HWND, tam: i32) -> Option<HICON> {
    let caminho = caminho_do_processo(h)?;
    if let Some(i) = ICONES_EXE.with_borrow(|c| c.get(&caminho).copied()) {
        return Some(i);
    }
    let icone = unsafe {
        let w: Vec<u16> = caminho.encode_utf16().chain([0]).collect();
        let f: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok()?;
        let cor = f.GetImage(SIZE { cx: tam, cy: tam }, SIIGBF_ICONONLY).ok()?;
        let mascara = CreateBitmap(tam, tam, 1, 1, None);
        let i = CreateIconIndirect(&ICONINFO { fIcon: true.into(), hbmMask: mascara, hbmColor: cor, ..Default::default() }).ok();
        _ = DeleteObject(mascara.into());
        _ = DeleteObject(cor.into());
        i?
    };
    ICONES_EXE.with_borrow_mut(|c| c.insert(caminho, icone));
    Some(icone)
}

fn pedir_icone(h: HWND, tipo: u32) -> Option<HICON> {
    let mut r = 0usize;
    let ok = unsafe { SendMessageTimeoutW(h, WM_GETICON, WPARAM(tipo as usize), LPARAM(0), SMTO_ABORTIFHUNG, 60, Some(&mut r)) };
    (ok.0 != 0 && r != 0).then(|| HICON(r as *mut _))
}

fn icone_da_classe(h: HWND, indice: GET_CLASS_LONG_INDEX) -> Option<HICON> {
    let r = unsafe { GetClassLongPtrW(h, indice) };
    (r != 0).then(|| HICON(r as *mut _))
}

/// Ícone da janela: o que ela mesma informa (navegador com perfil, por exemplo), senão o do
/// executável. Não é dono do HICON: não destruir.
pub fn icone(h: HWND, tam: i32) -> HICON {
    pedir_icone(h, ICON_BIG)
        .or_else(|| icone_da_classe(h, GCLP_HICON))
        .or_else(|| icone_do_exe(h, tam))
        .or_else(|| pedir_icone(h, ICON_SMALL2))
        .or_else(|| icone_da_classe(h, GCLP_HICONSM))
        .unwrap_or_default()
}
