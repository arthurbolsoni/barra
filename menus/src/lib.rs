//! Carregada pelo Windows em cada programa com janela, pelos ganchos globais que a barra
//! instala (SetWindowsHookEx). Põe no tema da barra os menus Win32 dos outros programas:
//!
//! - modo claro ou escuro do uxtheme (SetPreferredAppMode, ordinal 135, e FlushMenuThemes,
//!   136), a técnica do mod "Dark mode context menus" do Windhawk;
//! - menus suspensos (bandeja, clique direito, menu de sistema): no WM_INITMENUPOPUP os itens
//!   viram desenho próprio só enquanto o menu está aberto, pintados com a fonte e as cores do
//!   tema, e voltam ao normal no WM_UNINITMENUPOPUP;
//! - janela do menu: borda de 2 px em destaque, cantos retos, sem sombra;
//! - barra de menus clássica ("Arquivo  Editar…"): pelas mensagens WM_UAHDRAWMENU e
//!   WM_UAHDRAWMENUITEM, que o Windows manda para a janela desenhar a faixa;
//! - botões da barra de título (minimizar, maximizar, fechar): sai o WS_SYSMENU, e o DWM deixa
//!   de desenhar os dele; no lugar fica uma janela da própria janela (owned popup), com os
//!   glifos do tema.
//!
//! - controles Win32 (botões, caixas de seleção, abas, listas, campos…) e cores do sistema:
//!   ver `controles`.
//!
//! Menus que o próprio programa desenha (itens owner-draw ou com bitmap de callback, como os do
//! Explorer) ficam só com o modo escuro. Nada aqui pode entrar em pânico: um pânico dentro de
//! outro programa derrubaria o programa.


mod controles;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use windows::core::{w, PCSTR, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, WM_MOUSELEAVE, ODS_CHECKED, ODS_DISABLED, ODS_GRAYED, ODS_HOTLIGHT, ODS_INACTIVE, ODS_NOACCEL, ODS_SELECTED, ODT_MENU};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi, GetWindowDpiAwarenessContext, SetThreadDpiAwarenessContext};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

// ---------- o que a barra pede (memória comum a todos os processos) ----------

/// Modos do SetPreferredAppMode. PADRAO também quer dizer "desligado".
pub const PADRAO: u32 = 0;
pub const ESCURO: u32 = 2;
pub const CLARO: u32 = 3;

#[link_section = ".barra"]
static MODO: AtomicU32 = AtomicU32::new(PADRAO);

/// Cores do tema, 0xRRGGBB: fundo, fundo_claro, selecao, mudo, texto, texto_forte, destaque,
/// apagado (texto misturado 30% com o fundo), vermelho.
#[link_section = ".barra"]
static CORES: [AtomicU32; 9] = [const { AtomicU32::new(0) }; 9];

/// Sobe a cada pedido da barra (tema novo, ligar, desligar): cada linha de execução com janela
/// refaz as suas quando vê um número diferente.
#[link_section = ".barra"]
static VERSAO: AtomicU32 = AtomicU32::new(0);

const FUNDO: usize = 0;
const FUNDO_CLARO: usize = 1;
const SELECAO: usize = 2;
const MUDO: usize = 3;
const TEXTO: usize = 4;
const TEXTO_FORTE: usize = 5;
const DESTAQUE: usize = 6;
const APAGADO: usize = 7;
const VERMELHO: usize = 8;

/// Chamado pela barra: modo e as 9 cores, na ordem de `CORES`.
///
/// # Safety
/// `cores` aponta para 9 valores.
#[no_mangle]
pub unsafe extern "system" fn definir_tema(modo: u32, cores: *const u32) {
    if !cores.is_null() {
        for (i, c) in CORES.iter().enumerate() {
            c.store(*cores.add(i), Ordering::Relaxed);
        }
    }
    MODO.store(modo, Ordering::Relaxed);
    VERSAO.fetch_add(1, Ordering::Relaxed);
}

fn ligado() -> bool {
    matches!(MODO.load(Ordering::Relaxed), ESCURO | CLARO)
}

/// Cor do tema como COLORREF (0x00BBGGRR).
fn cor(papel: usize) -> COLORREF {
    let c = CORES[papel].load(Ordering::Relaxed);
    COLORREF((c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16) & 0xFF)
}

/// Executa sem deixar um pânico escapar para o programa hospedeiro.
fn seguro<R>(padrao: R, f: impl FnOnce() -> R) -> R {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(padrao)
}

// ---------- modo claro/escuro do uxtheme ----------

/// Modo já aplicado neste processo (u32::MAX: nenhum).
static APLICADO: AtomicU32 = AtomicU32::new(u32::MAX);

fn aplicar_modo() {
    let modo = MODO.load(Ordering::Relaxed);
    if !matches!(modo, PADRAO | ESCURO | CLARO) || APLICADO.swap(modo, Ordering::Relaxed) == modo {
        return;
    }
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

/// A DLL não pode sair do processo enquanto houver janela com subclasse apontando para ela:
/// fica presa até o programa fechar. Desligada, a subclasse só passa as mensagens adiante.
fn prender() {
    static PRESA: AtomicBool = AtomicBool::new(false);
    if PRESA.swap(true, Ordering::Relaxed) {
        return;
    }
    let mut m = HMODULE::default();
    unsafe {
        _ = GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            PCWSTR(prender as *const () as *const u16),
            &mut m,
        );
    }
}

// ---------- ganchos ----------

/// WH_GETMESSAGE: aplica o modo claro/escuro na primeira mensagem do processo e depois de
/// cada troca de tema; os controles da linha de execução relêem cores e tema (`controles`).
///
/// A mensagem vazia que a barra manda a todas as janelas ao ligar põe os botões do tema nas
/// janelas que já estavam abertas.
#[no_mangle]
pub extern "system" fn gancho(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    seguro((), aplicar_modo);
    seguro((), || controles::sincronizar(VERSAO.load(Ordering::Relaxed)));
    if code >= 0 && lp.0 != 0 && ligado() {
        seguro((), || unsafe {
            let m = &*(lp.0 as *const MSG);
            if m.message == WM_NULL {
                vestir_botoes(m.hwnd);
            }
        });
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

/// WH_CALLWNDPROCRET: depois de o programa preparar o menu (WM_INITMENUPOPUP), converte os
/// itens; ao fechar (WM_UNINITMENUPOPUP), devolve. Janela com barra de menus ganha a subclasse
/// que pinta a faixa; janela com barra de título do Windows ganha os botões do tema.
#[no_mangle]
pub extern "system" fn gancho_retorno(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && lp.0 != 0 {
        seguro((), || unsafe {
            let m = &*(lp.0 as *const CWPRETSTRUCT);
            match m.message {
                WM_INITMENUPOPUP if ligado() => converter(HMENU(m.wParam.0 as *mut _), m.hwnd),
                WM_UNINITMENUPOPUP => devolver(HMENU(m.wParam.0 as *mut _)),
                WM_NCPAINT | WM_NCACTIVATE | WM_ACTIVATE if ligado() => {
                    vestir_barra_de_menus(m.hwnd);
                    vestir_botoes(m.hwnd);
                }
                WM_WINDOWPOSCHANGED if ligado() => vestir_botoes(m.hwnd),
                _ => {}
            }
        });
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

/// WH_CBT: janela de menu (#32768) recém-criada ganha a moldura do tema; diálogo e dica, o
/// tema dos controles.
#[no_mangle]
pub extern "system" fn gancho_cbt(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HCBT_CREATEWND as i32 && ligado() {
        seguro((), || unsafe {
            let h = HWND(wp.0 as *mut _);
            let c = &*(lp.0 as *const CBT_CREATEWNDW);
            let cs = &*c.lpcs;
            if cs.lpszClass.0 as usize == 0x8000 {
                vestir_janela_de_menu(h, cs.hwndParent);
            } else {
                controles::criada(h);
            }
        });
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

// ---------- fontes e desenho ----------

struct Fontes(Vec<((u32, u32, bool), isize)>);

static FONTES: Mutex<Fontes> = Mutex::new(Fontes(Vec::new()));

/// CaskaydiaMono Nerd Font em 12 px na escala do monitor.
fn fonte(dpi: u32, negrito: bool) -> HFONT {
    fonte_de(dpi, 12, negrito)
}

/// CaskaydiaMono Nerd Font em `tamanho` px na escala do monitor.
fn fonte_de(dpi: u32, tamanho: u32, negrito: bool) -> HFONT {
    let mut f = FONTES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&(_, h)) = f.0.iter().find(|(k, _)| *k == (dpi, tamanho, negrito)) {
        return HFONT(h as *mut _);
    }
    let h = unsafe {
        CreateFontW(
            -(((tamanho * dpi + 48) / 96) as i32), 0, 0, 0,
            if negrito { FW_BOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
            0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0,
            w!("CaskaydiaMono NF"),
        )
    };
    f.0.push(((dpi, tamanho, negrito), h.0 as isize));
    h
}

fn px(v: i32, dpi: u32) -> i32 {
    (v * dpi as i32 + 48) / 96
}

fn dpi_de(h: HWND) -> u32 {
    match unsafe { GetDpiForWindow(h) } {
        0 => 96,
        d => d,
    }
}

fn largura(dc: HDC, texto: &[u16]) -> i32 {
    let mut s = SIZE::default();
    unsafe { _ = GetTextExtentPoint32W(dc, texto, &mut s) };
    s.cx
}

/// Largura do texto como o DrawText mostra: sem os "&" de tecla de acesso.
fn largura_visivel(dc: HDC, texto: &[u16]) -> i32 {
    let mut limpo = Vec::with_capacity(texto.len());
    let mut i = 0;
    while i < texto.len() {
        if texto[i] == b'&' as u16 && i + 1 < texto.len() {
            i += 1;
        }
        limpo.push(texto[i]);
        i += 1;
    }
    largura(dc, &limpo)
}

fn preencher(dc: HDC, r: RECT, c: COLORREF) {
    unsafe {
        let b = CreateSolidBrush(c);
        FillRect(dc, &r, b);
        _ = DeleteObject(b.into());
    }
}

fn escrever(dc: HDC, texto: &[u16], mut r: RECT, c: COLORREF, formato: DRAW_TEXT_FORMAT) {
    if texto.is_empty() {
        return;
    }
    let mut t = texto.to_vec();
    unsafe {
        SetTextColor(dc, c);
        SetBkMode(dc, TRANSPARENT);
        DrawTextW(dc, &mut t, &mut r, formato | DT_SINGLELINE | DT_VCENTER);
    }
}

fn glifo(cp: u16) -> [u16; 1] {
    [cp]
}

const G_MARCA: u16 = 0xf00c;
const G_RADIO: u16 = 0xf111;
const G_SETA: u16 = 0xf105;

/// Glifo no lugar dos bitmaps de sistema (menu de sistema: restaurar, minimizar…).
fn glifo_de_bitmap(b: HBITMAP) -> Option<u16> {
    match b.0 as isize {
        8 | 5 | 6 => Some(0xf00d), // fechar
        9 | 2 => Some(0xf2d2),     // restaurar
        10 => Some(0xf2d0),        // maximizar
        11 | 3 | 7 => Some(0xf2d1), // minimizar
        _ => None,
    }
}

/// Bitmap do item (ícone) reduzido ao lado, com alfa quando for de 32 bits.
fn desenhar_bitmap(dc: HDC, b: HBITMAP, x: i32, y: i32, lado: i32) {
    unsafe {
        let mut info = BITMAP::default();
        if GetObjectW(b.into(), size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut _)) == 0 || info.bmWidth <= 0 || info.bmHeight <= 0 {
            return;
        }
        let mem = CreateCompatibleDC(Some(dc));
        let antigo = SelectObject(mem, b.into());
        if info.bmBitsPixel == 32 {
            let mistura = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            _ = GdiAlphaBlend(dc, x, y, lado, lado, mem, 0, 0, info.bmWidth, info.bmHeight.abs(), mistura);
        } else {
            _ = SetStretchBltMode(dc, HALFTONE);
            _ = StretchBlt(dc, x, y, lado, lado, Some(mem), 0, 0, info.bmWidth, info.bmHeight.abs(), SRCCOPY);
        }
        SelectObject(mem, antigo);
        _ = DeleteDC(mem);
    }
}

// ---------- menus suspensos ----------

struct Item {
    pos: u32,
    id: u32,
    tipo: u32,
    texto: Vec<u16>,
    bitmap: isize,
    submenu: isize,
}

struct MenuConvertido {
    menu: isize,
    itens: Vec<Item>,
}

static MENUS: Mutex<Vec<MenuConvertido>> = Mutex::new(Vec::new());

fn menus() -> std::sync::MutexGuard<'static, Vec<MenuConvertido>> {
    MENUS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Converte os itens do menu em desenho próprio. Pode ser chamado mais de uma vez para o mesmo
/// menu (WM_INITMENUPOPUP e MN_SIZEWINDOW): o que já foi convertido fica, e só itens que o
/// programa acrescentou depois são convertidos.
unsafe fn converter(menu: HMENU, dono: HWND) {
    if menu.is_invalid() || dono.is_invalid() {
        return;
    }
    let n = GetMenuItemCount(Some(menu));
    if n <= 0 {
        return;
    }
    let ja = {
        let mut v = menus();
        v.iter().position(|m| m.menu == menu.0 as isize).map(|i| v.remove(i).itens)
    };
    let novo = ja.is_none();
    let mut itens = ja.unwrap_or_default();
    let antes = itens.len();
    for pos in 0..n as u32 {
        let mut texto = [0u16; 512];
        let mut mii = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_ID | MIIM_STRING | MIIM_BITMAP | MIIM_SUBMENU,
            dwTypeData: windows::core::PWSTR(texto.as_mut_ptr()),
            cch: texto.len() as u32 - 1,
            ..Default::default()
        };
        if GetMenuItemInfoW(menu, pos, true, &mut mii).is_err() {
            break;
        }
        let ownerdraw = mii.fType.0 & MFT_OWNERDRAW.0 != 0;
        if !novo && ownerdraw {
            continue; // já convertido por nós
        }
        // o programa já desenha (ou pede para desenhar) este menu: fica como está
        if ownerdraw || mii.fType.0 & MFT_BITMAP.0 != 0 || mii.hbmpItem == HBMMENU_CALLBACK {
            if novo {
                return;
            }
            continue;
        }
        let len = (mii.cch as usize).min(texto.len() - 1);
        itens.push(Item {
            pos,
            id: mii.wID,
            tipo: mii.fType.0,
            texto: texto[..len].to_vec(),
            bitmap: mii.hbmpItem.0 as isize,
            submenu: mii.hSubMenu.0 as isize,
        });
    }
    for it in &itens[antes..] {
        let mii = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE,
            fType: MENU_ITEM_TYPE(it.tipo | MFT_OWNERDRAW.0),
            ..Default::default()
        };
        _ = SetMenuItemInfoW(menu, it.pos, true, &mii);
    }
    if novo {
        // fundo do tema também nas margens do menu
        fundo_do_menu(menu, true);
    }
    menus().push(MenuConvertido { menu: menu.0 as isize, itens });
    prender();
    _ = SetWindowSubclass(dono, Some(proc_dono), 0xBA02, 0);
}

/// Pincel do `fundo` do tema no menu (margens do menu suspenso, faixa da barra de menus), ou
/// de volta ao padrão.
unsafe fn fundo_do_menu(menu: HMENU, tema: bool) {
    static PINCEIS: Mutex<Vec<(u32, isize)>> = Mutex::new(Vec::new());
    let pincel = if tema {
        let c = cor(FUNDO).0;
        let mut p = PINCEIS.lock().unwrap_or_else(|e| e.into_inner());
        match p.iter().find(|(k, _)| *k == c) {
            Some(&(_, b)) => b,
            None => {
                let b = CreateSolidBrush(COLORREF(c)).0 as isize;
                p.push((c, b));
                b
            }
        }
    } else {
        0
    };
    let info = MENUINFO { cbSize: size_of::<MENUINFO>() as u32, fMask: MIM_BACKGROUND, hbrBack: HBRUSH(pincel as *mut _), ..Default::default() };
    _ = SetMenuInfo(menu, &info);
}

unsafe fn devolver(menu: HMENU) {
    let Some(m) = ({
        let mut v = menus();
        v.iter().position(|m| m.menu == menu.0 as isize).map(|i| v.remove(i))
    }) else {
        return;
    };
    if !IsMenu(menu).as_bool() {
        return;
    }
    fundo_do_menu(menu, false);
    // pela identidade do item, não pela posição: o programa pode ter mexido no menu depois
    // da conversão (itens que ele acrescentou não são desenho próprio e ficam como estão)
    let n = GetMenuItemCount(Some(menu)).max(0) as u32;
    let mut separadores = m.itens.iter().filter(|it| it.tipo & MFT_SEPARATOR.0 != 0);
    for pos in 0..n {
        let mut mii = MENUITEMINFOW { cbSize: size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_FTYPE | MIIM_ID | MIIM_SUBMENU, ..Default::default() };
        if GetMenuItemInfoW(menu, pos, true, &mut mii).is_err() || mii.fType.0 & MFT_OWNERDRAW.0 == 0 {
            continue;
        }
        let it = if mii.fType.0 & MFT_SEPARATOR.0 != 0 {
            separadores.next()
        } else {
            m.itens.iter().find(|it| it.tipo & MFT_SEPARATOR.0 == 0 && it.id == mii.wID && it.submenu == mii.hSubMenu.0 as isize)
        };
        let Some(it) = it else { continue };
        let mut texto = it.texto.clone();
        texto.push(0);
        let mii = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | if it.tipo & MFT_SEPARATOR.0 == 0 { MIIM_STRING } else { MENU_ITEM_MASK(0) },
            fType: MENU_ITEM_TYPE(it.tipo),
            dwTypeData: windows::core::PWSTR(texto.as_mut_ptr()),
            ..Default::default()
        };
        _ = SetMenuItemInfoW(menu, pos, true, &mii);
    }
}

/// Divide "Texto\tAtalho".
fn partes(t: &[u16]) -> (&[u16], &[u16]) {
    match t.iter().position(|&c| c == 9) {
        Some(i) => (&t[..i], &t[i + 1..]),
        None => (t, &[]),
    }
}

/// Item convertido: pelo menu e id (desenho) ou só pelo id (medida, que não traz o menu).
fn achar<R>(menu: Option<isize>, id: u32, f: impl FnOnce(&Item) -> R) -> Option<R> {
    let v = menus();
    for m in v.iter().rev() {
        if menu.is_some_and(|x| x != m.menu) {
            continue;
        }
        if let Some(it) = m.itens.iter().find(|it| it.id == id || (it.submenu != 0 && it.submenu as u32 == id)) {
            return Some(f(it));
        }
    }
    None
}

unsafe extern "system" fn proc_dono(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    if wp.0 == 0 && lp.0 != 0 && ligado() {
        if msg == WM_MEASUREITEM && seguro(false, || medir(h, &mut *(lp.0 as *mut MEASUREITEMSTRUCT))) {
            return LRESULT(1);
        }
        if msg == WM_DRAWITEM && seguro(false, || desenhar(&*(lp.0 as *const DRAWITEMSTRUCT))) {
            return LRESULT(1);
        }
    }
    DefSubclassProc(h, msg, wp, lp)
}

unsafe fn medir(h: HWND, mi: &mut MEASUREITEMSTRUCT) -> bool {
    if mi.CtlType != ODT_MENU {
        return false;
    }
    let dpi = dpi_de(h);
    let Some((sep, texto)) = achar(None, mi.itemID, |it| (it.tipo & MFT_SEPARATOR.0 != 0, it.texto.clone())) else {
        return false;
    };
    if sep {
        mi.itemWidth = 1;
        mi.itemHeight = px(9, dpi) as u32;
        return true;
    }
    let dc = GetDC(Some(h));
    let antiga = SelectObject(dc, fonte(dpi, true).into());
    let (t, atalho) = partes(&texto);
    let mut l = px(28, dpi) + largura_visivel(dc, t) + px(24, dpi);
    if !atalho.is_empty() {
        l += px(32, dpi) + largura(dc, atalho);
    }
    SelectObject(dc, antiga);
    ReleaseDC(Some(h), dc);
    mi.itemWidth = l as u32;
    mi.itemHeight = px(26, dpi) as u32;
    true
}

unsafe fn desenhar(di: &DRAWITEMSTRUCT) -> bool {
    if di.CtlType != ODT_MENU {
        return false;
    }
    let menu = di.hwndItem.0 as isize;
    let Some((tipo, texto, bitmap, submenu)) = achar(Some(menu), di.itemID, |it| (it.tipo, it.texto.clone(), it.bitmap, it.submenu != 0)) else {
        return false;
    };
    let dc = di.hDC;
    let r = di.rcItem;
    let janela = WindowFromDC(dc);
    let dpi = dpi_de(janela);
    let e = di.itemState.0;
    let selecionado = e & ODS_SELECTED.0 != 0;
    let habilitado = e & (ODS_DISABLED.0 | ODS_GRAYED.0) == 0;
    let marcado = e & ODS_CHECKED.0 != 0;
    preencher(dc, r, cor(if selecionado && habilitado { SELECAO } else { FUNDO }));
    if tipo & MFT_SEPARATOR.0 != 0 {
        let y = (r.top + r.bottom) / 2;
        preencher(dc, RECT { left: r.left + px(10, dpi), top: y, right: r.right - px(10, dpi), bottom: y + 1 }, cor(MUDO));
        ExcludeClipRect(dc, r.left, r.top, r.right, r.bottom);
        return true;
    }
    let tinta = if !habilitado {
        cor(MUDO)
    } else if selecionado {
        cor(TEXTO_FORTE)
    } else {
        cor(TEXTO)
    };
    let apagado = if selecionado { cor(TEXTO) } else { cor(APAGADO) };
    let antiga = SelectObject(dc, fonte(dpi, marcado).into());
    let coluna = RECT { left: r.left, top: r.top, right: r.left + px(28, dpi), bottom: r.bottom };
    if marcado {
        let g = if tipo & MFT_RADIOCHECK.0 != 0 { G_RADIO } else { G_MARCA };
        escrever(dc, &glifo(g), coluna, cor(DESTAQUE), DT_CENTER | DT_NOPREFIX);
    } else if let Some(g) = glifo_de_bitmap(HBITMAP(bitmap as *mut _)) {
        escrever(dc, &glifo(g), coluna, tinta, DT_CENTER | DT_NOPREFIX);
    } else if bitmap > 11 {
        let lado = px(16, dpi);
        desenhar_bitmap(dc, HBITMAP(bitmap as *mut _), coluna.left + (px(28, dpi) - lado) / 2, r.top + (r.bottom - r.top - lado) / 2, lado);
    }
    let fim = r.right - px(24, dpi);
    let (t, atalho) = partes(&texto);
    let acesso = if e & ODS_NOACCEL.0 != 0 { DT_HIDEPREFIX } else { DRAW_TEXT_FORMAT(0) };
    escrever(dc, t, RECT { left: r.left + px(28, dpi), top: r.top, right: fim, bottom: r.bottom }, tinta, DT_LEFT | DT_END_ELLIPSIS | acesso);
    if !atalho.is_empty() {
        SelectObject(dc, fonte(dpi, false).into());
        escrever(dc, atalho, RECT { left: r.left + px(28, dpi), top: r.top, right: fim, bottom: r.bottom }, apagado, DT_RIGHT | DT_NOPREFIX);
    }
    if submenu {
        SelectObject(dc, fonte(dpi, false).into());
        escrever(dc, &glifo(G_SETA), RECT { left: fim, top: r.top, right: r.right - px(4, dpi), bottom: r.bottom }, apagado, DT_CENTER | DT_NOPREFIX);
    }
    SelectObject(dc, antiga);
    // a seta de submenu do Windows vem depois deste desenho: fica fora do recorte
    ExcludeClipRect(dc, r.left, r.top, r.right, r.bottom);
    true
}

// ---------- janela do menu ----------

/// `dono`: a janela que recebe as mensagens do menu (a do TrackPopupMenu).
unsafe fn vestir_janela_de_menu(h: HWND, dono: HWND) {
    prender();
    // sem sombra (a classe é deste processo)
    let estilo = GetClassLongPtrW(h, GCL_STYLE);
    if estilo & CS_DROPSHADOW.0 as usize != 0 {
        SetClassLongPtrW(h, GCL_STYLE, (estilo & !(CS_DROPSHADOW.0 as usize)) as isize);
    }
    _ = SetWindowSubclass(h, Some(proc_janela_de_menu), 0xBA03, dono.0 as usize);
}

/// Janela que recebe as mensagens do menu (WM_MEASUREITEM, WM_DRAWITEM). O Windows não
/// informa; quem abre menu chama SetForegroundWindow na própria janela antes, então ela é a
/// janela em primeiro plano (ou a ativa) desta linha de execução.
unsafe fn dono_do_menu(menu: HWND, dica: HWND) -> HWND {
    if !dica.is_invalid() {
        return dica;
    }
    let nossa = |j: HWND| !j.is_invalid() && GetWindowThreadProcessId(j, None) == GetWindowThreadProcessId(menu, None);
    if let Ok(d) = GetWindow(menu, GW_OWNER) {
        if nossa(d) {
            return d;
        }
    }
    let fg = GetForegroundWindow();
    if nossa(fg) {
        return fg;
    }
    GetActiveWindow()
}

const MN_GETHMENU: u32 = 0x01E1;
const MN_SIZEWINDOW: u32 = 0x01E2;

/// Moldura do menu: `fundo` com 2 px de `destaque` por fora.
unsafe fn pintar_moldura(h: HWND, dc: HDC) {
    let mut j = RECT::default();
    _ = GetWindowRect(h, &mut j);
    let mut c = RECT::default();
    _ = GetClientRect(h, &mut c);
    let mut origem = POINT::default();
    _ = ClientToScreen(h, &mut origem);
    let (cx, cy) = (origem.x - j.left, origem.y - j.top);
    let salvo = SaveDC(dc);
    ExcludeClipRect(dc, cx, cy, cx + c.right, cy + c.bottom);
    let todo = RECT { left: 0, top: 0, right: j.right - j.left, bottom: j.bottom - j.top };
    preencher(dc, todo, cor(DESTAQUE));
    preencher(dc, RECT { left: 2, top: 2, right: todo.right - 2, bottom: todo.bottom - 2 }, cor(FUNDO));
    _ = RestoreDC(dc, salvo);
}

unsafe extern "system" fn proc_janela_de_menu(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, dono: usize) -> LRESULT {
    if !ligado() {
        return DefSubclassProc(h, msg, wp, lp);
    }
    match msg {
        // menu aberto com TPM_NONOTIFY não manda WM_INITMENUPOPUP: converte quando o Windows
        // pede para a janela do menu calcular o tamanho, antes de medir os itens
        MN_SIZEWINDOW => {
            seguro((), || {
                let menu = HMENU(SendMessageW(h, MN_GETHMENU, None, None).0 as *mut _);
                converter(menu, dono_do_menu(h, HWND(dono as *mut _)));
            });
            DefSubclassProc(h, msg, wp, lp)
        }
        WM_DESTROY => {
            seguro((), || devolver(HMENU(SendMessageW(h, MN_GETHMENU, None, None).0 as *mut _)));
            DefSubclassProc(h, msg, wp, lp)
        }
        WM_NCPAINT => {
            seguro((), || {
                let dc = GetWindowDC(Some(h));
                pintar_moldura(h, dc);
                ReleaseDC(Some(h), dc);
            });
            LRESULT(0)
        }
        // menu com animação de abertura: o Windows tira uma "foto" da janela pelo WM_PRINT
        WM_PRINT => {
            let r = DefSubclassProc(h, msg, wp, lp);
            if lp.0 as u32 & PRF_NONCLIENT as u32 != 0 {
                seguro((), || pintar_moldura(h, HDC(wp.0 as *mut _)));
            }
            r
        }
        WM_SHOWWINDOW if wp.0 != 0 => {
            seguro((), || {
                let canto = DWMWCP_DONOTROUND.0 as u32;
                let borda = cor(DESTAQUE).0;
                _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &canto as *const _ as *const _, 4);
                _ = DwmSetWindowAttribute(h, DWMWA_BORDER_COLOR, &borda as *const _ as *const _, 4);
            });
            DefSubclassProc(h, msg, wp, lp)
        }
        _ => DefSubclassProc(h, msg, wp, lp),
    }
}

// ---------- barra de menus clássica ----------

const WM_UAHDRAWMENU: u32 = 0x91;
const WM_UAHDRAWMENUITEM: u32 = 0x92;

#[repr(C)]
struct UahMenu {
    menu: HMENU,
    dc: HDC,
    flags: u32,
}

#[repr(C)]
struct UahMenuItem {
    posicao: i32,
    metricas: [u32; 8],
    popup: [u32; 4],
    atualizar: u32,
}

#[repr(C)]
struct UahDrawMenuItem {
    dis: DRAWITEMSTRUCT,
    um: UahMenu,
    umi: UahMenuItem,
}


/// Item da barra de menus convertido em desenho próprio: assim o Windows pergunta a largura
/// (WM_MEASUREITEM) e cada nome cabe na CaskaydiaMono. Só ficam convertidos enquanto o tema
/// está ligado.
struct ItemBarra {
    pos: u32,
    id: u32,
    submenu: isize,
    tipo: u32,
    texto: Vec<u16>,
}

static BARRAS: Mutex<Vec<(isize, Vec<ItemBarra>)>> = Mutex::new(Vec::new());

fn barras() -> std::sync::MutexGuard<'static, Vec<(isize, Vec<ItemBarra>)>> {
    BARRAS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Converte os itens da barra de menus; devolve se ficou convertida. Barra com bitmap ou
/// desenho próprio do programa (botões de janela MDI, por exemplo) fica como está.
unsafe fn converter_barra(menu: HMENU) -> bool {
    if menu.is_invalid() {
        return false;
    }
    if barras().iter().any(|(m, _)| *m == menu.0 as isize) {
        return true;
    }
    let n = GetMenuItemCount(Some(menu));
    if n <= 0 {
        return false;
    }
    let mut itens = Vec::with_capacity(n as usize);
    for pos in 0..n as u32 {
        let mut texto = [0u16; 256];
        let mut mii = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_ID | MIIM_STRING | MIIM_BITMAP | MIIM_SUBMENU,
            dwTypeData: windows::core::PWSTR(texto.as_mut_ptr()),
            cch: texto.len() as u32 - 1,
            ..Default::default()
        };
        if GetMenuItemInfoW(menu, pos, true, &mut mii).is_err() || mii.fType.0 & (MFT_OWNERDRAW.0 | MFT_BITMAP.0) != 0 || !mii.hbmpItem.is_invalid() {
            return false;
        }
        let len = (mii.cch as usize).min(texto.len() - 1);
        itens.push(ItemBarra { pos, id: mii.wID, submenu: mii.hSubMenu.0 as isize, tipo: mii.fType.0, texto: texto[..len].to_vec() });
    }
    for it in &itens {
        let mii = MENUITEMINFOW { cbSize: size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_FTYPE, fType: MENU_ITEM_TYPE(it.tipo | MFT_OWNERDRAW.0), ..Default::default() };
        _ = SetMenuItemInfoW(menu, it.pos, true, &mii);
    }
    fundo_do_menu(menu, true);
    barras().push((menu.0 as isize, itens));
    true
}

/// Volta a barra de menus ao normal (tema desligado).
unsafe fn devolver_barra(menu: HMENU) {
    let Some((_, itens)) = ({
        let mut v = barras();
        v.iter().position(|(m, _)| *m == menu.0 as isize).map(|i| v.remove(i))
    }) else {
        return;
    };
    if !IsMenu(menu).as_bool() {
        return;
    }
    fundo_do_menu(menu, false);
    let n = GetMenuItemCount(Some(menu)).max(0) as u32;
    for pos in 0..n {
        let mut mii = MENUITEMINFOW { cbSize: size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_FTYPE | MIIM_ID | MIIM_SUBMENU, ..Default::default() };
        if GetMenuItemInfoW(menu, pos, true, &mut mii).is_err() || mii.fType.0 & MFT_OWNERDRAW.0 == 0 {
            continue;
        }
        let Some(it) = itens.iter().find(|it| it.id == mii.wID && it.submenu == mii.hSubMenu.0 as isize) else { continue };
        let mut texto = it.texto.clone();
        texto.push(0);
        let mii = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_STRING,
            fType: MENU_ITEM_TYPE(it.tipo),
            dwTypeData: windows::core::PWSTR(texto.as_mut_ptr()),
            ..Default::default()
        };
        _ = SetMenuItemInfoW(menu, pos, true, &mii);
    }
}

/// Texto do item da barra convertida: pelo id, pelo submenu (o WM_MEASUREITEM de um item que
/// abre submenu traz o submenu no lugar do id) ou pela posição.
fn texto_da_barra(menu: HMENU, id: Option<u32>, pos: Option<u32>) -> Option<Vec<u16>> {
    let v = barras();
    let (_, itens) = v.iter().find(|(m, _)| *m == menu.0 as isize)?;
    itens
        .iter()
        .find(|it| id.is_some_and(|i| it.id == i || (it.submenu != 0 && it.submenu as u32 == i)) || pos.is_some_and(|p| it.pos == p))
        .map(|it| it.texto.clone())
}

unsafe fn vestir_barra_de_menus(h: HWND) {
    if h.is_invalid() || GetWindowLongW(h, GWL_STYLE) as u32 & WS_CHILD.0 != 0 || GetMenu(h).is_invalid() {
        return;
    }
    // GetProp: só uma vez por janela
    if !GetPropW(h, w!("barra-menus")).0.is_null() {
        return;
    }
    _ = SetPropW(h, w!("barra-menus"), Some(HANDLE(1 as *mut _)));
    prender();
    _ = SetWindowSubclass(h, Some(proc_barra_de_menus), 0xBA04, 0);
    if converter_barra(GetMenu(h)) {
        _ = DrawMenuBar(h);
    }
}

/// Faixa da barra de menus em coordenadas da janela.
unsafe fn faixa(h: HWND) -> Option<RECT> {
    let mut mbi = MENUBARINFO { cbSize: size_of::<MENUBARINFO>() as u32, ..Default::default() };
    GetMenuBarInfo(h, OBJID_MENU, 0, &mut mbi).ok()?;
    let mut j = RECT::default();
    GetWindowRect(h, &mut j).ok()?;
    let mut r = mbi.rcBar;
    _ = OffsetRect(&mut r, -j.left, -j.top);
    Some(r)
}

/// A linha clara de 1 px que o Windows deixa entre a barra de menus e o conteúdo.
unsafe fn linha_de_baixo(h: HWND) {
    let Some(f) = faixa(h) else { return };
    let dc = GetWindowDC(Some(h));
    preencher(dc, RECT { left: f.left, top: f.bottom, right: f.right, bottom: f.bottom + 1 }, cor(FUNDO));
    ReleaseDC(Some(h), dc);
}

unsafe extern "system" fn proc_barra_de_menus(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    let menu = GetMenu(h);
    if !ligado() {
        // tema desligado: a barra convertida volta ao normal na primeira mensagem
        if barras().iter().any(|(m, _)| *m == menu.0 as isize) {
            seguro((), || {
                devolver_barra(menu);
                _ = DrawMenuBar(h);
            });
        }
        return DefSubclassProc(h, msg, wp, lp);
    }
    match msg {
        WM_UAHDRAWMENU if lp.0 != 0 => {
            let um = &*(lp.0 as *const UahMenu);
            if seguro(false, || match faixa(h) {
                Some(f) => {
                    preencher(um.dc, f, cor(FUNDO));
                    true
                }
                None => false,
            }) {
                return LRESULT(0);
            }
            DefSubclassProc(h, msg, wp, lp)
        }
        WM_UAHDRAWMENUITEM if lp.0 != 0 => {
            let d = &*(lp.0 as *const UahDrawMenuItem);
            let texto = texto_da_barra(d.um.menu, None, Some(d.umi.posicao as u32)).or_else(|| texto_do_item(d.um.menu, d.umi.posicao));
            if let Some(t) = texto {
                if seguro(false, || item_da_barra(h, d.um.dc, d.dis.rcItem, d.dis.itemState.0, &t)) {
                    return LRESULT(0);
                }
            }
            DefSubclassProc(h, msg, wp, lp)
        }
        WM_MEASUREITEM if wp.0 == 0 && lp.0 != 0 => {
            let mi = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
            if mi.CtlType == ODT_MENU {
                if let Some(t) = texto_da_barra(menu, Some(mi.itemID), None) {
                    seguro((), || {
                        let dpi = dpi_de(h);
                        let dc = GetDC(Some(h));
                        let antiga = SelectObject(dc, fonte(dpi, false).into());
                        mi.itemWidth = (largura_visivel(dc, &t) + px(4, dpi)) as u32;
                        SelectObject(dc, antiga);
                        ReleaseDC(Some(h), dc);
                        mi.itemHeight = px(20, dpi) as u32;
                    });
                    return LRESULT(1);
                }
            }
            DefSubclassProc(h, msg, wp, lp)
        }
        WM_DRAWITEM if wp.0 == 0 && lp.0 != 0 => {
            let di = &*(lp.0 as *const DRAWITEMSTRUCT);
            if di.CtlType == ODT_MENU && di.hwndItem.0 == menu.0 {
                if let Some(t) = texto_da_barra(menu, Some(di.itemID), None) {
                    if seguro(false, || item_da_barra(h, di.hDC, di.rcItem, di.itemState.0, &t)) {
                        return LRESULT(1);
                    }
                }
            }
            DefSubclassProc(h, msg, wp, lp)
        }
        WM_NCPAINT | WM_NCACTIVATE => {
            // menu novo na janela (SetMenu): converte também
            if !barras().iter().any(|(m, _)| *m == menu.0 as isize) && seguro(false, || converter_barra(menu)) {
                _ = DrawMenuBar(h);
            }
            let r = DefSubclassProc(h, msg, wp, lp);
            seguro((), || linha_de_baixo(h));
            r
        }
        _ => DefSubclassProc(h, msg, wp, lp),
    }
}

/// Texto do item da barra de menus pelo próprio menu; vazio nos botões de janela MDI.
unsafe fn texto_do_item(menu: HMENU, pos: i32) -> Option<Vec<u16>> {
    let mut texto = [0u16; 256];
    let mut mii = MENUITEMINFOW {
        cbSize: size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_STRING,
        dwTypeData: windows::core::PWSTR(texto.as_mut_ptr()),
        cch: texto.len() as u32 - 1,
        ..Default::default()
    };
    GetMenuItemInfoW(menu, pos as u32, true, &mut mii).ok()?;
    let t = texto[..(mii.cch as usize).min(texto.len() - 1)].to_vec();
    (!t.is_empty()).then_some(t)
}

/// Item da barra de menus: fundo `fundo`, `fundo_claro` sob o mouse, `selecao` aberto.
unsafe fn item_da_barra(h: HWND, dc: HDC, r: RECT, e: u32, t: &[u16]) -> bool {
    let fundo = if e & ODS_SELECTED.0 != 0 {
        SELECAO
    } else if e & ODS_HOTLIGHT.0 != 0 {
        FUNDO_CLARO
    } else {
        FUNDO
    };
    preencher(dc, r, cor(fundo));
    let inativo = e & (ODS_INACTIVE.0 | ODS_DISABLED.0 | ODS_GRAYED.0) != 0 || GetForegroundWindow() != h;
    let tinta = if inativo { cor(APAGADO) } else if e & (ODS_SELECTED.0 | ODS_HOTLIGHT.0) != 0 { cor(TEXTO_FORTE) } else { cor(TEXTO) };
    let acesso = if e & ODS_NOACCEL.0 != 0 { DT_HIDEPREFIX } else { DRAW_TEXT_FORMAT(0) };
    let anterior = SelectObject(dc, fonte(dpi_de(h), false).into());
    escrever(dc, t, r, tinta, DT_CENTER | acesso);
    SelectObject(dc, anterior);
    true
}

// ---------- botões da barra de título ----------

/// Propriedade na janela que ganhou os botões do tema: a janela dos botões.
const PROP_BOTOES: PCWSTR = w!("barra-botoes");
const CLASSE_BOTOES: PCWSTR = w!("barra-botoes");
const ID_BOTOES: usize = 0xBA05;
/// Largura de cada botão, em px a 96 dpi: a mesma célula das janelas na barra.
const CELULA: i32 = 28;
/// Glifos do menu de sistema no tema (`glifo_de_bitmap`), em 14 px como os glifos de sistema.
const G_MINIMIZAR: u16 = 0xf2d1;
const G_MAXIMIZAR: u16 = 0xf2d0;
const G_RESTAURAR: u16 = 0xf2d2;
const G_FECHAR: u16 = 0xf00d;
const TAMANHO_GLIFO: u32 = 14;

#[derive(Clone, Copy, PartialEq)]
enum Botao {
    Minimizar,
    Maximizar,
    Fechar,
}

/// Botões da janela, da esquerda para a direita: minimizar e maximizar pelos estilos, fechar
/// se a classe não tira (CS_NOCLOSE).
unsafe fn botoes_de(dono: HWND) -> Vec<Botao> {
    let estilo = GetWindowLongW(dono, GWL_STYLE) as u32;
    let mut v = Vec::with_capacity(3);
    if estilo & WS_MINIMIZEBOX.0 != 0 {
        v.push(Botao::Minimizar);
    }
    if estilo & WS_MAXIMIZEBOX.0 != 0 {
        v.push(Botao::Maximizar);
    }
    if GetClassLongPtrW(dono, GCL_STYLE) & CS_NOCLOSE.0 as usize == 0 {
        v.push(Botao::Fechar);
    }
    v
}

/// Janela com botão na barra (o mesmo critério da barra, que é onde as molduras são pintadas)
/// e com a barra de título desenhada pelo Windows: a área do programa começa abaixo dela.
/// Navegadores, Windows Terminal e Explorer estendem a área do programa até o alto e desenham
/// os próprios botões; ficam como estão.
unsafe fn titulo_do_windows(h: HWND) -> bool {
    if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() || GetWindowTextLengthW(h) == 0 {
        return false;
    }
    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
    if ex & WS_EX_APPWINDOW.0 == 0
        && (ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 || GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid()))
    {
        return false;
    }
    let mut j = RECT::default();
    let mut c = POINT::default();
    if GetWindowRect(h, &mut j).is_err() || !ClientToScreen(h, &mut c).as_bool() {
        return false;
    }
    c.y - j.top >= GetSystemMetricsForDpi(SM_CYCAPTION, dpi_de(h))
}

/// Estado da janela dos botões, guardado no GWLP_USERDATA: botão sob o mouse, botão apertado
/// e se a dona está ativa.
#[derive(Clone, Copy, Default)]
struct Estado {
    sob: Option<usize>,
    apertado: Option<usize>,
    ativa: bool,
}

fn ler_estado(h: HWND) -> Estado {
    let v = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } as usize;
    let botao = |b: usize| (b & 0xFF).checked_sub(1);
    Estado { sob: botao(v), apertado: botao(v >> 8), ativa: v & 0x1_0000 != 0 }
}

fn gravar_estado(h: HWND, e: Estado) {
    let botao = |b: Option<usize>| b.map_or(0, |i| i + 1) & 0xFF;
    let v = botao(e.sob) | botao(e.apertado) << 8 | (e.ativa as usize) << 16;
    unsafe { SetWindowLongPtrW(h, GWLP_USERDATA, v as isize) };
}

/// Liga ou desliga o WS_SYSMENU. Sem ele o DWM não desenha os botões nem o ícone; o título
/// fica, o duplo clique continua maximizando (WS_MAXIMIZEBOX fica) e Alt+F4 continua fechando.
unsafe fn menu_de_sistema(h: HWND, sim: bool) {
    let estilo = GetWindowLongPtrW(h, GWL_STYLE);
    let menu = WS_SYSMENU.0 as isize;
    let novo = if sim { estilo | menu } else { estilo & !menu };
    if novo != estilo {
        SetWindowLongPtrW(h, GWL_STYLE, novo);
        _ = SetWindowPos(h, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

fn modulo() -> HMODULE {
    let mut m = HMODULE::default();
    unsafe {
        _ = GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(modulo as *const () as *const u16),
            &mut m,
        );
    }
    m
}

/// Janela dos botões: popup da própria janela (fica logo acima dela na ordem das janelas, some
/// quando ela minimiza), sem ativar e fora da lista de janelas.
unsafe fn criar_botoes(dono: HWND) -> Option<HWND> {
    static REGISTRADA: AtomicBool = AtomicBool::new(false);
    let inst = HINSTANCE(modulo().0);
    if !REGISTRADA.swap(true, Ordering::Relaxed) {
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc_botoes),
            hInstance: inst,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASSE_BOTOES,
            ..Default::default()
        };
        RegisterClassW(&wc);
    }
    // no mesmo modo de DPI da dona, para as coordenadas baterem
    let antes = SetThreadDpiAwarenessContext(GetWindowDpiAwarenessContext(dono));
    let b = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, CLASSE_BOTOES, w!(""), WS_POPUP, 0, 0, 0, 0, Some(dono), None, Some(inst), None).ok();
    if !antes.0.is_null() {
        SetThreadDpiAwarenessContext(antes);
    }
    b
}

fn botoes(dono: HWND) -> Option<HWND> {
    let p = unsafe { GetPropW(dono, PROP_BOTOES) };
    (!p.0.is_null()).then(|| HWND(p.0))
}

/// Janela com barra de título do Windows ganha os botões do tema (uma vez por janela).
unsafe fn vestir_botoes(h: HWND) {
    if h.is_invalid() || botoes(h).is_some() {
        return;
    }
    let estilo = GetWindowLongW(h, GWL_STYLE) as u32;
    if estilo & WS_CHILD.0 != 0 || estilo & WS_SYSMENU.0 == 0 || estilo & WS_CAPTION.0 != WS_CAPTION.0 {
        return;
    }
    if !titulo_do_windows(h) || botoes_de(h).is_empty() {
        return;
    }
    prender();
    let Some(b) = criar_botoes(h) else { return };
    // a propriedade antes de mexer no estilo: as mensagens que isso manda voltam a este gancho
    _ = SetPropW(h, PROP_BOTOES, Some(HANDLE(b.0)));
    gravar_estado(b, Estado { ativa: GetAncestor(GetForegroundWindow(), GA_ROOTOWNER) == h, ..Default::default() });
    _ = SetWindowSubclass(h, Some(proc_dono_dos_botoes), ID_BOTOES, 0);
    menu_de_sistema(h, false);
    posicionar_botoes(h);
}

/// Tira os botões do tema; `devolver`: a janela continua aberta e recebe o WS_SYSMENU de volta.
unsafe fn despir_botoes(h: HWND, devolver: bool) {
    let b = botoes(h);
    _ = RemovePropW(h, PROP_BOTOES);
    _ = RemoveWindowSubclass(h, Some(proc_dono_dos_botoes), ID_BOTOES);
    if let Some(b) = b {
        _ = DestroyWindow(b);
    }
    if devolver {
        menu_de_sistema(h, true);
    }
}

/// Onde ficam os botões, em coordenadas de tela: no canto direito da barra de título, da borda
/// de cima (1 px) até a barra de menus ou a área do programa. Maximizada, a borda sobra para
/// fora do monitor nos quatro lados, e a barra de título começa depois dela.
unsafe fn lugar_dos_botoes(dono: HWND, n: i32) -> Option<RECT> {
    let mut j = RECT::default();
    GetWindowRect(dono, &mut j).ok()?;
    let mut c = POINT::default();
    if !ClientToScreen(dono, &mut c).as_bool() {
        return None;
    }
    let dpi = dpi_de(dono);
    let ex = GetWindowLongW(dono, GWL_EXSTYLE) as u32;
    let aba = if ex & WS_EX_CLIENTEDGE.0 != 0 { GetSystemMetricsForDpi(SM_CXEDGE, dpi) } else { 0 };
    let lado = c.x - j.left - aba;
    let mut base = c.y - aba;
    if !GetMenu(dono).is_invalid() {
        let mut mbi = MENUBARINFO { cbSize: size_of::<MENUBARINFO>() as u32, ..Default::default() };
        if GetMenuBarInfo(dono, OBJID_MENU, 0, &mut mbi).is_ok() {
            base = base.min(mbi.rcBar.top);
        }
    }
    let topo = if IsZoomed(dono).as_bool() { j.top + lado } else { j.top + 1 };
    let direita = j.right - lado;
    let esquerda = direita - n * px(CELULA, dpi);
    (base > topo && esquerda > j.left + lado).then_some(RECT { left: esquerda, top: topo, right: direita, bottom: base })
}

/// Lugar na ordem das janelas logo acima da dona. A janela dos botões nasce no alto de tudo, e
/// o Windows só garante que ela fique acima da dona. `true`: já está lá.
unsafe fn logo_acima(dono: HWND, b: HWND) -> (Option<HWND>, bool) {
    let acima = GetWindow(dono, GW_HWNDPREV).ok().filter(|a| !a.is_invalid());
    let topo = |j: HWND| GetWindowLongW(j, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
    match acima {
        Some(a) if a == b => (None, true),
        // SetWindowPos põe a janela logo abaixo de `a`, entre ela e a dona; acima da faixa
        // das janelas sempre no topo, o alto da faixa normal é o mesmo lugar
        Some(a) if !topo(a) || topo(dono) => (Some(a), false),
        _ => (Some(HWND_TOP), false),
    }
}

/// Acompanha a dona: posição, tamanho, ordem, maximizada ou não, visível ou não.
unsafe fn posicionar_botoes(dono: HWND) {
    let Some(b) = botoes(dono) else { return };
    let estilo = GetWindowLongW(dono, GWL_STYLE) as u32;
    let n = botoes_de(dono).len() as i32;
    let visivel = IsWindowVisible(dono).as_bool() && !IsIconic(dono).as_bool() && estilo & WS_CAPTION.0 == WS_CAPTION.0 && n > 0;
    match visivel.then(|| lugar_dos_botoes(dono, n)).flatten() {
        Some(r) => {
            let (ordem, fixa) = logo_acima(dono, b);
            let z = if fixa { SWP_NOZORDER } else { SET_WINDOW_POS_FLAGS(0) };
            _ = SetWindowPos(b, ordem, r.left, r.top, r.right - r.left, r.bottom - r.top, z | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        None => {
            _ = ShowWindow(b, SW_HIDE);
            gravar_estado(b, Estado { ativa: ler_estado(b).ativa, ..Default::default() });
        }
    }
    redesenhar(b);
}

unsafe extern "system" fn proc_dono_dos_botoes(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    if !ligado() {
        // tema desligado ou barra fechada: os botões do Windows voltam na primeira mensagem
        seguro((), || despir_botoes(h, true));
        return DefSubclassProc(h, msg, wp, lp);
    }
    match msg {
        WM_NCDESTROY => {
            seguro((), || despir_botoes(h, false));
            DefSubclassProc(h, msg, wp, lp)
        }
        WM_STYLECHANGED => {
            let r = DefSubclassProc(h, msg, wp, lp);
            seguro((), || {
                // o console põe o menu de sistema de volta em algumas trocas de modo
                if wp.0 as i32 == GWL_STYLE.0 && GetWindowLongW(h, GWL_STYLE) as u32 & WS_SYSMENU.0 != 0 {
                    menu_de_sistema(h, false);
                }
                posicionar_botoes(h);
            });
            r
        }
        WM_WINDOWPOSCHANGED => {
            let r = DefSubclassProc(h, msg, wp, lp);
            seguro((), || posicionar_botoes(h));
            r
        }
        WM_NCACTIVATE => {
            let r = DefSubclassProc(h, msg, wp, lp);
            seguro((), || {
                if let Some(b) = botoes(h) {
                    gravar_estado(b, Estado { ativa: wp.0 != 0, ..ler_estado(b) });
                    redesenhar(b);
                }
            });
            r
        }
        _ => DefSubclassProc(h, msg, wp, lp),
    }
}

/// Botão sob o ponto (coordenadas da janela dos botões).
unsafe fn botao_em(h: HWND, lp: LPARAM) -> Option<usize> {
    let (x, y) = ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
    let mut c = RECT::default();
    GetClientRect(h, &mut c).ok()?;
    let dono = GetWindow(h, GW_OWNER).ok()?;
    let i = x / px(CELULA, dpi_de(h));
    (x >= 0 && y >= 0 && x < c.right && y < c.bottom && (i as usize) < botoes_de(dono).len()).then_some(i as usize)
}

/// Clique no botão: o mesmo comando que os botões do Windows mandam.
unsafe fn acionar(h: HWND, i: usize) {
    let Ok(dono) = GetWindow(h, GW_OWNER) else { return };
    let Some(&b) = botoes_de(dono).get(i) else { return };
    let comando = match b {
        Botao::Minimizar => SC_MINIMIZE,
        Botao::Maximizar if IsZoomed(dono).as_bool() => SC_RESTORE,
        Botao::Maximizar => SC_MAXIMIZE,
        Botao::Fechar => SC_CLOSE,
    };
    _ = PostMessageW(Some(dono), WM_SYSCOMMAND, WPARAM(comando as usize), LPARAM(0));
}

/// Fundo `fundo`, como a barra de título pintada pelas molduras. Glifo em `texto` com a dona
/// ativa e apagado sem ela; sob o mouse, fundo `fundo_claro` (`selecao` apertado) e glifo
/// `texto_forte`, `vermelho` no fechar.
unsafe fn pintar_botoes(h: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(h, &mut ps);
    let mut c = RECT::default();
    _ = GetClientRect(h, &mut c);
    preencher(dc, c, cor(FUNDO));
    if let Ok(dono) = GetWindow(h, GW_OWNER) {
        let e = ler_estado(h);
        let dpi = dpi_de(h);
        let cel = px(CELULA, dpi);
        let zoom = IsZoomed(dono).as_bool();
        let anterior = SelectObject(dc, fonte_de(dpi, TAMANHO_GLIFO, false).into());
        for (i, b) in botoes_de(dono).into_iter().enumerate() {
            let r = RECT { left: i as i32 * cel, top: 0, right: (i as i32 + 1) * cel, bottom: c.bottom };
            let sob = e.sob == Some(i);
            if sob {
                preencher(dc, r, cor(if e.apertado == Some(i) { SELECAO } else { FUNDO_CLARO }));
            }
            let tinta = match (sob, b) {
                (true, Botao::Fechar) => cor(VERMELHO),
                (true, _) => cor(TEXTO_FORTE),
                _ if e.ativa => cor(TEXTO),
                _ => cor(APAGADO),
            };
            let g = match b {
                Botao::Minimizar => G_MINIMIZAR,
                Botao::Maximizar if zoom => G_RESTAURAR,
                Botao::Maximizar => G_MAXIMIZAR,
                Botao::Fechar => G_FECHAR,
            };
            escrever(dc, &glifo(g), r, tinta, DT_CENTER);
        }
        SelectObject(dc, anterior);
    }
    _ = EndPaint(h, &ps);
}

fn redesenhar(h: HWND) {
    unsafe { _ = InvalidateRect(Some(h), None, false) };
}

unsafe extern "system" fn proc_botoes(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        // a barra manda a mensagem vazia a todas as janelas quando troca o tema
        WM_NULL => {
            redesenhar(h);
            LRESULT(0)
        }
        WM_PAINT => {
            seguro((), || pintar_botoes(h));
            _ = ValidateRect(Some(h), None);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            seguro((), || {
                let e = ler_estado(h);
                let sob = botao_em(h, lp);
                if e.sob != sob {
                    gravar_estado(h, Estado { sob, ..e });
                    redesenhar(h);
                }
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                _ = TrackMouseEvent(&mut t);
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            gravar_estado(h, Estado { sob: None, ..ler_estado(h) });
            redesenhar(h);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            seguro((), || {
                let sob = botao_em(h, lp);
                gravar_estado(h, Estado { sob, apertado: sob, ..ler_estado(h) });
                SetCapture(h);
                redesenhar(h);
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            seguro((), || {
                let e = ler_estado(h);
                let sob = botao_em(h, lp);
                gravar_estado(h, Estado { sob, apertado: None, ..e });
                _ = ReleaseCapture();
                redesenhar(h);
                if let Some(i) = e.apertado.filter(|&i| sob == Some(i)) {
                    acionar(h, i);
                }
            });
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            gravar_estado(h, Estado { apertado: None, ..ler_estado(h) });
            redesenhar(h);
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}
