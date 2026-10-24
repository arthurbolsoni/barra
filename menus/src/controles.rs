//! Controles Win32 dos outros programas no tema, como os mods Translucent Windows e Win32 UI
//! Modernizer do Windhawk: as funções que os controles usam para escolher cor e se desenhar
//! são desviadas para a DLL dentro de cada programa.
//!
//! - cores do sistema (GetSysColor, GetSysColorBrush, FillRect com pincel de índice) pelos
//!   papéis do tema, só neste processo: nada de SetSysColors, que vale para o Windows inteiro;
//! - fundo e texto padrão das janelas e diálogos (WM_CTLCOLOR*, WM_ERASEBKGND): troca só o que
//!   saiu do padrão do Windows; programa que escolhe as próprias cores fica com elas;
//! - controles com tema (botão, caixa de seleção, opção, abas, cabeçalho, lista, árvore, barra
//!   de rolagem, progresso, edição, combo, barra de ferramentas, dica…): DrawThemeBackground
//!   pinta plano, de canto vivo, nas cores do tema, e DrawThemeText e GetThemeColor dão a cor
//!   do texto;
//! - faixa de baixo das caixas de mensagem (Rectangle): como a das caixas de tarefa.
//!
//! Os desvios ficam instalados até o programa fechar; com o tema desligado, só passam adiante.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicUsize, Ordering};
use std::sync::Once;

use retour::RawDetour;
use windows::core::{w, BOOL, HRESULT, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Controls::{EnableThemeDialogTexture, IsThemeDialogTextureEnabled, DTBGOPTS, DTTOPTS, DTTOPTS_FLAGS, ETDT_DISABLE, ETDT_ENABLE, ETDT_USETABTEXTURE, HTHEME};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::{cor, escrever, fonte_de, ligado, modulo, prender, seguro, APAGADO, DESTAQUE, ESCURO, FUNDO, FUNDO_CLARO, MODO, MUDO, SELECAO, TEXTO, TEXTO_FORTE, VERMELHO};

// ---------- desvios ----------

/// Endereço para chamar a função original (o trampolim do desvio); 0 enquanto não desviada.
type Original = AtomicUsize;

static GET_SYS_COLOR: Original = AtomicUsize::new(0);
static GET_SYS_COLOR_BRUSH: Original = AtomicUsize::new(0);
static FILL_RECT: Original = AtomicUsize::new(0);
static DEF_WINDOW_PROC_W: Original = AtomicUsize::new(0);
static DEF_WINDOW_PROC_A: Original = AtomicUsize::new(0);
static DRAW_THEME_BACKGROUND: Original = AtomicUsize::new(0);
static DRAW_THEME_BACKGROUND_EX: Original = AtomicUsize::new(0);
static DRAW_THEME_TEXT: Original = AtomicUsize::new(0);
static DRAW_THEME_TEXT_EX: Original = AtomicUsize::new(0);
static GET_THEME_COLOR: Original = AtomicUsize::new(0);
static GET_THEME_SYS_COLOR: Original = AtomicUsize::new(0);
static GET_THEME_SYS_COLOR_BRUSH: Original = AtomicUsize::new(0);
static RECTANGLE: Original = AtomicUsize::new(0);
static SET_BK_COLOR: Original = AtomicUsize::new(0);
static SET_DC_BRUSH_COLOR: Original = AtomicUsize::new(0);
static SET_DC_PEN_COLOR: Original = AtomicUsize::new(0);
static PAT_BLT: Original = AtomicUsize::new(0);
static CREATE_PEN: Original = AtomicUsize::new(0);
static BIT_BLT: Original = AtomicUsize::new(0);
static ALPHA_BLEND: Original = AtomicUsize::new(0);
static DWM_ATRIBUTO: Original = AtomicUsize::new(0);
static FAIXA_COMPOSICAO: Original = AtomicUsize::new(0);
static DUI_COR: Original = AtomicUsize::new(0);
static DUI_FUNDO: Original = AtomicUsize::new(0);
static DUI_TEXTO: Original = AtomicUsize::new(0);
static DUI_BORDA: Original = AtomicUsize::new(0);
static FAIXA_LOAD_UI: Original = AtomicUsize::new(0);
static FAIXA_DESTROY: Original = AtomicUsize::new(0);
static FAIXA_INVALIDAR: Original = AtomicUsize::new(0);
static FAIXA_DESCARREGAR: Original = AtomicUsize::new(0);

type FGetSysColor = unsafe extern "system" fn(i32) -> u32;
type FGetSysColorBrush = unsafe extern "system" fn(i32) -> HBRUSH;
type FFillRect = unsafe extern "system" fn(HDC, *const RECT, HBRUSH) -> i32;
type FDefProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
type FDrawThemeBackground = unsafe extern "system" fn(HTHEME, HDC, i32, i32, *const RECT, *const RECT) -> HRESULT;
type FDrawThemeBackgroundEx = unsafe extern "system" fn(HTHEME, HDC, i32, i32, *const RECT, *const DTBGOPTS) -> HRESULT;
type FDrawThemeText = unsafe extern "system" fn(HTHEME, HDC, i32, i32, PCWSTR, i32, u32, u32, *const RECT) -> HRESULT;
type FDrawThemeTextEx = unsafe extern "system" fn(HTHEME, HDC, i32, i32, PCWSTR, i32, u32, *mut RECT, *const DTTOPTS) -> HRESULT;
type FGetThemeColor = unsafe extern "system" fn(HTHEME, i32, i32, i32, *mut COLORREF) -> HRESULT;
type FGetThemeSysColor = unsafe extern "system" fn(HTHEME, i32) -> COLORREF;
type FGetThemeSysColorBrush = unsafe extern "system" fn(HTHEME, i32) -> HBRUSH;
type FGetThemeClass = unsafe extern "system" fn(HTHEME, PWSTR, i32) -> HRESULT;
type FRectangle = unsafe extern "system" fn(HDC, i32, i32, i32, i32) -> BOOL;

/// A função original guardada em `o`. Só serve dentro do próprio desvio (ou depois de
/// conferir que não é 0): o endereço é gravado antes de o desvio valer.
unsafe fn original<F: Copy>(o: &Original) -> F {
    let p = o.load(Ordering::Acquire);
    std::mem::transmute_copy(&p)
}

/// Desvia `nome` de `modulo` para `novo`. O trampolim vai para `o` antes de o desvio valer:
/// uma chamada que chegue no meio já encontra a original.
unsafe fn desviar(modulo: HMODULE, nome: PCSTR, novo: *const (), o: &Original) -> bool {
    let Some(alvo) = GetProcAddress(modulo, nome) else { return false };
    let Ok(d) = RawDetour::new(alvo as *const (), novo) else { return false };
    o.store(d.trampoline() as *const () as usize, Ordering::Release);
    if d.enable().is_err() {
        o.store(0, Ordering::Release);
        return false;
    }
    // o desvio fica até o programa fechar (a DLL está presa)
    std::mem::forget(d);
    true
}

static INSTALADOS: AtomicBool = AtomicBool::new(false);

/// A própria barra também carrega a DLL; lá nada é desviado.
pub(crate) fn excluido() -> bool {
    static EXCLUIDO: AtomicU32 = AtomicU32::new(2);
    match EXCLUIDO.load(Ordering::Relaxed) {
        2 => {
            let mut b = [0u16; 260];
            let n = unsafe { GetModuleFileNameW(None, &mut b) } as usize;
            let nome = String::from_utf16_lossy(&b[..n.min(b.len())]).to_lowercase();
            let sim = nome.rsplit('\\').next().is_some_and(|f| f == "barra.exe");
            EXCLUIDO.store(sim as u32, Ordering::Relaxed);
            sim
        }
        v => v == 1,
    }
}

fn instalar() {
    static UMA_VEZ: Once = Once::new();
    UMA_VEZ.call_once(|| unsafe {
        prender();
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else { return };
        let Ok(gdi32) = GetModuleHandleW(w!("gdi32.dll")) else { return };
        let Ok(ux) = LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) else { return };
        // pincéis do sistema antes do desvio, para reconhecer quem usa o padrão do Windows
        for (i, p) in PINCEIS_DO_SISTEMA.iter().enumerate() {
            p.store(GetSysColorBrush(SYS_COLOR_INDEX(i as i32)).0 as isize, Ordering::Relaxed);
        }
        desviar(user32, PCSTR(c"GetSysColor".as_ptr() as _), get_sys_color as *const (), &GET_SYS_COLOR);
        desviar(user32, PCSTR(c"GetSysColorBrush".as_ptr() as _), get_sys_color_brush as *const (), &GET_SYS_COLOR_BRUSH);
        desviar(user32, PCSTR(c"FillRect".as_ptr() as _), fill_rect as *const (), &FILL_RECT);
        desviar(user32, PCSTR(c"DefWindowProcW".as_ptr() as _), def_window_proc_w as *const (), &DEF_WINDOW_PROC_W);
        desviar(user32, PCSTR(c"DefWindowProcA".as_ptr() as _), def_window_proc_a as *const (), &DEF_WINDOW_PROC_A);
        desviar(gdi32, PCSTR(c"Rectangle".as_ptr() as _), rectangle as *const (), &RECTANGLE);
        desviar(gdi32, PCSTR(c"SetBkColor".as_ptr() as _), set_bk_color as *const (), &SET_BK_COLOR);
        desviar(gdi32, PCSTR(c"SetDCBrushColor".as_ptr() as _), set_dc_brush_color as *const (), &SET_DC_BRUSH_COLOR);
        desviar(gdi32, PCSTR(c"SetDCPenColor".as_ptr() as _), set_dc_pen_color as *const (), &SET_DC_PEN_COLOR);
        desviar(gdi32, PCSTR(c"PatBlt".as_ptr() as _), pat_blt as *const (), &PAT_BLT);
        desviar(gdi32, PCSTR(c"CreatePen".as_ptr() as _), create_pen as *const (), &CREATE_PEN);
        desviar(gdi32, PCSTR(c"BitBlt".as_ptr() as _), bit_blt as *const (), &BIT_BLT);
        desviar(gdi32, PCSTR(c"GdiAlphaBlend".as_ptr() as _), alpha_blend as *const (), &ALPHA_BLEND);
        if let Ok(dwm) = LoadLibraryExW(w!("dwmapi.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) {
            desviar(dwm, PCSTR(c"DwmSetWindowAttribute".as_ptr() as _), dwm_atributo as *const (), &DWM_ATRIBUTO);
        }
        // antes de qualquer faixa de opções nascer: ela pergunta pela composição ao montar
        if let Ok(dwm) = LoadLibraryExW(w!("dwmapi.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) {
            desviar(dwm, PCSTR(c"DwmIsCompositionEnabled".as_ptr() as _), faixa_composicao as *const (), &FAIXA_COMPOSICAO);
        }
        desviar(ux, PCSTR(c"DrawThemeBackground".as_ptr() as _), draw_theme_background as *const (), &DRAW_THEME_BACKGROUND);
        desviar(ux, PCSTR(c"DrawThemeBackgroundEx".as_ptr() as _), draw_theme_background_ex as *const (), &DRAW_THEME_BACKGROUND_EX);
        desviar(ux, PCSTR(c"DrawThemeTextEx".as_ptr() as _), draw_theme_text_ex as *const (), &DRAW_THEME_TEXT_EX);
        desviar(ux, PCSTR(c"DrawThemeText".as_ptr() as _), draw_theme_text as *const (), &DRAW_THEME_TEXT);
        desviar(ux, PCSTR(c"GetThemeColor".as_ptr() as _), get_theme_color as *const (), &GET_THEME_COLOR);
        desviar(ux, PCSTR(c"GetThemeSysColor".as_ptr() as _), get_theme_sys_color as *const (), &GET_THEME_SYS_COLOR);
        desviar(ux, PCSTR(c"GetThemeSysColorBrush".as_ptr() as _), get_theme_sys_color_brush as *const (), &GET_THEME_SYS_COLOR_BRUSH);
        INSTALADOS.store(true, Ordering::Release);
    });
    instalar_dui();
    instalar_faixa();
}

/// Desvios no DirectUI (DUI70.dll), se ele já está no processo. Chamado também quando ele
/// carrega depois da DLL (aviso de carga em `xaml`).
pub(crate) fn instalar_dui() {
    static UMA_VEZ: Once = Once::new();
    if excluido() {
        return;
    }
    let Ok(dui) = (unsafe { GetModuleHandleW(w!("dui70.dll")) }) else { return };
    UMA_VEZ.call_once(|| unsafe {
        prender();
        desviar(dui, PCSTR(c"?CreateColor@Value@DirectUI@@SAPEAV12@K@Z".as_ptr() as _), dui_cor as *const (), &DUI_COR);
        desviar(dui, PCSTR(c"?SetBackgroundColor@Element@DirectUI@@QEAAJK@Z".as_ptr() as _), dui_fundo as *const (), &DUI_FUNDO);
        desviar(dui, PCSTR(c"?SetForegroundColor@Element@DirectUI@@QEAAJK@Z".as_ptr() as _), dui_texto as *const (), &DUI_TEXTO);
        desviar(dui, PCSTR(c"?SetBorderColor@Element@DirectUI@@QEAAJK@Z".as_ptr() as _), dui_borda as *const (), &DUI_BORDA);
    });
}

// ---------- DirectUI ----------

/// Cor do DirectUI (0xAABBGGRR, os 24 bits de baixo como num COLORREF) no tema. O Gerenciador de
/// Tarefas monta as páginas no DirectUI com as cores do modo escuro no próprio código e nas
/// folhas de estilo, sem passar pelo tema do Windows: fundo #191919, texto branco e #CDCDCD,
/// brancos translúcidos na seleção e nas bordas, #243B86 translúcido no mapa de calor. O alfa
/// fica; as cores dos gráficos ficam como estão. Só no modo escuro: no claro o branco é fundo.
fn cor_dui(c: u32) -> u32 {
    if MODO.load(Ordering::Relaxed) != ESCURO {
        return c;
    }
    let papel = match c & 0x00FF_FFFF {
        0x19_19_19 | 0x1C_1C_1C | 0x20_20_20 if c >> 24 == 0xFF => FUNDO,
        0x2B_2B_2B | 0x2C_2C_2C if c >> 24 == 0xFF => FUNDO_CLARO,
        0xFF_FF_FF => TEXTO,
        0xCD_CD_CD => APAGADO,
        0x86_3B_24 => DESTAQUE,
        _ => return c,
    };
    c & 0xFF00_0000 | cor(papel).0
}

type FCorDui = unsafe extern "system" fn(u32) -> *mut std::ffi::c_void;
type FCorElemento = unsafe extern "system" fn(*mut std::ffi::c_void, u32) -> HRESULT;

/// `Value::CreateColor`: as cores das folhas de estilo e boa parte das do código.
unsafe extern "system" fn dui_cor(c: u32) -> *mut std::ffi::c_void {
    original::<FCorDui>(&DUI_COR)(cor_dui(c))
}

unsafe extern "system" fn dui_fundo(e: *mut std::ffi::c_void, c: u32) -> HRESULT {
    original::<FCorElemento>(&DUI_FUNDO)(e, cor_dui(c))
}

unsafe extern "system" fn dui_texto(e: *mut std::ffi::c_void, c: u32) -> HRESULT {
    original::<FCorElemento>(&DUI_TEXTO)(e, cor_dui(c))
}

unsafe extern "system" fn dui_borda(e: *mut std::ffi::c_void, c: u32) -> HRESULT {
    original::<FCorElemento>(&DUI_BORDA)(e, cor_dui(c))
}

// ---------- faixa de opções (UIRibbon) ----------

/// Faixa de opções do Explorador clássico (e de outros programas com o UIRibbon): as cores dela
/// não passam pelo tema do Windows nem pelas cores do sistema. O framework aceita cores globais
/// pelo IPropertyStore dele. Os métodos são desviados pela vtable de um framework criado numa
/// linha STA (o objeto é de apartamento: fora dele viria um proxy). O programa costuma chamar o
/// LoadUI antes de o desvio entrar; por isso as cores vão também na primeira invalidação de
/// comando de cada framework, que o Explorador faz a cada seleção e navegação.
pub(crate) fn instalar_faixa() {
    static UMA_VEZ: Once = Once::new();
    if excluido() || unsafe { GetModuleHandleW(w!("uiribbon.dll")) }.is_err() {
        return;
    }
    UMA_VEZ.call_once(|| {
        prender();
        std::thread::spawn(|| seguro((), || unsafe {
            use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
            const CLSID_UIRIBBONFRAMEWORK: windows::core::GUID = windows::core::GUID::from_u128(0x926749fa_2615_4987_8845_c33e65f2b957);
            let iniciou = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            if let Ok(f) = CoCreateInstance::<_, windows::core::IUnknown>(&CLSID_UIRIBBONFRAMEWORK, None, CLSCTX_INPROC_SERVER) {
                // IUIFramework: 3 Initialize, 4 Destroy, 5 LoadUI, 9 InvalidateUICommand,
                // 10 FlushPendingInvalidations
                let vt = *(windows::core::Interface::as_raw(&f) as *const *const usize);
                desviar_endereco(*vt.add(5), faixa_load_ui as *const (), &FAIXA_LOAD_UI);
                desviar_endereco(*vt.add(4), faixa_destroy as *const (), &FAIXA_DESTROY);
                desviar_endereco(*vt.add(9), faixa_invalidar as *const (), &FAIXA_INVALIDAR);
                desviar_endereco(*vt.add(10), faixa_descarregar as *const (), &FAIXA_DESCARREGAR);
            }
            if iniciou {
                CoUninitialize();
            }
        }));
    });
}

unsafe fn desviar_endereco(alvo: usize, novo: *const (), o: &Original) {
    let Ok(d) = RawDetour::new(alvo as *const (), novo) else { return };
    o.store(d.trampoline() as *const () as usize, Ordering::Release);
    if d.enable().is_err() {
        o.store(0, Ordering::Release);
        return;
    }
    std::mem::forget(d);
}

unsafe extern "system" fn faixa_load_ui(f: *mut std::ffi::c_void, inst: HINSTANCE, nome: PCWSTR) -> HRESULT {
    let hr = original::<unsafe extern "system" fn(*mut std::ffi::c_void, HINSTANCE, PCWSTR) -> HRESULT>(&FAIXA_LOAD_UI)(f, inst, nome);
    if hr.is_ok() {
        seguro((), || conferir_faixa(f));
    }
    hr
}

unsafe extern "system" fn faixa_destroy(f: *mut std::ffi::c_void) -> HRESULT {
    if let Ok(mut v) = FAIXAS.lock() {
        v.retain(|x| x.framework != f as usize);
    }
    original::<unsafe extern "system" fn(*mut std::ffi::c_void) -> HRESULT>(&FAIXA_DESTROY)(f)
}

unsafe extern "system" fn faixa_invalidar(f: *mut std::ffi::c_void, comando: u32, flags: i32, chave: *const PROPERTYKEY) -> HRESULT {
    let hr = original::<unsafe extern "system" fn(*mut std::ffi::c_void, u32, i32, *const PROPERTYKEY) -> HRESULT>(&FAIXA_INVALIDAR)(f, comando, flags, chave);
    seguro((), || conferir_faixa(f));
    hr
}

unsafe extern "system" fn faixa_descarregar(f: *mut std::ffi::c_void) -> HRESULT {
    let hr = original::<unsafe extern "system" fn(*mut std::ffi::c_void) -> HRESULT>(&FAIXA_DESCARREGAR)(f);
    seguro((), || conferir_faixa(f));
    hr
}

/// Framework com as cores do tema: a versão do tema aplicada e os valores de antes.
struct Faixa {
    framework: usize,
    versao: u32,
    antes: [[u64; 3]; 3],
}

static FAIXAS: std::sync::Mutex<Vec<Faixa>> = std::sync::Mutex::new(Vec::new());

thread_local! {
    /// Gravando as cores: o SetValue pode invalidar comandos e voltar aqui.
    static NA_FAIXA: Cell<bool> = const { Cell::new(false) };
}

/// Põe as cores do tema no framework (ou as devolve, com o tema desligado). O Explorador grava
/// as cores do modo escuro depois do LoadUI, por cima das do tema: a cada chamada, se o fundo
/// não é o do tema, grava de novo, e o que estava lá passa a ser o valor de antes.
unsafe fn conferir_faixa(f: *mut std::ffi::c_void) {
    if NA_FAIXA.replace(true) {
        return;
    }
    let versao = crate::VERSAO.load(Ordering::Relaxed);
    let Ok(mut v) = FAIXAS.lock() else {
        NA_FAIXA.set(false);
        return;
    };
    let i = v.iter().position(|x| x.framework == f as usize);
    match (i, ligado()) {
        (Some(i), true) if v[i].versao != versao || fundo_da_faixa(f) != Some(hsb(crate::CORES[FUNDO].load(Ordering::Relaxed)) as u64) => {
            let mudou = v[i].versao == versao;
            if let Some(antes) = cores_da_faixa(f, None) {
                // trocadas pelo programa: as dele são as de antes
                if mudou {
                    v[i].antes = antes;
                }
            }
            v[i].versao = versao;
        }
        (Some(i), false) => {
            cores_da_faixa(f, Some(&v[i].antes));
            v.remove(i);
        }
        (None, true) => {
            if let Some(antes) = cores_da_faixa(f, None) {
                v.push(Faixa { framework: f as usize, versao, antes });
            }
        }
        _ => {}
    }
    drop(v);
    NA_FAIXA.set(false);
}

/// UI_HSBCOLOR de uma cor 0xRRGGBB: matiz e saturação do HSL em 0–255; o brilho sai da
/// luminosidade pela reta dos valores que o próprio Explorador usa no modo escuro (fundo
/// #202020 com brilho 15, texto branco com 250).
fn hsb(c: u32) -> u32 {
    let (r, g, b) = (((c >> 16) & 0xFF) as f32 / 255.0, ((c >> 8) & 0xFF) as f32 / 255.0, (c & 0xFF) as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    let brilho = ((15.0 + (l - 32.0 / 255.0) * (235.0 / (1.0 - 32.0 / 255.0))) / 255.0).clamp(0.0, 1.0);
    let byte = |v: f32| (v * 255.0).round() as u32;
    byte(h) | byte(s) << 8 | byte(brilho) << 16
}

const IID_IPROPERTYSTORE: windows::core::GUID = windows::core::GUID::from_u128(0x886d8eeb_8cf2_4446_8d02_cdba1dbdcf99);

/// UI_PKEY_Global* (VT_UI4 = 19, VT_BOOL = 11).
fn chave_da_faixa(i: u32, tipo: u32) -> PROPERTYKEY {
    PROPERTYKEY { fmtid: windows::core::GUID::from_values(i, 0x7363, 0x696e, [0x84, 0x41, 0x79, 0x8a, 0xcf, 0x5a, 0xeb, 0xb7]), pid: tipo }
}

/// UI_PKEY_GlobalBackgroundColor atual do framework.
unsafe fn fundo_da_faixa(f: *mut std::ffi::c_void) -> Option<u64> {
    let vt = |o: *mut std::ffi::c_void, i: usize| *(*(o as *const *const usize)).add(i);
    let consultar: unsafe extern "system" fn(*mut std::ffi::c_void, *const windows::core::GUID, *mut *mut std::ffi::c_void) -> HRESULT = std::mem::transmute(vt(f, 0));
    let mut loja = std::ptr::null_mut();
    if consultar(f, &IID_IPROPERTYSTORE, &mut loja).is_err() || loja.is_null() {
        return None;
    }
    let ler: unsafe extern "system" fn(*mut std::ffi::c_void, *const PROPERTYKEY, *mut [u64; 3]) -> HRESULT = std::mem::transmute(vt(loja, 5));
    let mut v = [0u64; 3];
    let ok = ler(loja, &chave_da_faixa(2000, 19), &mut v).is_ok();
    let liberar: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = std::mem::transmute(vt(loja, 2));
    liberar(loja);
    ok.then_some(v[1] & 0xFFFF_FFFF)
}

/// Cores globais da faixa: fundo, destaque e texto do tema, ou `devolver` (os valores de antes).
/// Devolve os valores que estavam lá.
unsafe fn cores_da_faixa(f: *mut std::ffi::c_void, devolver: Option<&[[u64; 3]; 3]>) -> Option<[[u64; 3]; 3]> {
    let vt = |o: *mut std::ffi::c_void, i: usize| *(*(o as *const *const usize)).add(i);
    let consultar: unsafe extern "system" fn(*mut std::ffi::c_void, *const windows::core::GUID, *mut *mut std::ffi::c_void) -> HRESULT = std::mem::transmute(vt(f, 0));
    let mut loja = std::ptr::null_mut();
    if consultar(f, &IID_IPROPERTYSTORE, &mut loja).is_err() || loja.is_null() {
        return None;
    }
    // IPropertyStore: 5 GetValue, 6 SetValue, 7 Commit; chaves UI_PKEY_Global*Color (VT_UI4)
    let ler: unsafe extern "system" fn(*mut std::ffi::c_void, *const PROPERTYKEY, *mut [u64; 3]) -> HRESULT = std::mem::transmute(vt(loja, 5));
    let gravar: unsafe extern "system" fn(*mut std::ffi::c_void, *const PROPERTYKEY, *const [u64; 3]) -> HRESULT = std::mem::transmute(vt(loja, 6));
    let confirmar: unsafe extern "system" fn(*mut std::ffi::c_void) -> HRESULT = std::mem::transmute(vt(loja, 7));
    let rgb = |p: usize| crate::CORES[p].load(Ordering::Relaxed);
    let mut antes = [[0u64; 3]; 3];
    let valores = [(2000, 19u64, hsb(rgb(FUNDO)) as u64), (2001, 19, hsb(rgb(DESTAQUE)) as u64), (2002, 19, hsb(rgb(TEXTO)) as u64)];
    for (k, (i, tipo, v)) in valores.into_iter().enumerate() {
        let chave = |i: u32| chave_da_faixa(i, tipo as u32);
        _ = ler(loja, &chave(i), &mut antes[k]);
        let valor = devolver.map_or([tipo, v, 0], |d| d[k]);
        _ = gravar(loja, &chave(i), &valor);
    }
    _ = confirmar(loja);
    let liberar: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = std::mem::transmute(vt(loja, 2));
    liberar(loja);
    Some(antes)
}

// ---------- cores do sistema ----------

/// Papel do tema de cada cor do sistema (COLOR_*); `None`: fica a do Windows (área de
/// trabalho). As bordas 3D viram uma linha `mudo` de 1 px: sombra e luz externas em `mudo`,
/// as internas no fundo.
fn papel_da_cor(i: i32) -> Option<usize> {
    Some(match i {
        // barra de rolagem, títulos, menu, janela, bordas, MDI, botão, 3D interno, barra de menus
        0 | 2 | 3 | 4 | 5 | 10 | 11 | 12 | 15 | 21 | 22 | 27 | 28 | 30 => FUNDO,
        // moldura, sombra e luz do 3D, texto desabilitado
        6 | 16 | 17 | 20 => MUDO,
        // texto do menu, da janela, do título, do botão, da dica
        7 | 8 | 9 | 18 | 23 => TEXTO,
        13 | 29 => SELECAO,
        14 => TEXTO_FORTE,
        19 => APAGADO,
        24 => FUNDO_CLARO,
        26 => DESTAQUE,
        _ => return None,
    })
}

const N_CORES: usize = 31;

/// Pincéis do sistema (GetSysColorBrush original), para reconhecer o padrão do Windows.
static PINCEIS_DO_SISTEMA: [AtomicIsize; N_CORES] = [const { AtomicIsize::new(0) }; N_CORES];

/// Pincel de cada papel do tema e a cor com que foi feito.
static PINCEIS: [AtomicIsize; 9] = [const { AtomicIsize::new(0) }; 9];
static PINCEIS_COR: [AtomicU32; 9] = [const { AtomicU32::new(u32::MAX) }; 9];

/// Pincel do papel na cor atual do tema. Os de cores antigas não são apagados: algum
/// programa pode ainda estar com eles.
fn pincel(papel: usize) -> HBRUSH {
    let c = cor(papel).0;
    let atual = PINCEIS[papel].load(Ordering::Acquire);
    unsafe {
        if atual != 0 && PINCEIS_COR[papel].load(Ordering::Acquire) == c && GetObjectType(HGDIOBJ(atual as *mut _)) == OBJ_BRUSH.0 as u32 {
            return HBRUSH(atual as *mut _);
        }
        let novo = CreateSolidBrush(COLORREF(c));
        PINCEIS_COR[papel].store(c, Ordering::Release);
        PINCEIS[papel].store(novo.0 as isize, Ordering::Release);
        novo
    }
}

static CANETAS: [AtomicIsize; 9] = [const { AtomicIsize::new(0) }; 9];
static CANETAS_COR: [AtomicU32; 9] = [const { AtomicU32::new(u32::MAX) }; 9];

/// Caneta de 1 px do papel na cor atual do tema; as de cores antigas ficam, como os pincéis.
fn caneta(papel: usize) -> HPEN {
    let c = cor(papel).0;
    let atual = CANETAS[papel].load(Ordering::Acquire);
    unsafe {
        if atual != 0 && CANETAS_COR[papel].load(Ordering::Acquire) == c && GetObjectType(HGDIOBJ(atual as *mut _)) == OBJ_PEN.0 as u32 {
            return HPEN(atual as *mut _);
        }
        let nova = CreatePen(PS_SOLID, 1, COLORREF(c));
        CANETAS_COR[papel].store(c, Ordering::Release);
        CANETAS[papel].store(nova.0 as isize, Ordering::Release);
        nova
    }
}

fn eh_pincel_do_tema(b: HBRUSH) -> bool {
    b.0 as isize != 0 && PINCEIS.iter().any(|p| p.load(Ordering::Relaxed) == b.0 as isize)
}

/// Índice da cor do sistema de um pincel: os de índice da classe (COLOR_* + 1) e os do sistema.
fn indice_do_pincel(b: HBRUSH) -> Option<i32> {
    let v = b.0 as isize;
    if (1..=N_CORES as isize).contains(&v) {
        return Some(v as i32 - 1);
    }
    if v == 0 {
        return None;
    }
    PINCEIS_DO_SISTEMA.iter().position(|p| p.load(Ordering::Relaxed) == v).map(|i| i as i32)
}

/// GetSysColor sem o desvio.
fn cor_do_sistema(i: i32) -> u32 {
    unsafe {
        match GET_SYS_COLOR.load(Ordering::Acquire) {
            0 => GetSysColor(SYS_COLOR_INDEX(i)),
            _ => original::<FGetSysColor>(&GET_SYS_COLOR)(i),
        }
    }
}

unsafe extern "system" fn get_sys_color(i: i32) -> u32 {
    if ligado() {
        if let Some(p) = papel_da_cor(i) {
            return cor(p).0;
        }
    }
    original::<FGetSysColor>(&GET_SYS_COLOR)(i)
}

unsafe extern "system" fn get_sys_color_brush(i: i32) -> HBRUSH {
    if ligado() {
        if let Some(p) = papel_da_cor(i) {
            return pincel(p);
        }
    }
    original::<FGetSysColorBrush>(&GET_SYS_COLOR_BRUSH)(i)
}

unsafe extern "system" fn fill_rect(dc: HDC, r: *const RECT, b: HBRUSH) -> i32 {
    let o = original::<FFillRect>(&FILL_RECT);
    if ligado() {
        if let Some(p) = indice_do_pincel(b).and_then(papel_da_cor).or_else(|| seguro(None, || papel_do_pincel_cinza(b))) {
            return o(dc, r, pincel(p));
        }
    }
    o(dc, r, b)
}

/// Cinzas que os programas usam no modo escuro do Windows, com pincel e cor próprios (o
/// Explorador pinta o painel da esquerda, a barra de endereço e a barra de status assim): o
/// papel do tema de cada um. Só no modo escuro.
fn papel_do_cinza(c: u32) -> Option<usize> {
    if crate::MODO.load(Ordering::Relaxed) != crate::ESCURO {
        return None;
    }
    match c & 0x00FF_FFFF {
        0x13_13_13 | 0x19_19_19 | 0x1C_1C_1C | 0x20_20_20 => Some(FUNDO),
        0x2B_2B_2B | 0x2C_2C_2C => Some(FUNDO_CLARO),
        // linhas separadoras (painel da esquerda do Explorador)
        0x38_38_38 => Some(MUDO),
        _ => None,
    }
}

/// Pincel sólido de um desses cinzas.
unsafe fn papel_do_pincel_cinza(b: HBRUSH) -> Option<usize> {
    if (b.0 as isize) <= N_CORES as isize {
        return None;
    }
    let mut lb = LOGBRUSH::default();
    if GetObjectW(HGDIOBJ(b.0), size_of::<LOGBRUSH>() as i32, Some(&mut lb as *mut _ as *mut _)) == 0 || lb.lbStyle != BS_SOLID {
        return None;
    }
    papel_do_cinza(lb.lbColor.0)
}

/// Soma dos canais de um COLORREF.
fn luz(c: u32) -> u32 {
    (c & 0xFF) + ((c >> 8) & 0xFF) + ((c >> 16) & 0xFF)
}

type FSetBkColor = unsafe extern "system" fn(HDC, COLORREF) -> COLORREF;

/// Fundo de texto opaco (árvore, listas) num desses cinzas: a cor do tema.
unsafe extern "system" fn set_bk_color(dc: HDC, c: COLORREF) -> COLORREF {
    let c = if ligado() { papel_do_cinza(c.0).map_or(c, cor) } else { c };
    original::<FSetBkColor>(&SET_BK_COLOR)(dc, c)
}

/// Pincel e caneta do DC (DC_BRUSH, DC_PEN) num desses cinzas: o separador do painel da
/// esquerda do Explorador é pintado assim. A faixa de opções pinta a fileira das abas com o
/// pincel do DC em preto, fixo no modo escuro, e a divisória embaixo dela num tom mais escuro
/// que o fundo: o que o uiribbon.dll pede mais escuro que o fundo do tema vira o fundo (preto
/// é preto no resto).
unsafe extern "system" fn set_dc_brush_color(dc: HDC, c: COLORREF) -> COLORREF {
    let c = if ligado() {
        papel_do_cinza(c.0)
            .or_else(|| (crate::MODO.load(Ordering::Relaxed) == crate::ESCURO && luz(c.0) < luz(cor(FUNDO).0) && seguro(false, chamado_pela_faixa)).then_some(FUNDO))
            .map_or(c, cor)
    } else {
        c
    };
    original::<FSetBkColor>(&SET_DC_BRUSH_COLOR)(dc, c)
}

/// Caneta num desses cinzas (a linha do separador do painel da esquerda do Explorador).
unsafe extern "system" fn create_pen(estilo: PEN_STYLE, largura: i32, c: COLORREF) -> HPEN {
    let c = if ligado() { papel_do_cinza(c.0).map_or(c, cor) } else { c };
    original::<unsafe extern "system" fn(PEN_STYLE, i32, COLORREF) -> HPEN>(&CREATE_PEN)(estilo, largura, c)
}

type FPatBlt = unsafe extern "system" fn(HDC, i32, i32, i32, i32, ROP_CODE) -> BOOL;

/// PatBlt com o pincel do DC num desses cinzas (pincel sólido próprio selecionado no DC):
/// pinta com o pincel do tema no lugar.
unsafe extern "system" fn pat_blt(dc: HDC, x: i32, y: i32, w: i32, h: i32, rop: ROP_CODE) -> BOOL {
    let o = original::<FPatBlt>(&PAT_BLT);
    if ligado() && rop == PATCOPY {
        let atual = GetCurrentObject(dc, OBJ_BRUSH);
        if let Some(p) = seguro(None, || papel_do_pincel_cinza(HBRUSH(atual.0))) {
            let antes = SelectObject(dc, pincel(p).into());
            let r = o(dc, x, y, w, h, rop);
            SelectObject(dc, antes);
            return r;
        }
    }
    o(dc, x, y, w, h, rop)
}

unsafe extern "system" fn set_dc_pen_color(dc: HDC, c: COLORREF) -> COLORREF {
    let c = if ligado() { papel_do_cinza(c.0).map_or(c, cor) } else { c };
    original::<FSetBkColor>(&SET_DC_PEN_COLOR)(dc, c)
}

/// A faixa de opções pergunta se há composição para decidir se a fileira das abas fica no
/// vidro do DWM; sem Mica o vidro sai preto. Para ela a resposta é não, e a fileira vem opaca,
/// com as cores globais. O resto do programa recebe a resposta verdadeira.
unsafe extern "system" fn faixa_composicao(sim: *mut BOOL) -> HRESULT {
    let hr = original::<unsafe extern "system" fn(*mut BOOL) -> HRESULT>(&FAIXA_COMPOSICAO)(sim);
    if hr.is_ok() && !sim.is_null() && ligado() && seguro(false, || chamado_pela_faixa()) {
        *sim = false.into();
    }
    hr
}

/// O endereço de retorno está no uiribbon.dll.
fn chamado_pela_faixa() -> bool {
    let Ok(m) = (unsafe { GetModuleHandleW(w!("uiribbon.dll")) }) else { return false };
    let inicio = m.0 as usize;
    let fim = unsafe {
        let nt = inicio + *((inicio + 0x3C) as *const u32) as usize;
        inicio + *((nt + 0x50) as *const u32) as usize
    };
    let mut quadros = [std::ptr::null_mut(); 6];
    let n = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut quadros, None) } as usize;
    quadros[..n].iter().any(|&q| (inicio..fim).contains(&(q as usize)))
}

/// FillRect sem o desvio.
fn preencher_direto(dc: HDC, r: &RECT, b: HBRUSH) {
    unsafe {
        match FILL_RECT.load(Ordering::Acquire) {
            0 => _ = FillRect(dc, r, b),
            _ => _ = original::<FFillRect>(&FILL_RECT)(dc, r, b),
        }
    }
}

/// Faixa de baixo de uma caixa de mensagem: retângulo num diálogo, de um lado a outro até o pé
/// da janela, com pincel sólido na cor 3D do Windows. O user32 cria esse pincel com a cor lida
/// direto do sistema, sem passar pelo GetSysColor.
unsafe fn faixa_de_caixa(dc: HDC, e: i32, d: i32, b: i32) -> bool {
    let mut lb = LOGBRUSH::default();
    if GetObjectW(GetCurrentObject(dc, OBJ_BRUSH), size_of::<LOGBRUSH>() as i32, Some(&mut lb as *mut _ as *mut _)) == 0
        || lb.lbStyle != BS_SOLID
        || lb.lbColor.0 != cor_do_sistema(COLOR_3DFACE.0)
    {
        return false;
    }
    let janela = WindowFromDC(dc);
    let mut c = RECT::default();
    classe_da_janela(janela, "#32770") && GetClientRect(janela, &mut c).is_ok() && e <= c.left && d >= c.right && b >= c.bottom
}

/// A faixa fica como a das caixas de tarefa: `fundo_claro`, com uma linha `mudo` em cima.
unsafe extern "system" fn rectangle(dc: HDC, e: i32, c: i32, d: i32, b: i32) -> BOOL {
    let o = original::<FRectangle>(&RECTANGLE);
    if ligado() && seguro(false, || faixa_de_caixa(dc, e, d, b)) {
        let pincel_antes = SelectObject(dc, pincel(FUNDO_CLARO).into());
        let caneta_antes = SelectObject(dc, caneta(FUNDO_CLARO).into());
        let feito = o(dc, e, c, d, b);
        SelectObject(dc, pincel_antes);
        SelectObject(dc, caneta_antes);
        preencher_direto(dc, &RECT { left: e, top: c, right: d, bottom: c + 1 }, pincel(MUDO));
        return feito;
    }
    o(dc, e, c, d, b)
}

// ---------- fundo e texto padrão das janelas ----------

const WM_CTLCOLOR_PRIMEIRA: u32 = WM_CTLCOLORMSGBOX;
const WM_CTLCOLOR_ULTIMA: u32 = WM_CTLCOLORSTATIC;

/// Depois do WM_CTLCOLOR*: se o pincel é o padrão (do sistema, do tema, sólido numa cor de
/// fundo do tema ou a textura das abas de um diálogo com tema), texto e fundo do padrão do
/// Windows viram os do tema. O pincel sólido na cor do tema é o do programa que pediu a cor ao
/// Windows (GetThemeColor, GetSysColor) e recebeu a do tema, mas deixou o texto preto. Pincel
/// do próprio programa, em outra cor, fica com as cores que ele escolheu.
unsafe fn ajustar_cor_de_controle(janela: HWND, dc: HDC, r: LRESULT) -> LRESULT {
    let b = HBRUSH(r.0 as *mut _);
    let do_sistema = indice_do_pincel(b).and_then(papel_da_cor);
    let padrao = do_sistema.is_some() || eh_pincel_do_tema(b) || (b.0 as isize != 0 && {
        let mut lb = LOGBRUSH::default();
        GetObjectW(HGDIOBJ(b.0), size_of::<LOGBRUSH>() as i32, Some(&mut lb as *mut _ as *mut _)) != 0
            && match lb.lbStyle {
                BS_SOLID => [FUNDO, FUNDO_CLARO, SELECAO].iter().any(|&p| cor(p) == lb.lbColor),
                BS_PATTERN | BS_DIBPATTERN | BS_DIBPATTERNPT => IsThemeDialogTextureEnabled(janela).as_bool(),
                _ => false,
            }
    });
    if !padrao {
        return r;
    }
    let t = GetTextColor(dc).0;
    if t == cor_do_sistema(COLOR_WINDOWTEXT.0) || t == cor_do_sistema(COLOR_BTNTEXT.0) || t == 0 {
        SetTextColor(dc, cor(TEXTO));
    } else if t == cor_do_sistema(COLOR_GRAYTEXT.0) {
        SetTextColor(dc, cor(MUDO));
    }
    let f = GetBkColor(dc).0;
    if f == cor_do_sistema(COLOR_WINDOW.0) || f == cor_do_sistema(COLOR_BTNFACE.0) || f == 0xFFFFFF {
        SetBkColor(dc, cor(FUNDO));
    }
    match do_sistema {
        Some(p) => LRESULT(pincel(p).0 as isize),
        // textura das abas
        None if !eh_pincel_do_tema(b) && {
            let mut lb = LOGBRUSH::default();
            GetObjectW(HGDIOBJ(b.0), size_of::<LOGBRUSH>() as i32, Some(&mut lb as *mut _ as *mut _)) != 0 && lb.lbStyle != BS_SOLID
        } => LRESULT(pincel(FUNDO).0 as isize),
        None => r,
    }
}

/// Fundo da classe com cor do sistema (COLOR_* + 1 ou pincel do sistema): o do tema.
unsafe fn apagar_fundo(h: HWND, dc: HDC) -> bool {
    let b = HBRUSH(GetClassLongPtrW(h, GCLP_HBRBACKGROUND) as *mut _);
    let Some(p) = indice_do_pincel(b).and_then(papel_da_cor) else { return false };
    let mut r = RECT::default();
    if GetClientRect(h, &mut r).is_err() {
        return false;
    }
    preencher_direto(dc, &r, pincel(p));
    true
}

unsafe fn padrao(o: FDefProc, h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if ligado() {
        match msg {
            WM_ERASEBKGND if wp.0 != 0 => {
                if seguro(false, || apagar_fundo(h, HDC(wp.0 as *mut _))) {
                    return LRESULT(1);
                }
            }
            WM_CTLCOLOR_PRIMEIRA..=WM_CTLCOLOR_ULTIMA if wp.0 != 0 => {
                let r = o(h, msg, wp, lp);
                return seguro(r, || ajustar_cor_de_controle(h, HDC(wp.0 as *mut _), r));
            }
            _ => {}
        }
    }
    o(h, msg, wp, lp)
}

unsafe extern "system" fn def_window_proc_w(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    padrao(original(&DEF_WINDOW_PROC_W), h, msg, wp, lp)
}

unsafe extern "system" fn def_window_proc_a(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    padrao(original(&DEF_WINDOW_PROC_A), h, msg, wp, lp)
}

/// Diálogos (#32770): o padrão vem do DefDlgProc, que não passa pelo DefWindowProc exportado.
/// Uma subclasse acerta o resultado depois que o diálogo e o padrão responderam.
unsafe extern "system" fn proc_dialogo(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    if !ligado() {
        seguro((), || devolver_textura(h));
        return DefSubclassProc(h, msg, wp, lp);
    }
    let r = DefSubclassProc(h, msg, wp, lp);
    if (WM_CTLCOLOR_PRIMEIRA..=WM_CTLCOLOR_ULTIMA).contains(&msg) && wp.0 != 0 {
        return seguro(r, || ajustar_cor_de_controle(h, HDC(wp.0 as *mut _), r));
    }
    r
}

/// Diálogo com a textura das abas (páginas de propriedades): a uxtheme pinta a textura por
/// dentro, sem passar pelos desvios, atrás dos títulos das caixas de grupo e dos rótulos.
/// Sai enquanto o tema está ligado; a propriedade guarda esta cópia da DLL, para devolver. Uma
/// cópia de outra versão, que ficou no programa depois de a barra ser atualizada, não mexe no
/// que esta fez.
const PROP_TEXTURA: PCWSTR = w!("barra-textura");

unsafe fn tirar_textura(h: HWND) {
    if IsThemeDialogTextureEnabled(h).as_bool() {
        _ = SetPropW(h, PROP_TEXTURA, Some(HANDLE(modulo().0)));
        _ = EnableThemeDialogTexture(h, ETDT_DISABLE);
    }
}

unsafe fn devolver_textura(h: HWND) {
    if GetPropW(h, PROP_TEXTURA).0 == modulo().0 {
        _ = RemovePropW(h, PROP_TEXTURA);
        _ = EnableThemeDialogTexture(h, ETDT_ENABLE | ETDT_USETABTEXTURE);
        _ = InvalidateRect(Some(h), None, true);
    }
}

pub(crate) fn classe_da_janela(h: HWND, nome: &str) -> bool {
    let mut b = [0u16; 32];
    let n = unsafe { GetClassNameW(h, &mut b) } as usize;
    n == nome.len() && b[..n].iter().zip(nome.encode_utf16()).all(|(a, c)| *a == c)
}

unsafe fn vestir_dialogo(h: HWND) {
    if classe_da_janela(h, "#32770") {
        tirar_textura(h);
        _ = SetWindowSubclass(h, Some(proc_dialogo), 0xBA06, 0);
    }
}

/// Janela recém-criada (gancho CBT): os desvios entram antes de a primeira janela do programa
/// criar os controles (a árvore com caixas de seleção, por exemplo, guarda as imagens delas ao
/// nascer); diálogo ganha a subclasse; dica fica de canto vivo.
pub fn criada(h: HWND) {
    if !ligado() || excluido() {
        return;
    }
    instalar();
    if !INSTALADOS.load(Ordering::Acquire) {
        return;
    }
    unsafe {
        vestir_dialogo(h);
        if classe_da_janela(h, "tooltips_class32") {
            let canto = DWMWCP_DONOTROUND.0 as u32;
            _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &canto as *const _ as *const _, 4);
        }
    }
}

/// Janela criada, já com o pai: as barras de ferramentas do caminho do Explorador (pai
/// "Breadcrumb Parent") ganham a marca de pintura.
pub fn criada_com_pai(h: HWND) {
    if excluido() || !INSTALADOS.load(Ordering::Acquire) {
        return;
    }
    unsafe {
        if classe_da_janela(h, "ToolbarWindow32") && GetParent(h).is_ok_and(|p| classe_da_janela(p, "Breadcrumb Parent")) {
            _ = SetWindowSubclass(h, Some(proc_caminho), ID_CAMINHO, 0);
        }
        if classe_da_janela(h, "NetUIHWND") && classe_da_janela(GetAncestor(h, GA_ROOT), "CabinetWClass") {
            _ = SetWindowSubclass(h, Some(proc_faixa), ID_FAIXA, 0);
        }
    }
}

const ID_CAMINHO: usize = 0x6261_7263; // "barc"
const ID_FAIXA: usize = 0x6261_7266; // "barf"

/// Botão de ajuda (?) da faixa de opções do Explorador, no canto direito da fileira das abas:
/// a faixa não tem como escondê-lo. Coordenadas da janela da faixa; None: ela não tem a fileira.
unsafe fn ajuda_da_faixa(h: HWND) -> Option<RECT> {
    let mut c = RECT::default();
    GetClientRect(h, &mut c).ok()?;
    let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(h).max(96) as i32;
    let px = |v: i32| v * dpi / 96;
    (c.bottom >= px(55) && c.right > px(200)).then(|| RECT { left: c.right - px(26), top: px(31), right: c.right, bottom: px(54) })
}

/// A faixa desenhou direto na janela dela (fora do WM_PAINT, a fileira inteira das abas quando
/// o mouse passa, ou o ícone): cobre o botão de ajuda de novo.
unsafe fn cobrir_ajuda(dc: HDC) {
    let h = WindowFromDC(dc);
    if h.is_invalid() || !classe_da_janela(h, "NetUIHWND") || !classe_da_janela(GetAncestor(h, GA_ROOT), "CabinetWClass") {
        return;
    }
    if let Some(a) = ajuda_da_faixa(h) {
        preencher_direto(dc, &a, pincel(FUNDO));
    }
}

/// Menus com cantos retos: o menu de contexto do Explorador pede cantos arredondados depois de
/// a barra pedir retos, e o arredondamento corta a borda do tema nos cantos.
unsafe extern "system" fn dwm_atributo(h: HWND, atributo: u32, valor: *const std::ffi::c_void, tamanho: u32) -> HRESULT {
    let o = original::<unsafe extern "system" fn(HWND, u32, *const std::ffi::c_void, u32) -> HRESULT>(&DWM_ATRIBUTO);
    if atributo == DWMWA_WINDOW_CORNER_PREFERENCE.0 as u32 && ligado() && classe_da_janela(h, "#32768") {
        let reto = DWMWCP_DONOTROUND.0 as u32;
        return o(h, atributo, &reto as *const _ as *const _, 4);
    }
    o(h, atributo, valor, tamanho)
}

unsafe extern "system" fn bit_blt(d: HDC, x: i32, y: i32, w: i32, h: i32, o: HDC, x1: i32, y1: i32, rop: ROP_CODE) -> BOOL {
    let r = original::<unsafe extern "system" fn(HDC, i32, i32, i32, i32, HDC, i32, i32, ROP_CODE) -> BOOL>(&BIT_BLT)(d, x, y, w, h, o, x1, y1, rop);
    if ligado() && seguro(false, chamado_pela_faixa) {
        seguro((), || cobrir_ajuda(d));
    }
    r
}

unsafe extern "system" fn alpha_blend(d: HDC, x: i32, y: i32, w: i32, h: i32, o: HDC, x1: i32, y1: i32, w1: i32, h1: i32, f: BLENDFUNCTION) -> BOOL {
    let r = original::<unsafe extern "system" fn(HDC, i32, i32, i32, i32, HDC, i32, i32, i32, i32, BLENDFUNCTION) -> BOOL>(&ALPHA_BLEND)(d, x, y, w, h, o, x1, y1, w1, h1, f);
    if ligado() && seguro(false, chamado_pela_faixa) {
        seguro((), || cobrir_ajuda(d));
    }
    r
}

/// Janela da faixa de opções do Explorador: o botão de ajuda some (pintado com o fundo do tema
/// depois de cada pintura) e o mouse naquele canto não chega à faixa (sem clique, sem dica).
unsafe extern "system" fn proc_faixa(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    if !ligado() {
        return DefSubclassProc(h, msg, wp, lp);
    }
    match msg {
        WM_PAINT | WM_PRINTCLIENT => {
            let r = DefSubclassProc(h, msg, wp, lp);
            seguro((), || {
                if let Some(a) = ajuda_da_faixa(h) {
                    let dc = if msg == WM_PRINTCLIENT { HDC(wp.0 as *mut _) } else { GetDC(Some(h)) };
                    preencher_direto(dc, &a, pincel(FUNDO));
                    if msg != WM_PRINTCLIENT {
                        ReleaseDC(Some(h), dc);
                    }
                }
            });
            r
        }
        WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP => {
            let p = POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 };
            if seguro(None, || ajuda_da_faixa(h)).is_some_and(|a| PtInRect(&a, p).as_bool()) {
                return LRESULT(0);
            }
            DefSubclassProc(h, msg, wp, lp)
        }
        WM_NCDESTROY => {
            _ = RemoveWindowSubclass(h, Some(proc_faixa), ID_FAIXA);
            DefSubclassProc(h, msg, wp, lp)
        }
        _ => DefSubclassProc(h, msg, wp, lp),
    }
}

thread_local! {
    /// Uma barra de ferramentas do caminho (barra de endereço do Explorador) está pintando.
    static PINTANDO_CAMINHO: Cell<bool> = const { Cell::new(false) };
}

/// Marca a pintura das barras de ferramentas do caminho: a seta delas aponta para o lado. A
/// pintura vai para um DC de memória, sem a janela, por isso a marca fica na linha de execução.
unsafe extern "system" fn proc_caminho(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    match msg {
        WM_PAINT | WM_PRINTCLIENT => {
            let antes = PINTANDO_CAMINHO.replace(true);
            let r = DefSubclassProc(h, msg, wp, lp);
            PINTANDO_CAMINHO.set(antes);
            r
        }
        WM_NCDESTROY => {
            _ = RemoveWindowSubclass(h, Some(proc_caminho), ID_CAMINHO);
            DefSubclassProc(h, msg, wp, lp)
        }
        _ => DefSubclassProc(h, msg, wp, lp),
    }
}

// ---------- troca de tema ----------

thread_local! {
    /// Versão do tema já aplicada às janelas desta linha de execução.
    static VISTA: Cell<u32> = const { Cell::new(u32::MAX) };
    /// Mensagens sem nenhuma janela para refazer: linha que nunca terá janela desiste.
    static VAZIAS: Cell<u8> = const { Cell::new(0) };
}

/// Chamado pelo gancho de mensagens, em cada linha de execução com janela: instala os desvios
/// na primeira vez e, quando o tema muda (ou liga, ou desliga), faz as janelas desta linha
/// relerem cores e tema: WM_SYSCOLORCHANGE (os controles comuns guardam as cores do sistema),
/// WM_THEMECHANGED e redesenho.
pub fn sincronizar(versao: u32) {
    if excluido() {
        return;
    }
    if ligado() {
        instalar();
    }
    if !INSTALADOS.load(Ordering::Acquire) || VISTA.get() == versao || VAZIAS.get() > 32 {
        return;
    }
    let mut janelas: Vec<HWND> = Vec::new();
    unsafe extern "system" fn juntar(h: HWND, lp: LPARAM) -> BOOL {
        (*(lp.0 as *mut Vec<HWND>)).push(h);
        true.into()
    }
    unsafe { _ = EnumThreadWindows(GetCurrentThreadId(), Some(juntar), LPARAM(&mut janelas as *mut _ as isize)) };
    if janelas.is_empty() {
        VAZIAS.set(VAZIAS.get() + 1);
        return;
    }
    VISTA.set(versao);
    for h in janelas {
        unsafe { refazer(h) };
    }
}

unsafe fn refazer(h: HWND) {
    let mut filhas: Vec<HWND> = Vec::new();
    unsafe extern "system" fn juntar(h: HWND, lp: LPARAM) -> BOOL {
        (*(lp.0 as *mut Vec<HWND>)).push(h);
        true.into()
    }
    _ = EnumChildWindows(Some(h), Some(juntar), LPARAM(&mut filhas as *mut _ as isize));
    let eu = GetCurrentThreadId();
    for j in std::iter::once(h).chain(filhas) {
        if ligado() {
            vestir_dialogo(j);
            criada_com_pai(j);
        }
        // janela filha de outra linha de execução recebe pela fila, sem esperar
        if GetWindowThreadProcessId(j, None) == eu {
            SendMessageW(j, WM_SYSCOLORCHANGE, None, None);
            SendMessageW(j, WM_THEMECHANGED, None, None);
        } else {
            _ = PostMessageW(Some(j), WM_SYSCOLORCHANGE, WPARAM(0), LPARAM(0));
            _ = PostMessageW(Some(j), WM_THEMECHANGED, WPARAM(0), LPARAM(0));
        }
    }
    crate::refazer_moldura(h);
    _ = RedrawWindow(Some(h), None, None, RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN);
}

// ---------- controles com tema ----------

#[derive(Clone, Copy, PartialEq)]
enum Classe {
    Botao,
    Edicao,
    Combo,
    Abas,
    Cabecalho,
    Lista,
    Arvore,
    Rolagem,
    Progresso,
    Trilho,
    Giro,
    Ferramentas,
    Rebar,
    Status,
    Dica,
    ListBox,
    Tarefa,
    Link,
    Assistente,
    /// Barra de endereço e caixa de busca do Explorador.
    Endereco,
    /// Menus que o programa desenha pelo tema do Windows (o de contexto do Explorador).
    Menu,
}

/// Classe do tema pelo nome (GetThemeClass, ordinal 74 da uxtheme), sem o prefixo
/// "Explorer::" e sem diferença de maiúsculas.
fn classe(t: HTHEME) -> Option<Classe> {
    static GET: AtomicUsize = AtomicUsize::new(1);
    let mut f = GET.load(Ordering::Relaxed);
    if f == 1 {
        f = unsafe {
            GetModuleHandleW(w!("uxtheme.dll"))
                .ok()
                .and_then(|m| GetProcAddress(m, PCSTR(74 as *const u8)))
                .map_or(0, |p| p as usize)
        };
        GET.store(f, Ordering::Relaxed);
    }
    if f == 0 || t.0 == 0 {
        return None;
    }
    let get: FGetThemeClass = unsafe { std::mem::transmute(f) };
    let mut b = [0u16; 64];
    if unsafe { get(t, PWSTR(b.as_mut_ptr()), b.len() as i32) }.is_err() {
        return None;
    }
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    let mut nome = &b[..n];
    if let Some(i) = nome.windows(2).rposition(|p| p == [b':' as u16, b':' as u16]) {
        nome = &nome[i + 2..];
    }
    let igual = |s: &str| nome.len() == s.len() && nome.iter().zip(s.bytes()).all(|(&a, b)| a < 128 && (a as u8).eq_ignore_ascii_case(&b));
    Some(match () {
        _ if igual("Button") => Classe::Botao,
        _ if igual("Edit") => Classe::Edicao,
        _ if igual("ComboBox") => Classe::Combo,
        _ if igual("Tab") => Classe::Abas,
        _ if igual("Header") => Classe::Cabecalho,
        _ if igual("ListView") => Classe::Lista,
        _ if igual("TreeView") => Classe::Arvore,
        _ if igual("ScrollBar") => Classe::Rolagem,
        _ if igual("Progress") => Classe::Progresso,
        _ if igual("TrackBar") => Classe::Trilho,
        _ if igual("Spin") => Classe::Giro,
        _ if igual("Toolbar") => Classe::Ferramentas,
        _ if igual("AddressBand") || igual("SearchBox") => Classe::Endereco,
        _ if igual("Menu") => Classe::Menu,
        _ if igual("Rebar") => Classe::Rebar,
        _ if igual("Status") => Classe::Status,
        _ if igual("Tooltip") => Classe::Dica,
        _ if igual("Listbox") => Classe::ListBox,
        _ if igual("TaskDialog") || igual("TaskDialogStyle") => Classe::Tarefa,
        _ if igual("Link") => Classe::Link,
        _ if igual("AeroWizard") || igual("AeroWizardStyle") => Classe::Assistente,
        _ => return None,
    })
}

// glifos da CaskaydiaMono Nerd Font
const G_MARCA: u16 = 0xf00c;
const G_CIRCULO: u16 = 0xf10c;
const G_CIRCULO_MARCADO: u16 = 0xf192;
const G_ESQUERDA: u16 = 0xf104;
const G_DIREITA: u16 = 0xf105;
const G_CIMA: u16 = 0xf106;
const G_BAIXO: u16 = 0xf107;
const G_MAIS_A_DIREITA: u16 = 0xf101;

/// Onde pintar, respeitando o que o DrawThemeBackgroundEx pediu para omitir.
struct Tela {
    dc: HDC,
    r: RECT,
    sem_fundo: bool,
    sem_borda: bool,
}

impl Tela {
    fn largura(&self) -> i32 {
        self.r.right - self.r.left
    }

    fn altura(&self) -> i32 {
        self.r.bottom - self.r.top
    }

    fn preencher(&self, r: RECT, papel: usize) {
        if r.right > r.left && r.bottom > r.top {
            preencher_direto(self.dc, &r, pincel(papel));
        }
    }

    fn fundo(&self, papel: usize) {
        if !self.sem_fundo {
            self.preencher(self.r, papel);
        }
    }

    /// Borda de 1 px por dentro; `lados`: esquerda, cima, direita, baixo.
    fn borda_lados(&self, papel: usize, lados: [bool; 4]) {
        if self.sem_borda {
            return;
        }
        let r = self.r;
        let [e, c, d, b] = lados;
        if e {
            self.preencher(RECT { right: r.left + 1, ..r }, papel);
        }
        if c {
            self.preencher(RECT { bottom: r.top + 1, ..r }, papel);
        }
        if d {
            self.preencher(RECT { left: r.right - 1, ..r }, papel);
        }
        if b {
            self.preencher(RECT { top: r.bottom - 1, ..r }, papel);
        }
    }

    fn borda(&self, papel: usize) {
        self.borda_lados(papel, [true; 4]);
    }

    fn caixa(&self, fundo: usize, borda: usize) {
        self.fundo(fundo);
        self.borda(borda);
    }

    /// Linha de 1 px no meio do retângulo: horizontal ou vertical.
    fn linha(&self, papel: usize, horizontal: bool) {
        let r = self.r;
        if horizontal {
            let y = (r.top + r.bottom) / 2;
            self.preencher(RECT { top: y, bottom: y + 1, ..r }, papel);
        } else {
            let x = (r.left + r.right) / 2;
            self.preencher(RECT { left: x, right: x + 1, ..r }, papel);
        }
    }

    /// Glifo centralizado, com altura de `fracao` do lado menor.
    fn glifo(&self, cp: u16, papel: usize, fracao: f32) {
        let lado = self.largura().min(self.altura());
        let tamanho = ((lado as f32) * fracao).round().max(6.0) as u32;
        unsafe {
            let antiga = SelectObject(self.dc, fonte_de(96, tamanho, false).into());
            escrever(self.dc, &[cp], self.r, cor(papel), DT_CENTER | DT_NOPREFIX);
            SelectObject(self.dc, antiga);
        }
    }
}

/// Pinta a parte; `false`: fica com o desenho do Windows.
fn pintar(c: Classe, t: &Tela, parte: i32, estado: i32) -> bool {
    match c {
        Classe::Botao => botao(t, parte, estado),
        // Menus: as regras dos menus da barra (DESIGN.md, "Menus")
        Classe::Menu => match parte {
            // MENU_BARBACKGROUND, MENU_POPUPBACKGROUND, MENU_POPUPGUTTER, MENU_POPUPCHECKBACKGROUND
            7 | 9 | 12 | 13 => {
                t.fundo(FUNDO);
                true
            }
            // MENU_POPUPBORDERS: 2 px em destaque, como a moldura do menu
            10 => {
                t.fundo(FUNDO);
                if !t.sem_borda {
                    for i in 0..2 {
                        let r = RECT { left: t.r.left + i, top: t.r.top + i, right: t.r.right - i, bottom: t.r.bottom - i };
                        Tela { r, ..*t }.borda(DESTAQUE);
                    }
                }
                true
            }
            // MENU_BARITEM: sob o mouse e aberto
            8 => {
                match estado {
                    2 => t.fundo(FUNDO_CLARO),
                    3 => t.fundo(SELECAO),
                    _ => t.fundo(FUNDO),
                }
                true
            }
            // MENU_POPUPITEM e MENU_POPUPITEM_FOCUSABLE (o do menu de contexto do Explorador): fundo;
            // selecionado em selecao (HOT, DISABLEDHOT)
            14 | 27 => {
                t.fundo(if matches!(estado, 2 | 4) { SELECAO } else { FUNDO });
                true
            }
            // MENU_POPUPSEPARATOR: linha de 1 px em mudo, recuada 10 px
            15 => {
                let r = RECT { left: t.r.left + 10, right: t.r.right - 10, ..t.r };
                Tela { r, ..*t }.linha(MUDO, true);
                true
            }
            // MENU_POPUPCHECK: marca (ou bolinha) em destaque
            11 => {
                let g = if matches!(estado, 3 | 4) { G_CIRCULO_MARCADO } else { G_MARCA };
                t.glifo(g, if matches!(estado, 2 | 4) { MUDO } else { DESTAQUE }, 0.7);
                true
            }
            // MENU_POPUPSUBMENU: seta em texto-apagado
            16 => {
                t.glifo(G_DIREITA, if estado == 2 { MUDO } else { APAGADO }, 0.7);
                true
            }
            _ => false,
        },
        // ABP_ADDRESSBAND e SBP_SEARCHBOX: a caixa, como a de edição
        Classe::Endereco => match parte {
            1 => {
                t.caixa(FUNDO, match estado { 2 => APAGADO, 4 => DESTAQUE, _ => MUDO });
                true
            }
            _ => false,
        },
        Classe::Edicao => match parte {
            // EP_EDITTEXT, EP_BACKGROUNDWITHBORDER e as bordas EP_EDITBORDER_*
            1 | 5 | 6..=9 => {
                let borda = match (parte, estado) {
                    (6..=9, 2) | (1 | 5, 2) => APAGADO,
                    (6..=9, 3) | (1 | 5, 5) => DESTAQUE,
                    _ => MUDO,
                };
                t.caixa(FUNDO, borda);
                true
            }
            // EP_BACKGROUND
            3 => {
                t.fundo(FUNDO);
                true
            }
            _ => false,
        },
        Classe::Combo => match parte {
            // botão da seta (CP_DROPDOWNBUTTON, à direita, à esquerda)
            1 | 6 | 7 => {
                t.glifo(G_BAIXO, match estado { 2 | 3 => TEXTO, 4 => MUDO, _ => APAGADO }, 0.6);
                true
            }
            2 | 3 => {
                t.fundo(FUNDO);
                true
            }
            // CP_BORDER: combo editável
            4 => {
                t.caixa(FUNDO, match estado { 2 => APAGADO, 3 => DESTAQUE, _ => MUDO });
                true
            }
            // CP_READONLY: lista suspensa, parece botão
            5 => {
                let (f, b) = match estado { 2 => (SELECAO, MUDO), 3 => (SELECAO, DESTAQUE), 4 => (FUNDO, MUDO), _ => (FUNDO_CLARO, MUDO) };
                t.caixa(f, b);
                true
            }
            // CP_DROPDOWNITEM
            9 => {
                t.fundo(if estado == 2 { SELECAO } else { FUNDO });
                true
            }
            _ => false,
        },
        Classe::Abas => match parte {
            // TABP_PANE
            9 => {
                t.caixa(FUNDO, MUDO);
                true
            }
            // TABP_BODY, TABP_AEROWIZARDBODY
            10 | 11 => {
                t.fundo(FUNDO);
                true
            }
            1..=8 => {
                // selecionada: sem a linha de baixo, cobre a borda do painel; linha de 2 px em
                // `destaque` em cima
                let selecionada = matches!(estado, 3 | 5);
                t.fundo(if estado == 2 { FUNDO_CLARO } else { FUNDO });
                t.borda_lados(MUDO, [true, true, true, false]);
                if selecionada && !t.sem_borda {
                    t.preencher(RECT { bottom: t.r.top + 2, ..t.r }, DESTAQUE);
                }
                true
            }
            _ => false,
        },
        Classe::Cabecalho => match parte {
            0 => {
                t.fundo(FUNDO);
                t.borda_lados(MUDO, [false, false, false, true]);
                true
            }
            // HP_HEADERITEM (e as pontas): repouso, sob o mouse, apertado; com ou sem ordem
            1..=3 => {
                t.fundo([FUNDO, FUNDO_CLARO, SELECAO][((estado.max(1) - 1) % 3) as usize]);
                t.borda_lados(MUDO, [false, false, true, true]);
                true
            }
            // HP_HEADERSORTARROW
            4 => {
                t.glifo(if estado == 1 { G_CIMA } else { G_BAIXO }, APAGADO, 1.0);
                true
            }
            // HP_HEADERDROPDOWN, HP_HEADERDROPDOWNFILTER
            5 | 6 => {
                if estado == 3 {
                    t.fundo(FUNDO_CLARO);
                }
                t.glifo(G_BAIXO, APAGADO, 0.6);
                true
            }
            _ => false,
        },
        Classe::Lista => match parte {
            // LVP_LISTITEM
            1 => {
                match estado {
                    2 => t.fundo(FUNDO_CLARO),
                    3 | 5 | 6 => t.fundo(SELECAO),
                    _ => {}
                }
                true
            }
            // LVP_GROUPHEADER: sob o mouse nos estados pares
            6 => {
                if estado % 2 == 0 {
                    t.fundo(FUNDO_CLARO);
                }
                true
            }
            // LVP_GROUPHEADERLINE
            7 => {
                t.linha(MUDO, true);
                true
            }
            // LVP_EXPANDBUTTON, LVP_COLLAPSEBUTTON
            8 | 9 => {
                t.glifo(if parte == 8 { G_BAIXO } else { G_CIMA }, if estado == 1 { APAGADO } else { TEXTO }, 0.8);
                true
            }
            _ => false,
        },
        Classe::Arvore => match parte {
            // TVP_TREEITEM
            1 => {
                match estado {
                    2 => t.fundo(FUNDO_CLARO),
                    3 | 5 | 6 => t.fundo(SELECAO),
                    _ => {}
                }
                true
            }
            // TVP_GLYPH, TVP_HOTGLYPH
            2 | 4 => {
                t.glifo(if estado == 2 { G_BAIXO } else { G_DIREITA }, if parte == 4 { TEXTO } else { APAGADO }, 1.0);
                true
            }
            _ => false,
        },
        Classe::Rolagem => rolagem(t, parte, estado),
        Classe::Progresso => match parte {
            // PP_BAR, PP_BARVERT, PP_TRANSPARENTBAR*
            1 | 2 | 11 | 12 => {
                t.fundo(FUNDO_CLARO);
                true
            }
            // PP_CHUNK*, PP_FILL*: normal, erro, pausa, parcial
            3..=6 => {
                t.fundo(match (parte, estado) {
                    (5 | 6, 2) => VERMELHO,
                    (5 | 6, 3) => APAGADO,
                    _ => DESTAQUE,
                });
                true
            }
            // brilho que corre por cima: fica sem
            7..=10 => true,
            _ => false,
        },
        Classe::Trilho => match parte {
            1 | 2 | 9 | 10 => {
                t.fundo(MUDO);
                true
            }
            3..=8 => {
                t.fundo(match estado { 2 | 3 => TEXTO_FORTE, 5 => MUDO, _ => DESTAQUE });
                true
            }
            _ => false,
        },
        Classe::Giro => match parte {
            1..=4 => {
                let f = match estado { 2 => SELECAO, 3 => SELECAO, 4 => FUNDO, _ => FUNDO_CLARO };
                t.caixa(f, MUDO);
                t.glifo([G_CIMA, G_BAIXO, G_DIREITA, G_ESQUERDA][(parte - 1) as usize], if estado == 4 { MUDO } else { TEXTO }, 0.8);
                true
            }
            _ => false,
        },
        Classe::Ferramentas => match parte {
            // botões: transparentes em repouso
            1..=4 => {
                match estado {
                    2 | 7 | 8 => t.fundo(FUNDO_CLARO),
                    3 | 5 | 6 => t.fundo(SELECAO),
                    _ => {}
                }
                // seta do botão dividido: para baixo; no caminho da barra de endereço, para o
                // lado (para baixo com o menu aberto)
                if parte == 4 {
                    let lado = !matches!(estado, 3 | 5 | 6) && PINTANDO_CAMINHO.get();
                    t.glifo(if lado { G_DIREITA } else { G_BAIXO }, if estado == 4 { MUDO } else { APAGADO }, 0.7);
                }
                true
            }
            5 => {
                t.linha(MUDO, false);
                true
            }
            6 => {
                t.linha(MUDO, true);
                true
            }
            7 => {
                t.glifo(G_BAIXO, APAGADO, 0.8);
                true
            }
            _ => false,
        },
        Classe::Rebar => match parte {
            0 | 3 | 6 => {
                t.fundo(FUNDO);
                true
            }
            1 | 2 | 7 | 8 => true,
            4 | 5 => {
                if estado != 1 {
                    t.fundo(if estado == 3 { SELECAO } else { FUNDO_CLARO });
                }
                t.glifo(G_MAIS_A_DIREITA, APAGADO, 0.6);
                true
            }
            _ => false,
        },
        Classe::Status => match parte {
            0 => {
                t.fundo(FUNDO);
                t.borda_lados(MUDO, [false, true, false, false]);
                true
            }
            1..=3 => true,
            _ => false,
        },
        Classe::Dica => match parte {
            // TTP_STANDARD, TTP_STANDARDTITLE, TTP_BALLOON, TTP_BALLOONTITLE: como a dica da barra
            1..=4 => {
                t.fundo(FUNDO);
                if !t.sem_borda {
                    for i in 0..2 {
                        let r = RECT { left: t.r.left + i, top: t.r.top + i, right: t.r.right - i, bottom: t.r.bottom - i };
                        Tela { r, ..*t }.borda(DESTAQUE);
                    }
                }
                true
            }
            _ => false,
        },
        Classe::ListBox => match parte {
            // bordas: repouso, foco, sob o mouse, desabilitada
            1..=4 => {
                t.caixa(FUNDO, match estado { 2 => DESTAQUE, 3 => APAGADO, _ => MUDO });
                true
            }
            // LBCP_ITEM
            5 => {
                t.fundo(if estado == 1 { FUNDO_CLARO } else { SELECAO });
                true
            }
            _ => false,
        },
        Classe::Tarefa => match parte {
            // painel principal e as áreas dentro dele (instrução, conteúdo, expandido, links de
            // comando, controles, verificação, opções); o Executar pinta o conteúdo com elas
            1 | 2 | 4 | 6 | 7 | 9 | 14 | 21 => {
                t.fundo(FUNDO);
                true
            }
            // TDLG_SECONDARYPANEL, TDLG_FOOTNOTEPANE: faixa de baixo, com linha em cima
            8 | 15 => {
                t.fundo(FUNDO_CLARO);
                t.borda_lados(MUDO, [false, true, false, false]);
                true
            }
            // botões, nota de rodapé e rodapé expandido, dentro da faixa de baixo
            10 | 11 | 16 | 18 => {
                t.fundo(FUNDO_CLARO);
                true
            }
            // TDLG_FOOTNOTESEPARATOR
            17 => {
                t.linha(MUDO, true);
                true
            }
            _ => false,
        },
        Classe::Assistente => match parte {
            1..=3 => {
                t.fundo(FUNDO);
                true
            }
            4 => {
                t.fundo(FUNDO_CLARO);
                t.borda_lados(MUDO, [false, true, false, false]);
                true
            }
            _ => false,
        },
        Classe::Link => false,
    }
}

fn botao(t: &Tela, parte: i32, estado: i32) -> bool {
    match parte {
        // BP_PUSHBUTTON: repouso, sob o mouse, apertado, desabilitado, padrão
        1 => {
            let (f, b) = match estado {
                2 => (SELECAO, MUDO),
                3 => (SELECAO, DESTAQUE),
                4 => (FUNDO, MUDO),
                5 | 6 => (FUNDO_CLARO, DESTAQUE),
                _ => (FUNDO_CLARO, MUDO),
            };
            t.caixa(f, b);
            true
        }
        // BP_RADIOBUTTON: 1-4 desmarcado, 5-8 marcado
        2 => {
            let s = (estado.max(1) - 1) % 4;
            if estado >= 5 {
                t.glifo(G_CIRCULO_MARCADO, if s == 3 { MUDO } else { DESTAQUE }, 1.0);
            } else {
                t.glifo(G_CIRCULO, match s { 1 | 2 => TEXTO, 3 => MUDO, _ => APAGADO }, 1.0);
            }
            true
        }
        // BP_CHECKBOX: desmarcado, marcado, misto (e implícito, excluído), 4 estados cada
        3 => {
            let tipo = (estado.max(1) - 1) / 4;
            let s = (estado.max(1) - 1) % 4;
            let lado = t.largura().min(t.altura());
            let x = t.r.left + (t.largura() - lado) / 2;
            let y = t.r.top + (t.altura() - lado) / 2;
            let caixa = Tela { r: RECT { left: x, top: y, right: x + lado, bottom: y + lado }, sem_fundo: false, sem_borda: false, ..*t };
            match tipo {
                1 => {
                    caixa.fundo(if s == 3 { MUDO } else { DESTAQUE });
                    caixa.glifo(G_MARCA, FUNDO, 0.8);
                }
                2 => {
                    caixa.borda(if s == 3 { MUDO } else { DESTAQUE });
                    let m = (lado / 4).max(2);
                    caixa.preencher(RECT { left: x + m, top: y + m, right: x + lado - m, bottom: y + lado - m }, if s == 3 { MUDO } else { DESTAQUE });
                }
                _ => caixa.borda(match s { 1 | 2 => TEXTO, 3 => MUDO, _ => APAGADO }),
            }
            true
        }
        // BP_GROUPBOX
        4 => {
            t.borda(MUDO);
            true
        }
        // BP_COMMANDLINK
        6 => {
            match estado {
                2 => t.fundo(FUNDO_CLARO),
                3 => t.fundo(SELECAO),
                5 | 6 => t.borda(DESTAQUE),
                _ => {}
            }
            true
        }
        _ => false,
    }
}

fn rolagem(t: &Tela, parte: i32, estado: i32) -> bool {
    match parte {
        // SBP_ARROWBTN: cima, baixo, esquerda, direita (4 estados cada) e os de passagem
        1 => {
            let (direcao, s) = if estado >= 17 { (estado - 17, 0) } else { ((estado.max(1) - 1) / 4, (estado.max(1) - 1) % 4) };
            t.fundo(FUNDO);
            let g = [G_CIMA, G_BAIXO, G_ESQUERDA, G_DIREITA][direcao.clamp(0, 3) as usize];
            t.glifo(g, match s { 1 => TEXTO, 2 => TEXTO_FORTE, _ => MUDO }, 0.7);
            true
        }
        // SBP_THUMBBTNHORZ, SBP_THUMBBTNVERT: faixa no meio do trilho
        2 | 3 => {
            t.fundo(FUNDO);
            let r = t.r;
            let interno = if parte == 2 {
                let m = t.altura() / 4;
                RECT { left: r.left + 1, top: r.top + m, right: r.right - 1, bottom: r.bottom - m }
            } else {
                let m = t.largura() / 4;
                RECT { left: r.left + m, top: r.top + 1, right: r.right - m, bottom: r.bottom - 1 }
            };
            if !t.sem_fundo {
                t.preencher(interno, match estado { 2 => APAGADO, 3 => TEXTO, _ => MUDO });
            }
            true
        }
        // trilhos e caixa do canto
        4..=7 | 10 | 11 => {
            t.fundo(FUNDO);
            true
        }
        // pegadores
        8 | 9 => true,
        _ => false,
    }
}

/// Papel da cor do texto de cada parte.
fn texto_de(c: Classe, parte: i32, estado: i32) -> usize {
    match c {
        Classe::Menu => match (parte, estado) {
            (14 | 27, 3 | 4) | (8, 4..=6) => MUDO,
            (14 | 27, 2) | (8, 2 | 3) => TEXTO_FORTE,
            _ => TEXTO,
        },
        Classe::Botao => match parte {
            1 => match estado { 4 => MUDO, 2 | 3 => TEXTO_FORTE, _ => TEXTO },
            2 if estado == 4 || estado == 8 => MUDO,
            3 if (estado.max(1) - 1) % 4 == 3 => MUDO,
            4 if estado == 2 => MUDO,
            6 => if estado == 4 { MUDO } else { TEXTO_FORTE },
            _ => TEXTO,
        },
        Classe::Edicao => match (parte, estado) {
            (1, 8) => APAGADO,
            (1, 4) => MUDO,
            _ => TEXTO,
        },
        Classe::Combo => match (parte, estado) {
            (8, _) => APAGADO,
            (_, 4) => MUDO,
            _ => TEXTO,
        },
        Classe::Abas => match estado {
            4 => MUDO,
            3 | 5 => TEXTO_FORTE,
            _ => TEXTO,
        },
        Classe::Lista => match (parte, estado) {
            (5, _) => APAGADO,
            (6, _) => TEXTO_FORTE,
            (1, 4) => MUDO,
            _ => TEXTO,
        },
        Classe::Arvore | Classe::Ferramentas => if estado == 4 { MUDO } else { TEXTO },
        Classe::Dica | Classe::Tarefa => if parte == 2 || (c == Classe::Dica && parte == 4) { TEXTO_FORTE } else { TEXTO },
        Classe::Link => if estado == 2 { DESTAQUE } else { TEXTO },
        Classe::Assistente => if parte == 2 { TEXTO_FORTE } else { TEXTO },
        _ => TEXTO,
    }
}

fn cor_do_texto(t: HTHEME, parte: i32, estado: i32) -> Option<COLORREF> {
    classe(t).map(|c| cor(texto_de(c, parte, estado)))
}

const DTBG_CLIPRECT: u32 = 0x1;
const DTBG_OMITBORDER: u32 = 0x4;
const DTBG_OMITCONTENT: u32 = 0x8;
const DTBG_COMPUTINGREGION: u32 = 0x10;
const DTT_TEXTCOLOR: u32 = 0x1;
const DTT_CALCRECT: u32 = 0x200;
const DTT_GRAYED: u32 = 0x1;

/// Pinta com o DC salvo e o recorte pedido; `false`: a parte fica com o Windows.
unsafe fn desenhar(t: HTHEME, dc: HDC, parte: i32, estado: i32, r: RECT, recorte: Option<RECT>, flags: u32) -> bool {
    let Some(c) = classe(t) else { return false };
    let salvo = SaveDC(dc);
    if let Some(k) = recorte {
        IntersectClipRect(dc, k.left, k.top, k.right, k.bottom);
    }
    let tela = Tela { dc, r, sem_fundo: flags & DTBG_OMITCONTENT != 0, sem_borda: flags & DTBG_OMITBORDER != 0 };
    let feito = pintar(c, &tela, parte, estado);
    _ = RestoreDC(dc, salvo);
    feito
}

unsafe extern "system" fn draw_theme_background(t: HTHEME, dc: HDC, parte: i32, estado: i32, r: *const RECT, recorte: *const RECT) -> HRESULT {
    if ligado() && !r.is_null() {
        let (rr, k) = (*r, recorte.as_ref().copied());
        if seguro(false, || desenhar(t, dc, parte, estado, rr, k, 0)) {
            return S_OK;
        }
    }
    original::<FDrawThemeBackground>(&DRAW_THEME_BACKGROUND)(t, dc, parte, estado, r, recorte)
}

unsafe extern "system" fn draw_theme_background_ex(t: HTHEME, dc: HDC, parte: i32, estado: i32, r: *const RECT, o: *const DTBGOPTS) -> HRESULT {
    if ligado() && !r.is_null() {
        let opcoes = o.as_ref().map(|o| (o.dwFlags, o.rcClip));
        let flags = opcoes.map_or(0, |(f, _)| f);
        if flags & DTBG_COMPUTINGREGION == 0 {
            let k = opcoes.filter(|(f, _)| f & DTBG_CLIPRECT != 0).map(|(_, k)| k);
            let rr = *r;
            if seguro(false, || desenhar(t, dc, parte, estado, rr, k, flags)) {
                return S_OK;
            }
        }
    }
    original::<FDrawThemeBackgroundEx>(&DRAW_THEME_BACKGROUND_EX)(t, dc, parte, estado, r, o)
}

unsafe extern "system" fn draw_theme_text(t: HTHEME, dc: HDC, parte: i32, estado: i32, texto: PCWSTR, n: i32, f: u32, f2: u32, r: *const RECT) -> HRESULT {
    if ligado() && !r.is_null() && DRAW_THEME_TEXT_EX.load(Ordering::Acquire) != 0 {
        if let Some(c) = seguro(None, || cor_do_texto(t, parte, estado)) {
            let c = if f2 & DTT_GRAYED != 0 { cor(MUDO) } else { c };
            let mut rr = *r;
            let o = DTTOPTS { dwSize: size_of::<DTTOPTS>() as u32, dwFlags: DTTOPTS_FLAGS(DTT_TEXTCOLOR), crText: c, ..Default::default() };
            return original::<FDrawThemeTextEx>(&DRAW_THEME_TEXT_EX)(t, dc, parte, estado, texto, n, f, &mut rr, &o);
        }
    }
    original::<FDrawThemeText>(&DRAW_THEME_TEXT)(t, dc, parte, estado, texto, n, f, f2, r)
}

/// Texto com cor escolhida pelo programa (DTT_TEXTCOLOR) fica com ela.
unsafe extern "system" fn draw_theme_text_ex(t: HTHEME, dc: HDC, parte: i32, estado: i32, texto: PCWSTR, n: i32, f: u32, r: *mut RECT, o: *const DTTOPTS) -> HRESULT {
    let orig = original::<FDrawThemeTextEx>(&DRAW_THEME_TEXT_EX);
    if ligado() {
        let opcoes = o.as_ref().copied();
        let flags = opcoes.map_or(0, |o| o.dwFlags.0);
        if flags & (DTT_TEXTCOLOR | DTT_CALCRECT) == 0 {
            if let Some(c) = seguro(None, || cor_do_texto(t, parte, estado)) {
                let mut novo = opcoes.unwrap_or(DTTOPTS { dwSize: size_of::<DTTOPTS>() as u32, ..Default::default() });
                novo.dwFlags = DTTOPTS_FLAGS(flags | DTT_TEXTCOLOR);
                novo.crText = c;
                return orig(t, dc, parte, estado, texto, n, f, r, &novo);
            }
        }
    }
    orig(t, dc, parte, estado, texto, n, f, r, o)
}

const TMT_BORDERCOLOR: i32 = 3801;
const TMT_FILLCOLOR: i32 = 3802;
const TMT_TEXTCOLOR: i32 = 3803;
const TMT_FILLCOLORHINT: i32 = 3821;
const TMT_BORDERCOLORHINT: i32 = 3822;
const TMT_ACCENTCOLORHINT: i32 = 3823;

/// Cor que o tema do Windows define para a parte: a do tema da barra, só nas classes que a
/// barra pinta e só quando o Windows tem a cor (sem ela, o controle cai na cor do sistema).
unsafe extern "system" fn get_theme_color(t: HTHEME, parte: i32, estado: i32, prop: i32, saida: *mut COLORREF) -> HRESULT {
    let hr = original::<FGetThemeColor>(&GET_THEME_COLOR)(t, parte, estado, prop, saida);
    if hr.is_ok() && ligado() && !saida.is_null() {
        if let Some(c) = seguro(None, || classe(t)) {
            let papel = match prop {
                TMT_TEXTCOLOR => Some(texto_de(c, parte, estado)),
                TMT_FILLCOLOR | TMT_FILLCOLORHINT => Some(FUNDO),
                TMT_BORDERCOLOR | TMT_BORDERCOLORHINT => Some(MUDO),
                TMT_ACCENTCOLORHINT => Some(DESTAQUE),
                _ => None,
            };
            if let Some(p) = papel {
                *saida = cor(p);
            }
        }
    }
    hr
}

unsafe extern "system" fn get_theme_sys_color(t: HTHEME, i: i32) -> COLORREF {
    if ligado() {
        if let Some(p) = papel_da_cor(i) {
            return cor(p);
        }
    }
    original::<FGetThemeSysColor>(&GET_THEME_SYS_COLOR)(t, i)
}

unsafe extern "system" fn get_theme_sys_color_brush(t: HTHEME, i: i32) -> HBRUSH {
    if ligado() {
        if let Some(p) = papel_da_cor(i) {
            return pincel(p);
        }
    }
    original::<FGetThemeSysColorBrush>(&GET_THEME_SYS_COLOR_BRUSH)(t, i)
}
