//! Apps XAML (UWP com WinUI 2, e WinUI 3) nas cores do tema, como os mods "Styler" do
//! Windhawk: a DLL entra pelo XAML Diagnostics (`InitializeXamlDiagnosticsEx`) como TAP. O XAML
//! a carrega, chama `SetSite` com o `IXamlDiagnostics` e passa a avisar cada elemento da árvore
//! visual, na linha de execução da interface dele. Lá os recursos do app podem ser mexidos.
//!
//! O que muda: a cor dos pincéis de tema do WinUI (`TextFillColorPrimaryBrush`,
//! `ControlFillColorDefaultBrush`…) nos dicionários de `Application.Resources`. O pincel é o
//! mesmo objeto em todos os controles que o usam, então trocar o `Color` dele repinta tudo na
//! hora. A cor original fica guardada para voltar quando a barra desliga ou troca de tema.
//!
//! As interfaces do XAML são chamadas pela vtable, com o IID de cada framework (as mesmas
//! interfaces do `Windows.UI.Xaml` e do `Microsoft.UI.Xaml` só mudam de IID); IIDs e ordem dos
//! métodos tirados dos .winmd.

#![allow(non_snake_case)] // nomes das interfaces COM como no xamlom.h

use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::core::{implement, interface, IInspectable, IUnknown, IUnknownImpl, IUnknown_Vtbl, Interface, Ref, BOOL, BSTR, GUID, HRESULT, HSTRING};
use windows_collections::IMap;
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::core::{s, w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::Ole::{IObjectWithSite, IObjectWithSite_Impl};
use windows::Win32::UI::WindowsAndMessaging::SetTimer;

use super::{seguro, APAGADO, CLARO, CORES, DESTAQUE, ESCURO, FUNDO, FUNDO_CLARO, MODO, MUDO, SELECAO, TEXTO, VERMELHO, VERSAO};

/// CLSID do TAP desta DLL, pedido no `InitializeXamlDiagnosticsEx`.
pub const CLSID_TAP: GUID = GUID::from_u128(0x3a1f9c2e_6b7d_4e58_9a0c_5d2b8e4f7a61);

// ---------- XAML Diagnostics (xamlom.h) ----------

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ParentChildRelation {
    pai: u64,
    filho: u64,
    indice: u32,
}

/// As cadeias são BSTR do XAML, só lidas aqui.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VisualElement {
    handle: u64,
    arquivo: *const u16,
    linha: u32,
    coluna: u32,
    posicao: u32,
    hash: *const u16,
    tipo: *const u16,
    nome: *const u16,
    filhos: u32,
}

#[interface("18C9E2B6-3F43-4116-9F2B-FF935D7770D2")]
unsafe trait IXamlDiagnostics: IUnknown {
    fn GetDispatcher(&self, saida: *mut *mut c_void) -> HRESULT;
    fn GetUiLayer(&self, saida: *mut *mut c_void) -> HRESULT;
    fn GetApplication(&self, saida: *mut *mut c_void) -> HRESULT;
    fn GetIInspectableFromHandle(&self, handle: u64, saida: *mut *mut c_void) -> HRESULT;
    fn GetHandleFromIInspectable(&self, obj: *mut c_void, handle: *mut u64) -> HRESULT;
    /// Não usado (devolve E_INVALIDARG com qualquer retângulo); fica pela ordem da vtable.
    fn HitTest(&self, r: RECT, n: *mut u32, handles: *mut *mut u64) -> HRESULT;
    fn RegisterInstance(&self, obj: *mut c_void, handle: *mut u64) -> HRESULT;
}

#[interface("A593B11A-D17F-48BB-8F66-83910731C8A5")]
unsafe trait IVisualTreeService: IUnknown {
    fn AdviseVisualTreeChange(&self, aviso: *mut c_void) -> HRESULT;
    fn UnadviseVisualTreeChange(&self, aviso: *mut c_void) -> HRESULT;
    fn GetEnums(&self, n: *mut u32, enums: *mut *mut c_void) -> HRESULT;
    fn CreateInstance(&self, tipo: *const u16, valor: *const u16, handle: *mut u64) -> HRESULT;
    fn GetPropertyValuesChain(&self, h: u64, nf: *mut u32, f: *mut *mut c_void, np: *mut u32, p: *mut *mut c_void) -> HRESULT;
    fn SetProperty(&self, h: u64, valor: u64, indice: u32) -> HRESULT;
    fn ClearProperty(&self, h: u64, indice: u32) -> HRESULT;
    fn GetCollectionCount(&self, h: u64, n: *mut u32) -> HRESULT;
    fn GetCollectionElements(&self, h: u64, inicio: u32, n: *mut u32, elementos: *mut *mut c_void) -> HRESULT;
    fn AddChild(&self, pai: u64, filho: u64, indice: u32) -> HRESULT;
    fn RemoveChild(&self, pai: u64, indice: u32) -> HRESULT;
    fn ClearChildren(&self, pai: u64) -> HRESULT;
}

#[interface("130F5136-EC43-4F61-89C7-9801A36D2E95")]
unsafe trait IVisualTreeService2: IVisualTreeService {
    fn GetPropertyIndex(&self, h: u64, nome: *const u16, indice: *mut u32) -> HRESULT;
    fn GetProperty(&self, h: u64, indice: u32, valor: *mut u64) -> HRESULT;
    fn ReplaceResource(&self, dicionario: u64, chave: u64, novo: u64) -> HRESULT;
}

#[interface("AA7A8931-80E4-4FEC-8F3B-553F87B4966E")]
pub unsafe trait IVisualTreeServiceCallback: IUnknown {
    fn OnVisualTreeChange(&self, relacao: ParentChildRelation, elemento: VisualElement, mudanca: i32) -> HRESULT;
}

/// `VisualMutationType::Add`.
const ADICIONADO: i32 = 0;

// ---------- frameworks ----------

/// IIDs de um framework XAML.
struct Framework {
    /// Tipo do SolidColorBrush, para o CreateInstance do XAML Diagnostics.
    tipo_pincel: &'static str,
    /// IApplication: 6 get_Resources.
    application: GUID,
    /// IResourceDictionary: 8 get_MergedDictionaries, 9 get_ThemeDictionaries.
    resource_dictionary: GUID,
    /// ISolidColorBrush: 6 get_Color, 7 put_Color.
    solid_color_brush: GUID,
    /// Propriedades de pincel dos elementos (interface, índice do get na vtable): Foreground,
    /// Background e BorderBrush de TextBlock, ContentPresenter, Control, Panel e Border.
    pinceis_dos_elementos: &'static [(GUID, usize)],
    /// IBrush, para o put_Background.
    brush: GUID,
    /// IBorder: 10 get_Background, 11 put_Background (igual nos dois).
    border: GUID,
}

const UWP_TEXTBLOCK: GUID = GUID::from_u128(0xae2d9271_3b4a_45fc_8468_f7949548f4d5);
const UWP_CONTENTPRESENTER: GUID = GUID::from_u128(0x79fde5b4_cd37_491c_8845_daf472defff6);
const UWP_CONTROL: GUID = GUID::from_u128(0xa8912263_2951_4f58_a9c5_5a134eaa7f07);
const UWP_PANEL: GUID = GUID::from_u128(0xa50a4bbd_8361_469c_90da_e9a40c7474df);
const UWP_BORDER: GUID = GUID::from_u128(0x797c4539_45bd_4633_a044_bfb02ef5170f);
const WINUI3_TEXTBLOCK: GUID = GUID::from_u128(0x1ac8d84f_392c_5c7e_83f5_a53e3bf0abb0);
const WINUI3_CONTENTPRESENTER: GUID = GUID::from_u128(0x075e6ca8_da46_5ec6_8c64_03b3ddeaf74f);
const WINUI3_CONTROL: GUID = GUID::from_u128(0x857d6e8a_d45a_5c69_a99c_bf6a5c54fb38);
const WINUI3_PANEL: GUID = GUID::from_u128(0x27a1b418_56f3_525e_b883_cefed905eed3);
const WINUI3_BORDER: GUID = GUID::from_u128(0x1ca13b47_ff5c_5abc_a411_a177df9482a9);

const UWP: Framework = Framework {
    tipo_pincel: "Windows.UI.Xaml.Media.SolidColorBrush",
    application: GUID::from_u128(0x74b861a1_7487_46a9_9a6e_c78b512726c5),
    resource_dictionary: GUID::from_u128(0xc1ea4f24_d6de_4191_8e3a_f48601f7489c),
    solid_color_brush: GUID::from_u128(0x9d850850_66f3_48df_9a8f_824bd5e070af),
    pinceis_dos_elementos: &[
        (UWP_TEXTBLOCK, 18),
        (UWP_CONTENTPRESENTER, 26),
        (UWP_CONTROL, 18),
        (UWP_CONTROL, 36),
        (UWP_CONTROL, 40),
        (UWP_PANEL, 7),
        (UWP_BORDER, 6),
        (UWP_BORDER, 10),
    ],
    brush: GUID::from_u128(0x8806a321_1e06_422c_a1cc_01696559e021),
    border: UWP_BORDER,
};

const WINUI3: Framework = Framework {
    tipo_pincel: "Microsoft.UI.Xaml.Media.SolidColorBrush",
    application: GUID::from_u128(0x06a8f4e7_1146_55af_820d_ebd55643b021),
    resource_dictionary: GUID::from_u128(0x1b690975_a710_5783_a6e1_15836f6186c2),
    solid_color_brush: GUID::from_u128(0xb3865c31_37c8_55c1_8a72_d41c67642e2a),
    pinceis_dos_elementos: &[
        (WINUI3_TEXTBLOCK, 18),
        (WINUI3_CONTENTPRESENTER, 26),
        (WINUI3_CONTENTPRESENTER, 44),
        (WINUI3_CONTENTPRESENTER, 52),
        (WINUI3_CONTROL, 24),
        (WINUI3_CONTROL, 40),
        (WINUI3_CONTROL, 46),
        (WINUI3_PANEL, 7),
        (WINUI3_BORDER, 6),
        (WINUI3_BORDER, 10),
    ],
    brush: GUID::from_u128(0x2de3cb83_1329_5679_88f8_c822bc5442cb),
    border: WINUI3_BORDER,
};

/// Windows.UI.Color.
#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
struct Cor {
    a: u8,
    r: u8,
    g: u8,
    b: u8,
}

fn cor_do_tema(papel: usize) -> Cor {
    let c = CORES[papel].load(Ordering::Relaxed);
    Cor { a: 0xFF, r: (c >> 16) as u8, g: (c >> 8) as u8, b: c as u8 }
}

/// Pincéis de tema do WinUI e o papel do tema de cada um, no critério dos controles Win32
/// (DESIGN.md, "Controles das outras janelas").
const PINCEIS: &[(&str, usize)] = &[
    // fundos
    ("ApplicationPageBackgroundThemeBrush", FUNDO),
    ("SolidBackgroundFillColorBaseBrush", FUNDO),
    ("SolidBackgroundFillColorBaseAltBrush", FUNDO),
    ("SolidBackgroundFillColorSecondaryBrush", FUNDO),
    ("SolidBackgroundFillColorTertiaryBrush", FUNDO_CLARO),
    ("SolidBackgroundFillColorQuarternaryBrush", FUNDO_CLARO),
    ("LayerFillColorDefaultBrush", FUNDO),
    ("LayerFillColorAltBrush", FUNDO_CLARO),
    ("LayerOnAcrylicFillColorDefaultBrush", FUNDO),
    ("LayerOnMicaBaseAltFillColorDefaultBrush", FUNDO_CLARO),
    ("LayerOnMicaBaseAltFillColorSecondaryBrush", FUNDO_CLARO),
    ("LayerOnMicaBaseAltFillColorTertiaryBrush", FUNDO),
    ("CardBackgroundFillColorDefaultBrush", FUNDO_CLARO),
    ("CardBackgroundFillColorSecondaryBrush", FUNDO),
    // controles: botão em fundo-claro, sob o mouse e apertado em seleção
    ("ControlFillColorDefaultBrush", FUNDO_CLARO),
    ("ControlFillColorSecondaryBrush", SELECAO),
    ("ControlFillColorTertiaryBrush", SELECAO),
    ("ControlFillColorDisabledBrush", FUNDO),
    ("ControlFillColorInputActiveBrush", FUNDO),
    ("ControlSolidFillColorDefaultBrush", FUNDO_CLARO),
    ("SubtleFillColorSecondaryBrush", FUNDO_CLARO),
    ("SubtleFillColorTertiaryBrush", SELECAO),
    // bordas
    ("ControlStrokeColorDefaultBrush", MUDO),
    ("ControlStrokeColorSecondaryBrush", MUDO),
    ("ControlStrongStrokeColorDefaultBrush", APAGADO),
    ("CardStrokeColorDefaultBrush", MUDO),
    ("CardStrokeColorDefaultSolidBrush", MUDO),
    ("DividerStrokeColorDefaultBrush", MUDO),
    ("SurfaceStrokeColorDefaultBrush", MUDO),
    ("SurfaceStrokeColorFlyoutBrush", MUDO),
    // texto
    ("TextFillColorPrimaryBrush", TEXTO),
    ("TextFillColorSecondaryBrush", APAGADO),
    ("TextFillColorTertiaryBrush", APAGADO),
    ("TextFillColorDisabledBrush", MUDO),
    ("TextOnAccentFillColorPrimaryBrush", FUNDO),
    ("TextOnAccentFillColorSecondaryBrush", FUNDO),
    // destaque
    ("AccentFillColorDefaultBrush", DESTAQUE),
    ("AccentFillColorSecondaryBrush", DESTAQUE),
    ("AccentFillColorTertiaryBrush", DESTAQUE),
    ("AccentTextFillColorPrimaryBrush", DESTAQUE),
    ("AccentTextFillColorSecondaryBrush", DESTAQUE),
    ("AccentTextFillColorTertiaryBrush", DESTAQUE),
    ("SystemFillColorCriticalBrush", VERMELHO),
];

// ---------- chamadas pela vtable ----------

/// `obj` na interface `iid` (QueryInterface).
fn como<T: Interface>(obj: &T, iid: &GUID) -> Option<IUnknown> {
    let mut p = null_mut();
    unsafe { obj.query(iid, &mut p).ok().ok()? };
    (!p.is_null()).then(|| unsafe { IUnknown::from_raw(p) })
}

unsafe fn metodo(obj: &IUnknown, indice: usize) -> usize {
    let vtabela = *(obj.as_raw() as *const *const usize);
    *vtabela.add(indice)
}

/// Propriedade que devolve uma interface (`get_X(out T)`), no índice `indice` da vtable.
fn obter(obj: &IUnknown, indice: usize) -> Option<IUnknown> {
    unsafe {
        let f: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT = std::mem::transmute(metodo(obj, indice));
        let mut p = null_mut();
        f(obj.as_raw(), &mut p).ok().ok()?;
        (!p.is_null()).then(|| IUnknown::from_raw(p))
    }
}

/// IVector<T>: 6 GetAt, 7 get_Size.
fn itens(vetor: &IUnknown) -> Vec<IUnknown> {
    unsafe {
        let tamanho: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT = std::mem::transmute(metodo(vetor, 7));
        let em: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT = std::mem::transmute(metodo(vetor, 6));
        let mut n = 0u32;
        if tamanho(vetor.as_raw(), &mut n).is_err() {
            return Vec::new();
        }
        (0..n.min(256))
            .filter_map(|i| {
                let mut p = null_mut();
                (em(vetor.as_raw(), i, &mut p).is_ok() && !p.is_null()).then(|| IUnknown::from_raw(p))
            })
            .collect()
    }
}

fn ler_cor(pincel: &IUnknown) -> Option<Cor> {
    unsafe {
        let f: unsafe extern "system" fn(*mut c_void, *mut Cor) -> HRESULT = std::mem::transmute(metodo(pincel, 6));
        let mut c = Cor { a: 0, r: 0, g: 0, b: 0 };
        f(pincel.as_raw(), &mut c).is_ok().then_some(c)
    }
}

fn gravar_cor(pincel: &IUnknown, c: Cor) -> HRESULT {
    unsafe {
        let f: unsafe extern "system" fn(*mut c_void, Cor) -> HRESULT = std::mem::transmute(metodo(pincel, 7));
        f(pincel.as_raw(), c)
    }
}

fn chave(texto: &str) -> Option<IInspectable> {
    PropertyValue::CreateString(&HSTRING::from(texto)).ok()
}

// ---------- o que cada linha de execução da interface guarda ----------

#[derive(Default)]
struct Linha {
    /// VERSAO já aplicada (None: nenhuma).
    versao: Option<u32>,
    app: Option<(IUnknown, &'static Framework)>,
    diag: Option<IXamlDiagnostics>,
    /// Cor original dos pincéis do WinUI no tema atual do app e o papel de cada uma, na ordem
    /// de `PINCEIS` (a primeira decide quando duas são iguais).
    por_cor: Vec<(Cor, usize)>,
    /// Pincéis dos dicionários recoloridos, com a cor de antes.
    pinceis: Vec<(IUnknown, Cor)>,
    /// Pincéis achados nos elementos, fora dos dicionários (o texto padrão vem do framework),
    /// com a cor de antes.
    descobertos: Vec<(IUnknown, Cor)>,
    /// Pincéis já olhados, recoloridos ou não.
    vistos: HashSet<usize>,
    /// Entradas de dicionário trocadas por um pincel nosso: dicionário, chave e o valor de antes
    /// (handles do XAML Diagnostics).
    trocados: Vec<(u64, u64, u64)>,
    /// Caminho até a raiz de cada janela do app (handles do XAML Diagnostics): RootScrollViewer
    /// e o ScrollContentPresenter dele; a raiz é o Border logo abaixo.
    acima_da_raiz: HashSet<u64>,
    /// Raízes das janelas.
    raizes: Vec<u64>,
    /// Raízes com o fundo do tema e o Background de antes.
    pintadas: Vec<(IUnknown, Option<IUnknown>)>,
}

/// Pincel sólido achado num dicionário.
struct Achado {
    nome: Option<HSTRING>,
    pincel: IUnknown,
    dicionario: IUnknown,
    chave: IInspectable,
}

thread_local! {
    static LINHA: RefCell<Linha> = RefCell::new(Linha::default());
}

fn papel_pela_cor(por_cor: &[(Cor, usize)], c: Cor) -> Option<usize> {
    por_cor.iter().find(|&&(k, _)| k == c).map(|&(_, p)| p).or_else(|| papel_do_cinza(c))
}

/// Cinzas do modo escuro do Windows em pincéis fora do WinUI: o Gerenciador de Tarefas pinta o
/// cabeçalho das páginas com um pincel próprio #191919. Só opacos e só no modo escuro.
fn papel_do_cinza(c: Cor) -> Option<usize> {
    if c.a != 0xFF || MODO.load(Ordering::Relaxed) != ESCURO || c.r != c.g || c.g != c.b {
        return None;
    }
    match c.r {
        0x19 | 0x1C | 0x20 => Some(FUNDO),
        0x2B | 0x2C => Some(FUNDO_CLARO),
        _ => None,
    }
}

/// Nome da chave (as de estilo implícito são tipos, não texto).
fn nome(k: &IInspectable) -> Option<HSTRING> {
    k.cast::<IPropertyValue>().ok()?.GetString().ok()
}

/// Pincéis sólidos do dicionário, dos de tema (só o do modo do tema) e dos mesclados, até 8
/// níveis, com o nome da chave. Percorre as entradas em vez de pedir pela chave: o `Lookup`
/// devolve outra instância do pincel, não a que os controles usam.
fn juntar(dic: &IUnknown, f: &Framework, temas: &[IInspectable], achados: &mut Vec<Achado>, vistos: &mut HashSet<usize>, nivel: u32) {
    if nivel > 8 {
        return;
    }
    if let Some(it) = dic.cast::<IMap<IInspectable, IInspectable>>().ok().and_then(|m| m.First().ok()) {
        while it.HasCurrent().unwrap_or(false) {
            if let Ok(kv) = it.Current() {
                if let (Some(p), Ok(chave)) = (kv.Value().ok().and_then(|v| como(&v, &f.solid_color_brush)), kv.Key()) {
                    if vistos.insert(p.as_raw() as usize) {
                        achados.push(Achado { nome: nome(&chave), pincel: p, dicionario: dic.clone(), chave });
                    }
                }
            }
            if !it.MoveNext().unwrap_or(false) {
                break;
            }
        }
    }
    if let Some(td) = obter(dic, 9).and_then(|t| t.cast::<IMap<IInspectable, IInspectable>>().ok()) {
        for t in temas {
            if td.HasKey(t).unwrap_or(false) {
                if let Some(d) = td.Lookup(t).ok().and_then(|v| como(&v, &f.resource_dictionary)) {
                    juntar(&d, f, temas, achados, vistos, nivel + 1);
                }
            }
        }
    }
    if let Some(md) = obter(dic, 8) {
        for d in itens(&md) {
            juntar(&d, f, temas, achados, vistos, nivel + 1);
        }
    }
}

/// Posição do pincel do WinUI na tabela `PINCEIS`.
fn conhecido(k: &Option<HSTRING>) -> Option<usize> {
    let k = k.as_ref()?;
    PINCEIS.iter().position(|(c, _)| k == *c)
}

/// Devolve as cores de antes e, com a barra ligada, pinta de novo com o tema atual.
///
/// Os pincéis do WinUI vão pelo nome. Os do próprio app (o Captura tem `CanvasBackgroundBrush`)
/// e os do framework costumam sair das cores do WinUI: vão pela cor original, quando ela é a de
/// um pincel do WinUI no mesmo app.
fn aplicar(l: &mut Linha) {
    l.versao = Some(VERSAO.load(Ordering::Relaxed));
    for (p, antes) in l.pinceis.drain(..).chain(l.descobertos.iter().cloned()) {
        _ = gravar_cor(&p, antes);
    }
    if let Some(svc) = l.diag.as_ref().and_then(|d| d.cast::<IVisualTreeService2>().ok()) {
        for (dic, chave, antes) in l.trocados.drain(..) {
            _ = unsafe { svc.ReplaceResource(dic, chave, antes) };
        }
    }
    for (borda, antes) in l.pintadas.drain(..) {
        _ = unsafe { por_background(&borda, antes.as_ref().map_or(null_mut(), |a| a.as_raw())) };
    }
    l.por_cor.clear();
    l.vistos.clear();
    let modo = MODO.load(Ordering::Relaxed);
    let Some((app, f)) = l.app.as_ref().filter(|_| matches!(modo, ESCURO | CLARO)) else {
        l.descobertos.clear();
        return;
    };
    let Some(recursos) = como(app, &f.application).and_then(|a| obter(&a, 6)) else { return };
    let temas: Vec<IInspectable> = (if modo == CLARO { &["Light"][..] } else { &["Default", "Dark"][..] }).iter().filter_map(|t| chave(t)).collect();
    let mut achados = Vec::new();
    juntar(&recursos, f, &temas, &mut achados, &mut l.vistos, 0);
    let mut por_cor: Vec<(usize, Cor)> = achados.iter().filter_map(|a| Some((conhecido(&a.nome)?, ler_cor(&a.pincel).filter(|c| c.a != 0)?))).collect();
    por_cor.sort_by_key(|&(i, _)| i);
    l.por_cor = por_cor.into_iter().map(|(i, c)| (c, PINCEIS[i].1)).collect();
    for a in achados {
        let Some(antes) = ler_cor(&a.pincel) else { continue };
        let Some(papel) = conhecido(&a.nome).map(|i| PINCEIS[i].1).or_else(|| papel_pela_cor(&l.por_cor, antes)) else {
            continue;
        };
        let hr = gravar_cor(&a.pincel, cor_do_tema(papel));
        if hr == E_ACCESSDENIED {
            if let Some(t) = l.diag.as_ref().and_then(|d| trocar(d, f, &a, papel)) {
                l.trocados.push(t);
            }
        } else if hr.is_ok() {
            l.pinceis.push((a.pincel, antes));
        }
    }
    for (p, antes) in &l.descobertos {
        l.vistos.insert(p.as_raw() as usize);
        if let Some(papel) = papel_pela_cor(&l.por_cor, *antes) {
            _ = gravar_cor(p, cor_do_tema(papel));
        }
    }
    for h in l.raizes.clone() {
        pintar_raiz(l, h);
    }
}

/// Fundo do tema na raiz de uma janela do app. A raiz é transparente, e por trás aparece o Mica
/// da janela, ou a cor reserva dele (#202020) quando o Mica está desligado, como na moldura da
/// barra: a Configurações ficava com cartões no tema sobre um fundo cinza.
fn pintar_raiz(l: &mut Linha, h: u64) {
    let (Some(diag), Some((_, f))) = (l.diag.as_ref(), l.app.as_ref()) else { return };
    let Some(svc) = diag.cast::<IVisualTreeService>().ok() else { return };
    let c = cor_do_tema(FUNDO);
    let tipo = BSTR::from(f.tipo_pincel);
    let valor = BSTR::from(format!("#FF{:02X}{:02X}{:02X}", c.r, c.g, c.b));
    unsafe {
        let objeto = |h: u64| {
            let mut p = null_mut();
            (diag.GetIInspectableFromHandle(h, &mut p).is_ok() && !p.is_null()).then(|| IUnknown::from_raw(p))
        };
        let Some(borda) = objeto(h).and_then(|o| como(&o, &f.border)) else { return };
        let mut novo = 0u64;
        if svc.CreateInstance(tipo.as_ptr(), valor.as_ptr(), &mut novo).is_err() {
            return;
        }
        let Some(pincel) = objeto(novo).and_then(|o| como(&o, &f.brush)) else { return };
        let antes = obter(&borda, 10);
        if por_background(&borda, pincel.as_raw()).is_ok() {
            l.pintadas.push((borda, antes));
        }
    }
}

/// IBorder::put_Background.
unsafe fn por_background(borda: &IUnknown, pincel: *mut c_void) -> HRESULT {
    let put: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT = std::mem::transmute(metodo(borda, 11));
    put(borda.as_raw(), pincel)
}

/// Pincel selado (os do próprio framework recusam o put_Color com E_ACCESSDENIED): troca a
/// entrada do dicionário por um pincel novo na cor do papel, pelo ReplaceResource do XAML
/// Diagnostics, o mesmo do Hot Reload, que atualiza quem usa o recurso.
fn trocar(diag: &IXamlDiagnostics, f: &Framework, a: &Achado, papel: usize) -> Option<(u64, u64, u64)> {
    let svc = diag.cast::<IVisualTreeService2>().ok()?;
    let c = cor_do_tema(papel);
    let tipo = BSTR::from(f.tipo_pincel);
    let valor = BSTR::from(format!("#FF{:02X}{:02X}{:02X}", c.r, c.g, c.b));
    let handle = |o: *mut c_void| {
        let mut h = 0u64;
        unsafe { diag.GetHandleFromIInspectable(o, &mut h) }.is_ok().then_some(h)
    };
    unsafe {
        let mut novo = 0u64;
        svc.CreateInstance(tipo.as_ptr(), valor.as_ptr(), &mut novo).ok().ok()?;
        let mut chave = 0u64;
        diag.RegisterInstance(a.chave.as_raw(), &mut chave).ok().ok()?;
        let (dic, antes) = (handle(a.dicionario.as_raw())?, handle(a.pincel.as_raw())?);
        svc.ReplaceResource(dic, chave, novo).ok().ok()?;
        Some((dic, chave, antes))
    }
}

/// Foreground, Background e BorderBrush de um elemento novo: pincel ainda não visto com a cor
/// de um pincel do WinUI ganha o papel dele.
fn olhar_elemento(l: &mut Linha, obj: &IUnknown) {
    let Some(f) = l.app.as_ref().map(|&(_, f)| f) else { return };
    if l.por_cor.is_empty() {
        return;
    }
    for &(iid, indice) in f.pinceis_dos_elementos {
        let Some(p) = como(obj, &iid).and_then(|i| obter(&i, indice)).and_then(|b| como(&b, &f.solid_color_brush)) else {
            continue;
        };
        if !l.vistos.insert(p.as_raw() as usize) {
            continue;
        }
        let Some(antes) = ler_cor(&p) else { continue };
        if let Some(papel) = papel_pela_cor(&l.por_cor, antes) {
            _ = gravar_cor(&p, cor_do_tema(papel));
            l.descobertos.push((p, antes));
        }
    }
}

/// Temporizador da linha: refaz quando a barra troca de tema, liga ou desliga.
unsafe extern "system" fn conferir(_: HWND, _: u32, _: usize, _: u32) {
    seguro((), || {
        LINHA.with_borrow_mut(|l| {
            if l.versao != Some(VERSAO.load(Ordering::Relaxed)) {
                aplicar(l);
            }
        })
    });
}

// ---------- o TAP ----------

#[implement(IObjectWithSite, IVisualTreeServiceCallback)]
struct Tap {
    site: Mutex<Option<IUnknown>>,
}

impl IObjectWithSite_Impl for Tap_Impl {
    fn SetSite(&self, site: Ref<'_, IUnknown>) -> windows::core::Result<()> {
        let site = site.cloned();
        *self.site.lock().unwrap_or_else(|e| e.into_inner()) = site.clone();
        let Some(site) = site else { return Ok(()) };
        // a barra também conecta de fora (apps com pacote, onde o gancho não entra): se o gancho
        // já conectou, ou se é a shell, fica só a primeira
        if FORA.load(Ordering::Relaxed) || ATIVO.swap(true, Ordering::Relaxed) {
            return Ok(());
        }
        CONECTADO.store(true, Ordering::Relaxed);
        // o AdviseVisualTreeChange chamado daqui trava esperando a linha da interface
        let servico = site.cast::<IVisualTreeService>()?.into_raw() as usize;
        let aviso = self.to_interface::<IVisualTreeServiceCallback>().into_raw() as usize;
        std::thread::spawn(move || unsafe {
            let servico = IVisualTreeService::from_raw(servico as *mut c_void);
            let aviso = IVisualTreeServiceCallback::from_raw(aviso as *mut c_void);
            _ = servico.AdviseVisualTreeChange(aviso.as_raw());
        });
        Ok(())
    }

    fn GetSite(&self, iid: *const GUID, saida: *mut *mut c_void) -> windows::core::Result<()> {
        unsafe {
            *saida = null_mut();
            match self.site.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                Some(s) => s.query(iid, saida).ok(),
                None => Err(E_FAIL.into()),
            }
        }
    }
}

impl IVisualTreeServiceCallback_Impl for Tap_Impl {
    unsafe fn OnVisualTreeChange(&self, relacao: ParentChildRelation, elemento: VisualElement, mudanca: i32) -> HRESULT {
        if mudanca != ADICIONADO {
            return S_OK;
        }
        let tipo = if elemento.tipo.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(elemento.tipo, (0..).take_while(|&i| *elemento.tipo.add(i) != 0).count())
        };
        seguro((), || {
            // reentrada (o ReplaceResource remonta elementos na hora): o elemento fica para depois
            LINHA.with(|linha| {
                let Ok(mut l) = linha.try_borrow_mut() else { return };
                let l = &mut *l;
                if l.app.is_none() {
                    let Some(diag) = self.site.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|s| s.cast::<IXamlDiagnostics>().ok()) else {
                        return;
                    };
                    l.app = aplicativo(&diag);
                    if l.app.is_none() {
                        return;
                    }
                    l.diag = Some(diag);
                    SetTimer(None, 0, 1000, Some(conferir));
                }
                if l.versao != Some(VERSAO.load(Ordering::Relaxed)) {
                    aplicar(l);
                }
                if tipo.ends_with(w!(".RootScrollViewer").as_wide())
                    || (tipo.ends_with(w!(".ScrollContentPresenter").as_wide()) && l.acima_da_raiz.contains(&relacao.pai))
                {
                    l.acima_da_raiz.insert(elemento.handle);
                } else if tipo.ends_with(w!(".Border").as_wide()) && l.acima_da_raiz.contains(&relacao.pai) {
                    l.raizes.push(elemento.handle);
                    if matches!(MODO.load(Ordering::Relaxed), ESCURO | CLARO) {
                        pintar_raiz(l, elemento.handle);
                    }
                }
                let mut p = null_mut();
                if let Some(d) = l.diag.clone() {
                    if d.GetIInspectableFromHandle(elemento.handle, &mut p).is_ok() && !p.is_null() {
                        olhar_elemento(l, &IUnknown::from_raw(p));
                    }
                }
            });
        });
        S_OK
    }
}

/// Application do XAML e o framework dele.
fn aplicativo(diag: &IXamlDiagnostics) -> Option<(IUnknown, &'static Framework)> {
    let mut p = null_mut();
    unsafe { diag.GetApplication(&mut p).ok().ok()? };
    if p.is_null() {
        return None;
    }
    let app = unsafe { IUnknown::from_raw(p) };
    [&UWP, &WINUI3].into_iter().find(|f| como(&app, &f.application).is_some()).map(|f| (app, f))
}

// ---------- entrada no processo ----------

/// DLLs que exportam o `InitializeXamlDiagnosticsEx`: a do XAML do sistema (UWP, WinUI 2) e a
/// do WinUI 3 (Windows App SDK).
const DLLS_XAML: [&str; 2] = ["windows.ui.xaml.dll", "microsoft.internal.frameworkudk.dll"];
/// Conexão do XAML Diagnostics feita (só uma por processo).
static CONECTADO: AtomicBool = AtomicBool::new(false);
/// Tentativa em andamento.
static TENTANDO: AtomicBool = AtomicBool::new(false);
/// TAP com o aviso da árvore ligado neste processo.
static ATIVO: AtomicBool = AtomicBool::new(false);

/// Processos da shell ficam de fora. Com o diagnóstico ligado, o Explorer (a shell e as janelas
/// do Explorador de Arquivos) caiu pelo CoreMessaging: callback com E_FAIL tratado como falha
/// fatal no Windows.UI.Xaml.dll, tanto ligando o XAML do sistema quanto só o WinUI 3. O Explorer
/// tem os dois frameworks; processo com os dois também fica de fora (`entrar`).
const SHELL: &[&str] = &[
    "explorer.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "searchhost.exe",
    "textinputhost.exe",
    "shellhost.exe",
    "lockapp.exe",
    "applicationframehost.exe",
];
static FORA: AtomicBool = AtomicBool::new(false);

#[repr(C)]
struct UnicodeString {
    bytes: u16,
    capacidade: u16,
    texto: *const u16,
}

/// LDR_DLL_NOTIFICATION_DATA (carga).
#[repr(C)]
struct DllCarregada {
    flags: u32,
    caminho: *const UnicodeString,
    nome: *const UnicodeString,
}

type RegistrarAviso = unsafe extern "system" fn(u32, unsafe extern "system" fn(u32, *const DllCarregada, *mut c_void), *mut c_void, *mut *mut c_void) -> i32;
type Ixde = unsafe extern "system" fn(PCWSTR, u32, PCWSTR, PCWSTR, GUID, PCWSTR) -> HRESULT;

/// No DllMain: o tema entra no XAML quando ele carrega, antes de o programa montar as janelas
/// (as ilhas XAML que já existem quando o TAP entra não passam pelo aviso da árvore). Se o XAML
/// já estava carregado (programa aberto antes da barra), entra agora.
pub fn iniciar() {
    let exe = unsafe {
        let mut buf = [0u16; 260];
        let n = GetModuleFileNameW(None, &mut buf) as usize;
        String::from_utf16_lossy(&buf[..n]).to_lowercase()
    };
    if exe.ends_with(r"\barra.exe") {
        return;
    }
    if SHELL.iter().any(|n| exe.ends_with(&format!(r"\{n}"))) {
        FORA.store(true, Ordering::Relaxed);
        return;
    }
    // o aviso de carga nunca sai: a DLL não pode sair do processo depois dele
    super::prender();
    unsafe {
        if let Some(f) = GetModuleHandleW(w!("ntdll.dll")).ok().and_then(|m| GetProcAddress(m, s!("LdrRegisterDllNotification"))) {
            let registrar: RegistrarAviso = std::mem::transmute(f);
            let mut cookie = null_mut();
            registrar(0, carregada, null_mut(), &mut cookie);
        }
        for (i, nome) in DLLS_XAML.iter().enumerate() {
            if GetModuleHandleW(&HSTRING::from(*nome)).is_ok() {
                entrar(i);
            }
        }
    }
}

/// Aviso do carregador (com o loader lock: só dispara a linha de execução).
unsafe extern "system" fn carregada(motivo: u32, dados: *const DllCarregada, _: *mut c_void) {
    if motivo != 1 || dados.is_null() || (*dados).nome.is_null() {
        return;
    }
    let u = &*(*dados).nome;
    let nome = String::from_utf16_lossy(std::slice::from_raw_parts(u.texto, u.bytes as usize / 2)).to_lowercase();
    if nome == "dui70.dll" {
        super::controles::instalar_dui();
    }
    if let Some(i) = DLLS_XAML.iter().position(|d| *d == nome) {
        entrar(i);
    }
}

/// Janela-ponte de ilha XAML criada (gancho CBT): sem conexão ainda, tenta de novo. O WinUI 3
/// só aceita a conexão com o núcleo XAML ativo, e no Explorer ele sobe com a primeira janela do
/// Explorador de Arquivos.
pub fn janela_criada(classe: &str) {
    match classe {
        "Microsoft.UI.Content.DesktopChildSiteBridge" => entrar(1),
        "Windows.UI.Composition.DesktopWindowContentBridge" => entrar(0),
        _ => {}
    }
}

/// Chama o InitializeXamlDiagnosticsEx do framework `i` no próprio processo, com esta DLL como
/// TAP. Logo depois da carga o XAML ainda pode recusar: tenta de novo algumas vezes. Processo
/// com os dois frameworks fica de fora (ver `SHELL`).
fn entrar(i: usize) {
    if FORA.load(Ordering::Relaxed) || CONECTADO.load(Ordering::Relaxed) || TENTANDO.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        seguro((), || unsafe {
            if GetModuleHandleW(&HSTRING::from(DLLS_XAML[1 - i])).is_ok() {
                FORA.store(true, Ordering::Relaxed);
                return;
            }
            let Ok(m) = GetModuleHandleW(&HSTRING::from(DLLS_XAML[i])) else { return };
            if i == 0 {
                seguro((), composicao::instalar);
            }
            let Some(f) = GetProcAddress(m, s!("InitializeXamlDiagnosticsEx")) else { return };
            let ixde: Ixde = std::mem::transmute(f);
            let mut buf = [0u16; 260];
            let n = GetModuleFileNameW(Some(super::modulo()), &mut buf) as usize;
            let dll = HSTRING::from_wide(&buf[..n]);
            let pid = windows::Win32::System::Threading::GetCurrentProcessId();
            for espera in [50, 250, 1000, 3000, 10000] {
                std::thread::sleep(std::time::Duration::from_millis(espera));
                let mut hr = E_FAIL;
                // o XAML só aceita nomes VisualDiagConnectionN e devolve ERROR_NOT_FOUND quando o
                // nome já está em uso (outro TAP, o Visual Studio)
                for c in 1..=100 {
                    let ponto = HSTRING::from(format!("VisualDiagConnection{c}"));
                    hr = ixde(PCWSTR(ponto.as_ptr()), pid, w!(""), PCWSTR(dll.as_ptr()), CLSID_TAP, PCWSTR::null());
                    if hr != HRESULT::from_win32(ERROR_NOT_FOUND.0) && hr != HRESULT::from_win32(ERROR_ALREADY_EXISTS.0) {
                        break;
                    }
                }
                if hr.is_ok() {
                    CONECTADO.store(true, Ordering::Relaxed);
                    break;
                }
            }
        });
        TENTANDO.store(false, Ordering::Relaxed);
    });
}

// ---------- servidor COM ----------

#[implement(IClassFactory)]
struct Fabrica;

impl IClassFactory_Impl for Fabrica_Impl {
    fn CreateInstance(&self, externo: Ref<'_, IUnknown>, iid: *const GUID, saida: *mut *mut c_void) -> windows::core::Result<()> {
        unsafe {
            *saida = null_mut();
            if externo.is_some() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            let tap: IObjectWithSite = Tap { site: Mutex::new(None) }.into();
            tap.query(iid, saida).ok()
        }
    }

    fn LockServer(&self, _: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

/// O XAML pede aqui o TAP do `CLSID_TAP`.
///
/// # Safety
/// Ponteiros válidos, como em qualquer DllGetClassObject.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const GUID, iid: *const GUID, saida: *mut *mut c_void) -> HRESULT {
    if saida.is_null() || clsid.is_null() {
        return E_POINTER;
    }
    *saida = null_mut();
    if *clsid != CLSID_TAP {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    super::prender();
    let f: IClassFactory = Fabrica.into();
    f.query(iid, saida)
}

/// A DLL fica presa no processo (subclasses, ganchos, TAP).
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

// ---------- composição ----------

/// Pincéis de cor da composição do XAML do sistema (dcomp.dll). O XAML Diagnostics não avisa os
/// elementos das ilhas XAML além da primeira: no Gerenciador de Tarefas, o cabeçalho, a busca e
/// o corpo das páginas são ilhas à parte, com pincéis próprios fora dos dicionários (fundo
/// #191919, texto branco). Toda cor de pincel do XAML passa pelo CompositionColorBrush na hora
/// de desenhar; ali os cinzas do modo escuro e o branco viram as cores do tema. As funções não
/// são exportadas: os endereços saem da vtable de um Compositor e de um pincel criados aqui.
mod composicao {
    use std::ffi::c_void;
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Once;

    use retour::RawDetour;
    use windows::core::{s, w, GUID, HRESULT, PCSTR, PCWSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

    use super::super::{seguro, CORES, ESCURO, FUNDO, FUNDO_CLARO, MODO, TEXTO};

    static PUT_COLOR: AtomicUsize = AtomicUsize::new(0);
    static COM_COR: AtomicUsize = AtomicUsize::new(0);

    const ICOMPOSITOR: GUID = GUID::from_u128(0xb403ca50_7f8c_4e83_985f_cc45060036d8);

    #[repr(C)]
    struct Opcoes {
        tamanho: u32,
        tipo_linha: i32,
        apartamento: i32,
    }

    /// Windows.UI.Color por valor: A, R, G e B do byte de baixo para o de cima.
    fn trocar(c: u32) -> u32 {
        if MODO.load(Ordering::Relaxed) != ESCURO {
            return c;
        }
        let (a, r, g, b) = (c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF, c >> 24);
        let papel = match (r, g, b) {
            (0x19, 0x19, 0x19) | (0x1C, 0x1C, 0x1C) | (0x20, 0x20, 0x20) if a == 0xFF => FUNDO,
            (0x2B, 0x2B, 0x2B) | (0x2C, 0x2C, 0x2C) if a == 0xFF => FUNDO_CLARO,
            (0xFF, 0xFF, 0xFF) => TEXTO,
            _ => return c,
        };
        let t = CORES[papel].load(Ordering::Relaxed);
        a | ((t >> 16) & 0xFF) << 8 | ((t >> 8) & 0xFF) << 16 | (t & 0xFF) << 24
    }

    unsafe extern "system" fn put_color(pincel: *mut c_void, c: u32) -> HRESULT {
        let f: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT = std::mem::transmute(PUT_COLOR.load(Ordering::Acquire));
        f(pincel, seguro(c, || trocar(c)))
    }

    unsafe extern "system" fn com_cor(compositor: *mut c_void, c: u32, saida: *mut *mut c_void) -> HRESULT {
        let f: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT = std::mem::transmute(COM_COR.load(Ordering::Acquire));
        f(compositor, seguro(c, || trocar(c)), saida)
    }

    unsafe fn desviar(alvo: usize, novo: *const (), o: &AtomicUsize) {
        let Ok(d) = RawDetour::new(alvo as *const (), novo) else { return };
        o.store(d.trampoline() as *const () as usize, Ordering::Release);
        if d.enable().is_ok() {
            std::mem::forget(d); // fica até o programa fechar (a DLL está presa)
        } else {
            o.store(0, Ordering::Release);
        }
    }

    /// Uma vez por processo, numa linha de execução própria (o Compositor precisa de uma fila
    /// do dispatcher na linha que o cria).
    pub fn instalar() {
        static UMA_VEZ: Once = Once::new();
        UMA_VEZ.call_once(|| unsafe {
            let simbolo = |dll: PCWSTR, nome: PCSTR| LoadLibraryExW(dll, None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok().and_then(|m| GetProcAddress(m, nome));
            let (Some(fila), Some(ativar), Some(texto)) = (
                simbolo(w!("CoreMessaging.dll"), s!("CreateDispatcherQueueController")),
                simbolo(w!("combase.dll"), s!("RoActivateInstance")),
                simbolo(w!("combase.dll"), s!("WindowsCreateString")),
            ) else {
                return;
            };
            let fila: unsafe extern "system" fn(Opcoes, *mut *mut c_void) -> HRESULT = std::mem::transmute(fila);
            let ativar: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT = std::mem::transmute(ativar);
            let texto: unsafe extern "system" fn(PCWSTR, u32, *mut *mut c_void) -> HRESULT = std::mem::transmute(texto);
            let vt = |o: *mut c_void, i: usize| *(*(o as *const *const usize)).add(i);
            let liberar = |o: *mut c_void| {
                let f: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(vt(o, 2));
                f(o);
            };
            // fila do dispatcher nesta linha (DQTYPE_THREAD_CURRENT, sem apartamento COM)
            let mut controlador = null_mut();
            if fila(Opcoes { tamanho: 12, tipo_linha: 2, apartamento: 0 }, &mut controlador).is_err() {
                return;
            }
            let nome = w!("Windows.UI.Composition.Compositor");
            let (mut classe, mut objeto) = (null_mut(), null_mut());
            if texto(nome, nome.len() as u32, &mut classe).is_err() || ativar(classe, &mut objeto).is_err() || objeto.is_null() {
                return;
            }
            let consultar: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT = std::mem::transmute(vt(objeto, 0));
            let mut compositor = null_mut();
            if consultar(objeto, &ICOMPOSITOR, &mut compositor).is_ok() && !compositor.is_null() {
                // ICompositor: 7 CreateColorBrush, 8 CreateColorBrushWithColor
                let criar: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT = std::mem::transmute(vt(compositor, 7));
                let mut pincel = null_mut();
                if criar(compositor, &mut pincel).is_ok() && !pincel.is_null() {
                    // ICompositionColorBrush: 6 get_Color, 7 put_Color
                    desviar(vt(pincel, 7), put_color as *const (), &PUT_COLOR);
                    liberar(pincel);
                }
                desviar(vt(compositor, 8), com_cor as *const (), &COM_COR);
                liberar(compositor);
            }
            liberar(objeto);
            liberar(controlador);
        });
    }
}
