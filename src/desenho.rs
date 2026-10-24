//! Desenho da barra, da dica e da visão geral com Direct2D, num DC (o da janela ou um bitmap,
//! no modo --preview). Visual do Omarchy: faixa fina em cima, texto e glifos em CaskaydiaMono
//! Nerd Font, cores do tema escolhido, cantos retos e bordas de 2 px como as do Hyprland.

use std::collections::HashMap;

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::WindowsAndMessaging::HICON;

use crate::tema::Tema;

mod central;
pub use central::CenaCentral;

const FONTE: PCWSTR = w!("CaskaydiaMono Nerd Font");

// glifos da Nerd Font
const G_MAIS: &str = "\u{f067}";
/// Programa sem ícone no painel de volume.
const G_PROGRAMA: &str = "\u{f08c6}";
pub const G_TERMINAL: &str = "\u{f120}";
pub const G_TECLADO: &str = "\u{f11c}";
/// Grupo ativo, como o workspace ativo da Waybar do Omarchy.
const G_GRUPO_ATIVO: &str = "\u{f14fb}";
const G_MARCA: &str = "\u{f00c}";
const G_SETA: &str = "\u{f105}";

/// Item do menu de contexto, como o Windows pede para desenhar.
pub struct ItemMenu<'a> {
    pub texto: &'a str,
    /// Tecla de atalho, alinhada à direita.
    pub atalho: &'a str,
    pub separador: bool,
    pub submenu: bool,
    pub marcado: bool,
    pub habilitado: bool,
    pub selecionado: bool,
}

/// O que fica embaixo do cursor.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Alvo {
    Bandeja(usize),
    Grupo(usize),
    NovoGrupo,
    Janela(isize),
    Rede,
    Volume,
    Relogio,
}

pub struct ItemGrupo {
    pub nome: String,
    pub qtd: usize,
    pub ativo: bool,
    /// Renomeado por quem usa: mostra o nome mesmo ativo (sem nome, o ativo vira o glifo).
    pub proprio: bool,
}

pub struct ItemJanela {
    pub hwnd: isize,
    /// Índice do grupo: as janelas vêm agrupadas, com um separador entre um grupo e outro.
    pub grupo: usize,
    pub icone: HICON,
    pub ativa: bool,
    pub minimizada: bool,
    pub piscando: bool,
    /// Selo do ITaskbarList3 (pode ser nulo).
    pub selo: HICON,
    /// (estado TBPF, fração 0..1) quando o programa mostra progresso.
    pub progresso: Option<(u32, f32)>,
    /// Glifo no lugar do ícone (terminal, Gerenciador de Tarefas…).
    pub glifo: Option<&'static str>,
}

pub struct ItemBandeja {
    pub indice: usize,
    pub icone: HICON,
    pub glifo: Option<&'static str>,
}

pub struct Cena {
    pub w: i32,
    pub h: i32,
    pub escala: f32,
    pub grupos: Vec<ItemGrupo>,
    pub janelas: Vec<ItemJanela>,
    pub hover: Option<Alvo>,
    /// "terça-feira 20:25", no centro
    pub hora: String,
    pub bandeja: Vec<ItemBandeja>,
    /// Glifo da rede e se tem conexão.
    pub rede: (&'static str, bool),
    /// (nível 0..1, mudo); None sem saída de som.
    pub volume: Option<(f32, bool)>,
    /// Rede ou volume com o painel aberto, ou o relógio com a central aberta.
    pub aberto: Option<Alvo>,
    /// Compromisso que começa em até 30 minutos: (texto, se faltam 5 minutos ou menos).
    pub aviso: Option<(String, bool)>,
    /// Execução de hoje com falha ou sessão do Claude aguardando permissão.
    pub alerta: bool,
}

/// O que fica embaixo do cursor no painel de rede ou de volume.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AlvoPainel {
    Mudo,
    Nivel,
    Saida(usize),
    Programa(usize),
    Radio,
    Bluetooth(usize),
    Link(usize),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tom {
    Normal,
    Destaque,
    Apagado,
    Amarelo,
}

/// Linhas do painel, de cima para baixo.
pub enum LinhaPainel {
    /// Nome do painel e, à direita, o nível.
    Titulo { texto: String, dir: String },
    /// Volume geral: glifo (clique liga o mudo), trilho e número.
    Nivel { glifo: &'static str, mudo: bool, nivel: f32 },
    /// Rótulo de seção com linha em cima; `chave` desenha o interruptor do Bluetooth.
    Rotulo { texto: String, chave: Option<bool> },
    Item { glifo: &'static str, texto: String, dir: String, tom: Tom, marcado: bool, sel: bool, alvo: Option<AlvoPainel>, separado: bool },
    Programa { icone: HICON, nome: String, nivel: f32, mudo: bool, alvo: AlvoPainel },
    Espaco(f32),
}

pub struct CenaPainel {
    pub w: i32,
    pub escala: f32,
    pub linhas: Vec<LinhaPainel>,
    pub hover: Option<AlvoPainel>,
}

pub type AlvosPainel = Vec<(D2D_RECT_F, AlvoPainel)>;

impl LinhaPainel {
    fn altura(&self) -> f32 {
        match self {
            LinhaPainel::Titulo { .. } | LinhaPainel::Nivel { .. } => 32.0,
            LinhaPainel::Rotulo { chave: Some(_), .. } => 30.0,
            LinhaPainel::Rotulo { .. } => 26.0,
            LinhaPainel::Item { .. } => 26.0,
            LinhaPainel::Programa { .. } => 22.0,
            LinhaPainel::Espaco(e) => *e,
        }
    }
}

/// Altura do painel com estas linhas (com a borda de baixo).
pub fn altura_painel(linhas: &[LinhaPainel], s: f32) -> i32 {
    ((linhas.iter().map(|l| l.altura()).sum::<f32>() + 1.0) * s).ceil() as i32
}

/// Visão geral: um quadro por grupo, com as janelas no lugar delas. As miniaturas são do
/// DWM e ficam por cima deste desenho; aqui vão fundo, nomes, bordas e as minimizadas.
pub struct CartaoV {
    pub r: D2D_RECT_F,
    pub cab: D2D_RECT_F,
    pub area: D2D_RECT_F,
    pub nome: String,
    pub qtd: usize,
    pub ativo: bool,
    /// Cartão "novo grupo".
    pub novo: bool,
    pub hover: bool,
    /// Janela sendo arrastada por cima.
    pub destino: bool,
    /// Escolhido pelo teclado.
    pub selecionado: bool,
}

pub struct JanelaV {
    pub r: D2D_RECT_F,
    pub titulo: String,
    pub icone: HICON,
    pub minimizada: bool,
    pub selecionada: bool,
    pub hover: bool,
    pub apagada: bool,
    /// Alt+Tab: faixa do nome embaixo do quadro, e o grupo da janela.
    pub rotulo: Option<D2D_RECT_F>,
    pub grupo: String,
}

pub struct CenaVisao {
    pub w: i32,
    pub h: i32,
    pub escala: f32,
    pub cartoes: Vec<CartaoV>,
    pub janelas: Vec<JanelaV>,
    pub filtro: String,
    pub titulo: String,
    /// Aberta pelo Alt+Tab (quadros lado a lado).
    pub lista: bool,
}

pub type Alvos = Vec<(D2D_RECT_F, Alvo)>;

fn cor(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a,
    }
}

/// `a` levado `f` (0..1) na direção de `b`.
fn mistura(a: u32, b: u32, f: f32) -> u32 {
    let canal = |s: u32| {
        let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
        ((x + (y - x) * f).round() as u32) << s
    };
    canal(16) | canal(8) | canal(0)
}

fn ret(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F { left, top, right, bottom }
}

pub struct Gfx {
    d2d: ID2D1Factory,
    dw: IDWriteFactory,
    wic: IWICImagingFactory,
    /// Alvo em uso: o do DC (barra, painéis) ou, durante a pintura dela, o da visão geral.
    rt: ID2D1RenderTarget,
    dc_rt: ID2D1DCRenderTarget,
    /// Ícones já convertidos para o alvo em uso (cada alvo tem os seus).
    icones: HashMap<isize, ID2D1Bitmap>,
    visao: Option<AlvoJanela>,
    pub tema: &'static Tema,
}

/// Alvo direto na janela da visão geral: desenha na GPU e apresenta sem esperar a tela. O do DC
/// leva 20–30 ms para a tela inteira (desenha e copia para o GDI); este, poucos ms.
struct AlvoJanela {
    janela: HWND,
    rt: ID2D1HwndRenderTarget,
    icones: HashMap<isize, ID2D1Bitmap>,
}

impl Gfx {
    pub fn new(tema: &'static Tema) -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let dc_rt = Self::alvo(&d2d)?;
            Ok(Self { d2d, dw, wic, rt: dc_rt.clone().into(), dc_rt, icones: HashMap::new(), visao: None, tema })
        }
    }

    fn alvo(d2d: &ID2D1Factory) -> Result<ID2D1DCRenderTarget> {
        unsafe {
            let rt = d2d.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            })?;
            // tons de cinza, como no Omarchy: o ClearType deixa franja colorida nas letras
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(rt)
        }
    }

    fn pincel(&self, c: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush> {
        unsafe { self.rt.CreateSolidColorBrush(&c, None) }
    }

    fn formato(&self, tam: f32, peso: DWRITE_FONT_WEIGHT, alinhar: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
        unsafe {
            let f = self.dw.CreateTextFormat(FONTE, None, peso, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, tam, w!("pt-br"))?;
            f.SetTextAlignment(alinhar)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            // título comprido termina em "…"
            let reticencias = self.dw.CreateEllipsisTrimmingSign(&f)?;
            let corte = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
            f.SetTrimming(&corte, &reticencias)?;
            Ok(f)
        }
    }

    fn texto(&self, s: &str, f: &IDWriteTextFormat, r: D2D_RECT_F, c: D2D1_COLOR_F) -> Result<()> {
        let u: Vec<u16> = s.encode_utf16().collect();
        let b = self.pincel(c)?;
        unsafe { self.rt.DrawText(&u, f, &r, &b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL) };
        Ok(())
    }

    /// Glifo da Nerd Font (mesmo desenho do texto, separado para ler melhor onde é ícone).
    fn glifo(&self, s: &str, f: &IDWriteTextFormat, r: D2D_RECT_F, c: D2D1_COLOR_F) -> Result<()> {
        self.texto(s, f, r, c)
    }

    fn largura(&self, s: &str, f: &IDWriteTextFormat) -> Result<f32> {
        let u: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            let l = self.dw.CreateTextLayout(&u, f, 4000.0, 100.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            l.GetMetrics(&mut m)?;
            Ok(m.widthIncludingTrailingWhitespace)
        }
    }

    /// Limita o desenho a `r` até o `soltar` correspondente.
    fn recortar(&self, r: D2D_RECT_F) {
        unsafe { self.rt.PushAxisAlignedClip(&r, D2D1_ANTIALIAS_MODE_ALIASED) };
    }

    fn soltar(&self) {
        unsafe { self.rt.PopAxisAlignedClip() };
    }

    fn preencher(&self, r: D2D_RECT_F, c: D2D1_COLOR_F) -> Result<()> {
        unsafe { self.rt.FillRectangle(&r, &self.pincel(c)?) };
        Ok(())
    }

    /// Borda de `e` px por dentro de `r`.
    fn borda(&self, r: D2D_RECT_F, c: D2D1_COLOR_F, e: f32) -> Result<()> {
        let m = e / 2.0;
        unsafe { self.rt.DrawRectangle(&ret(r.left + m, r.top + m, r.right - m, r.bottom - m), &self.pincel(c)?, e, None) };
        Ok(())
    }

    /// Ícone já reduzido para `tam` px com filtro bicúbico de alta qualidade (o DrawBitmap só
    /// tem linear, que serrilha ao reduzir 32 → 16).
    fn bitmap(&mut self, icone: HICON, tam: u32) -> Option<ID2D1Bitmap> {
        if icone.is_invalid() || tam == 0 {
            return None;
        }
        let chave = ((icone.0 as isize) << 8) ^ tam as isize;
        if let Some(b) = self.icones.get(&chave) {
            return Some(b.clone());
        }
        if self.icones.len() > 600 {
            self.icones.clear();
        }
        let b = unsafe {
            let wb = self.wic.CreateBitmapFromHICON(icone).ok()?;
            let (mut w, mut h) = (0u32, 0u32);
            wb.GetSize(&mut w, &mut h).ok()?;
            let fonte: IWICBitmapSource = if w != tam || h != tam {
                let esc = self.wic.CreateBitmapScaler().ok()?;
                esc.Initialize(&wb, tam, tam, WICBitmapInterpolationModeHighQualityCubic).ok()?;
                esc.into()
            } else {
                wb.into()
            };
            let conv = self.wic.CreateFormatConverter().ok()?;
            conv.Initialize(&fonte, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
            self.rt.CreateBitmapFromWicBitmap(&conv, None).ok()?
        };
        self.icones.insert(chave, b.clone());
        Some(b)
    }

    /// Desenha o ícone no tamanho exato do retângulo, alinhado ao pixel.
    fn icone(&mut self, ic: HICON, r: D2D_RECT_F, opac: f32) {
        let lado = (r.right - r.left).round().max(1.0);
        let (x, y) = (r.left.round(), r.top.round());
        if let Some(b) = self.bitmap(ic, lado as u32) {
            let alvo = ret(x, y, x + lado, y + lado);
            unsafe { self.rt.DrawBitmap(&b, Some(&alvo), opac, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None) };
        }
    }

    fn no_dc<R>(&mut self, dc: HDC, w: i32, h: i32, f: impl FnOnce(&mut Self) -> Result<R>) -> Result<R> {
        self.no_dc_em(dc, RECT { left: 0, top: 0, right: w, bottom: h }, f)
    }

    /// Desenha só no retângulo `r` do DC; dentro de `f` a origem é o canto de `r`.
    fn no_dc_em<R>(&mut self, dc: HDC, r: RECT, f: impl FnOnce(&mut Self) -> Result<R>) -> Result<R> {
        unsafe {
            self.dc_rt.BindDC(dc, &r)?;
            self.rt.BeginDraw();
        }
        let r = f(self);
        let fim = unsafe { self.rt.EndDraw(None, None) };
        if let Err(e) = fim {
            // dispositivo perdido: refaz o alvo e o cache de ícones na próxima pintura
            if e.code() == D2DERR_RECREATE_TARGET {
                self.dc_rt = Self::alvo(&self.d2d)?;
                self.rt = self.dc_rt.clone().into();
                self.icones.clear();
            }
            return Err(e);
        }
        r
    }

    /// Desenha a barra no DC e devolve onde ficou cada alvo clicável.
    pub fn desenhar(&mut self, dc: HDC, cena: &Cena) -> Result<Alvos> {
        self.no_dc(dc, cena.w, cena.h, |g| g.conteudo(cena))
    }

    fn conteudo(&mut self, c: &Cena) -> Result<Alvos> {
        let t = self.tema;
        let s = c.escala;
        let (w, h) = (c.w as f32, c.h as f32);
        let f_texto = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let f_glifo = self.formato(14.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let f_glifo_janela = self.formato(16.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let mut alvos: Alvos = Vec::new();
        let hover = |a: Alvo| c.hover == Some(a);
        let realce = cor(t.fundo_claro, 1.0);

        unsafe { self.rt.Clear(Some(&cor(t.fundo, 1.0))) };

        // grupos, como os workspaces da Waybar do Omarchy: o número, e o ativo vira o glifo
        // 󱓻 (grupo renomeado mostra o nome); vazio apagado
        let mut x = 6.0 * s;
        for (i, g) in c.grupos.iter().enumerate() {
            let rotulo = if g.ativo && !g.proprio { G_GRUPO_ATIVO } else { g.nome.as_str() };
            let larg = self.largura(rotulo, &f_texto)?.max(7.0 * s) + 14.0 * s;
            let r = ret(x, 0.0, x + larg, h);
            if hover(Alvo::Grupo(i)) && !g.ativo {
                self.preencher(r, realce)?;
            }
            let tinta = if g.ativo {
                cor(t.texto_forte, 1.0)
            } else if g.qtd == 0 {
                // apagado, mas legível (texto_escuro fica abaixo de 3:1 em vários temas)
                cor(mistura(t.texto, t.fundo, 0.3), 1.0)
            } else {
                cor(t.texto, 1.0)
            };
            self.texto(rotulo, &f_texto, r, tinta)?;
            alvos.push((r, Alvo::Grupo(i)));
            x = r.right;
        }
        let r = ret(x, 0.0, x + 24.0 * s, h);
        if hover(Alvo::NovoGrupo) {
            self.preencher(r, realce)?;
        }
        self.glifo(G_MAIS, &f_texto, r, cor(mistura(t.texto, t.fundo, 0.3), 1.0))?;
        alvos.push((r, Alvo::NovoGrupo));
        x = r.right + 14.0 * s;

        // janelas de todos os grupos, em ordem, com "|" entre um grupo e outro; a com foco
        // fica com o fundo de seleção
        let lado = 16.0 * s;
        let celula = 28.0 * s;
        let meio = h / 2.0;
        let mut anterior = None;
        for j in &c.janelas {
            if anterior.is_some_and(|g| g != j.grupo) {
                let sep = 12.0 * s;
                self.texto("|", &f_texto, ret(x, 0.0, x + sep, h), cor(mistura(t.texto, t.fundo, 0.3), 1.0))?;
                x += sep;
            }
            anterior = Some(j.grupo);
            let r = ret(x, 0.0, x + celula, h);
            if j.piscando {
                self.preencher(r, cor(t.amarelo, 0.28))?;
            } else if j.ativa {
                self.preencher(r, cor(t.selecao, 1.0))?;
            } else if hover(Alvo::Janela(j.hwnd)) {
                self.preencher(r, realce)?;
            }
            let cx = (r.left + r.right) / 2.0;
            let ri = ret(cx - lado / 2.0, meio - lado / 2.0, cx + lado / 2.0, meio + lado / 2.0);
            let apagado = if t.claro { 0.55 } else { 0.4 };
            match j.glifo {
                // glifo monocromático na cor do texto (minimizada, apagado)
                Some(g) => {
                    let tinta = if j.minimizada {
                        mistura(t.texto, t.fundo, 0.5)
                    } else if j.ativa {
                        t.texto_forte
                    } else {
                        t.texto
                    };
                    self.glifo(g, &f_glifo_janela, r, cor(tinta, 1.0))?
                }
                None => self.icone(j.icone, ri, if j.minimizada { apagado } else { 1.0 }),
            }
            if !j.selo.is_invalid() {
                let l = 9.0 * s;
                self.icone(j.selo, ret(ri.right - l * 0.6, ri.bottom - l * 0.7, ri.right + l * 0.4, ri.bottom + l * 0.3), 1.0);
            }
            if j.piscando {
                self.preencher(ret(r.left + 4.0 * s, h - 2.0 * s, r.right - 4.0 * s, h), cor(t.amarelo, 1.0))?;
            }
            // progresso (download, cópia): linha no alto da célula
            if let Some((estado, fracao)) = j.progresso {
                let tinta = match estado {
                    4 => t.vermelho,
                    8 => t.amarelo,
                    _ => t.verde,
                };
                let fr = if estado == 1 { 1.0 } else { fracao };
                let a = if estado == 1 { 0.5 } else { 1.0 };
                let (l, rr) = (r.left + 4.0 * s, r.right - 4.0 * s);
                self.preencher(ret(l, 0.0, rr, 2.0 * s), cor(t.selecao, 1.0))?;
                self.preencher(ret(l, 0.0, l + (rr - l) * fr, 2.0 * s), cor(tinta, a))?;
            }
            alvos.push((r, Alvo::Janela(j.hwnd)));
            x = r.right;
        }
        let fim_esquerda = x;

        // direita: rede e volume, e antes deles a bandeja dos outros programas
        let celula_sis = 28.0 * s;
        let rv = ret(w - 6.0 * s - celula_sis, 0.0, w - 6.0 * s, h);
        let rs = ret(rv.left - celula_sis, 0.0, rv.left, h);
        let (g_vol, vol_mudo) = match c.volume {
            Some((n, m)) => (crate::audio::glifo(n, m), m),
            None => (crate::audio::G_VOLUME_MUDO, true),
        };
        let sem_rede = mistura(t.texto, t.fundo, 0.3);
        for (r, alvo, g, tinta) in [
            (rs, Alvo::Rede, c.rede.0, if c.rede.1 { t.texto } else { sem_rede }),
            (rv, Alvo::Volume, g_vol, if vol_mudo { t.vermelho } else { t.texto }),
        ] {
            let aberto = c.aberto == Some(alvo);
            if aberto || hover(alvo) {
                self.preencher(r, realce)?;
            }
            let tinta = if aberto && tinta == t.texto { t.texto_forte } else { tinta };
            self.glifo(g, &f_glifo, r, cor(tinta, 1.0))?;
            alvos.push((r, alvo));
        }
        let mut xb = rs.left - 6.0 * s;
        for ic in c.bandeja.iter().rev() {
            let r = ret(xb - 24.0 * s, 0.0, xb, h);
            if hover(Alvo::Bandeja(ic.indice)) {
                self.preencher(r, realce)?;
            }
            let cx = (r.left + r.right) / 2.0;
            let l = 16.0 * s;
            match ic.glifo {
                Some(g) => self.glifo(g, &f_glifo, r, cor(t.texto, 1.0))?,
                None => self.icone(ic.icone, ret(cx - l / 2.0, meio - l / 2.0, cx + l / 2.0, meio + l / 2.0), 1.0),
            }
            alvos.push((r, Alvo::Bandeja(ic.indice)));
            xb = r.left;
        }

        // centro: dia e hora (some se as janelas chegarem até ele)
        // o relógio fica no centro; aviso e alerta vêm depois dele, dentro da mesma área de clique
        let lh = self.largura(&c.hora, &f_texto)? + 16.0 * s;
        let la = match &c.aviso {
            Some((a, _)) => self.largura(a, &f_texto)? + 10.0 * s,
            None => 0.0,
        };
        let lal = if c.alerta { 16.0 * s } else { 0.0 };
        let rh = ret(w / 2.0 - lh / 2.0, 0.0, w / 2.0 + lh / 2.0 + la + lal, h);
        if rh.left > fim_esquerda && rh.right < xb {
            let aberto = c.aberto == Some(Alvo::Relogio);
            if aberto || hover(Alvo::Relogio) {
                self.preencher(rh, realce)?;
            }
            let rr = ret(rh.left, 0.0, rh.left + lh, h);
            self.texto(&c.hora, &f_texto, rr, cor(if aberto { t.texto_forte } else { t.texto }, 1.0))?;
            if let Some((a, perto)) = &c.aviso {
                let tinta = if *perto { t.amarelo } else { mistura(t.texto, t.fundo, 0.3) };
                self.texto(a, &f_texto, ret(rr.right + 2.0 * s, 0.0, rr.right + la, h), cor(tinta, 1.0))?;
            }
            if c.alerta {
                let x = rh.right - lal + 4.0 * s;
                let q = 6.0 * s;
                self.preencher(ret(x, meio - q / 2.0, x + q, meio + q / 2.0), cor(t.vermelho, 1.0))?;
            }
            alvos.push((rh, Alvo::Relogio));
        }
        Ok(alvos)
    }

    /// Painel de rede ou de volume, preso embaixo do ícone: linhas de 26 px como as do menu,
    /// trilhos retos em `destaque`, borda de 1 px em `mudo` nos lados e embaixo.
    pub fn desenhar_painel(&mut self, dc: HDC, h: i32, c: &CenaPainel) -> Result<AlvosPainel> {
        self.no_dc(dc, c.w, h, |g| g.conteudo_painel(c, h as f32))
    }

    fn conteudo_painel(&mut self, c: &CenaPainel, h: f32) -> Result<AlvosPainel> {
        let t = self.tema;
        let s = c.escala;
        let w = c.w as f32;
        let f_texto = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let f_forte = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let f_dir = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let f_glifo = self.formato(14.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let f_glifo_nivel = self.formato(15.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let apagado = mistura(t.texto, t.fundo, 0.3);
        let rotulo = mistura(t.texto, t.fundo, 0.45);
        let hover = |a: AlvoPainel| c.hover == Some(a);
        let pad = 12.0 * s;
        let mut alvos: AlvosPainel = Vec::new();

        unsafe { self.rt.Clear(Some(&cor(t.fundo, 1.0))) };

        let mut y = 0.0;
        for l in &c.linhas {
            let a = l.altura() * s;
            let fim = y + a;
            match l {
                LinhaPainel::Titulo { texto, dir } => {
                    let r = ret(pad, y + 2.0 * s, w - pad, fim);
                    self.texto(texto, &f_forte, r, cor(t.texto_forte, 1.0))?;
                    self.texto(dir, &f_dir, r, cor(apagado, 1.0))?;
                }
                LinhaPainel::Nivel { glifo, mudo, nivel } => {
                    let meio = y + 13.0 * s;
                    let (topo, base) = (meio - 13.0 * s, meio + 13.0 * s);
                    let ri = ret(pad - 4.0 * s, topo, pad + 22.0 * s, base);
                    if hover(AlvoPainel::Mudo) {
                        self.preencher(ri, cor(t.fundo_claro, 1.0))?;
                    }
                    let tinta = if *mudo { t.vermelho } else { t.texto };
                    self.glifo(glifo, &f_glifo_nivel, ri, cor(tinta, 1.0))?;
                    alvos.push((ri, AlvoPainel::Mudo));
                    let (l0, l1) = (40.0 * s, w - pad - 44.0 * s);
                    let e = 3.0 * s;
                    self.preencher(ret(l0, meio - e, l1, meio + e), cor(t.mudo, 1.0))?;
                    let cheio = if *mudo { t.texto_escuro } else if hover(AlvoPainel::Nivel) { t.texto_forte } else { t.destaque };
                    self.preencher(ret(l0, meio - e, l0 + (l1 - l0) * nivel.clamp(0.0, 1.0), meio + e), cor(cheio, 1.0))?;
                    alvos.push((ret(l0, topo, l1, base), AlvoPainel::Nivel));
                    let num = if *mudo { "Mudo".to_string() } else { format!("{}%", (nivel * 100.0).round() as i32) };
                    self.texto(&num, &f_dir, ret(l1, topo, w - pad, base), cor(t.texto_forte, 1.0))?;
                }
                LinhaPainel::Rotulo { texto, chave } => {
                    self.preencher(ret(0.0, y, w, y + 1.0 * s), cor(t.selecao, 1.0))?;
                    let r = ret(0.0, y + 1.0 * s, w, fim);
                    if let Some(ligado) = chave {
                        if hover(AlvoPainel::Radio) {
                            self.preencher(r, cor(t.fundo_claro, 1.0))?;
                        }
                        let meio = (r.top + r.bottom) / 2.0;
                        let (cl, cr) = (w - pad - 26.0 * s, w - pad);
                        let caixa = ret(cl, meio - 6.0 * s, cr, meio + 6.0 * s);
                        let q = 8.0 * s;
                        if *ligado {
                            self.preencher(caixa, cor(t.destaque, 1.0))?;
                            self.preencher(ret(cr - 2.0 * s - q, meio - q / 2.0, cr - 2.0 * s, meio + q / 2.0), cor(t.fundo, 1.0))?;
                        } else {
                            self.preencher(caixa, cor(t.mudo, 1.0))?;
                            self.preencher(ret(cl + 2.0 * s, meio - q / 2.0, cl + 2.0 * s + q, meio + q / 2.0), cor(apagado, 1.0))?;
                        }
                        alvos.push((r, AlvoPainel::Radio));
                        self.texto(texto, &f_texto, ret(pad, r.top, cl - 8.0 * s, r.bottom), cor(t.texto, 1.0))?;
                    } else {
                        self.texto(texto, &f_texto, ret(pad, y + 6.0 * s, w - pad, fim), cor(rotulo, 1.0))?;
                    }
                }
                LinhaPainel::Item { glifo, texto, dir, tom, marcado, sel, alvo, separado } => {
                    let mut r = ret(0.0, y, w, fim);
                    if *separado {
                        self.preencher(ret(0.0, y, w, y + 1.0 * s), cor(t.selecao, 1.0))?;
                        r.top += 1.0 * s;
                    }
                    if let Some(al) = alvo {
                        if hover(*al) {
                            self.preencher(r, cor(t.fundo_claro, 1.0))?;
                        }
                        alvos.push((r, *al));
                    }
                    let link = matches!(alvo, Some(AlvoPainel::Link(_)));
                    let (tinta_g, tinta) = if *sel {
                        (t.destaque, t.texto_forte)
                    } else if link {
                        (t.texto_escuro, apagado)
                    } else {
                        (apagado, t.texto)
                    };
                    self.glifo(glifo, &f_glifo, ret(pad - 4.0 * s, r.top, pad + 22.0 * s, r.bottom), cor(tinta_g, 1.0))?;
                    let mut fim_texto = w - pad;
                    if *marcado {
                        self.glifo(G_MARCA, &f_glifo, ret(w - pad - 18.0 * s, r.top, w - pad, r.bottom), cor(t.destaque, 1.0))?;
                        fim_texto -= 24.0 * s;
                    }
                    if !dir.is_empty() {
                        let ld = self.largura(dir, &f_dir)?;
                        let tom = match tom {
                            Tom::Normal => t.texto,
                            Tom::Destaque => t.destaque,
                            Tom::Apagado => apagado,
                            Tom::Amarelo => t.amarelo,
                        };
                        self.texto(dir, &f_dir, ret(fim_texto - ld - 2.0 * s, r.top, fim_texto, r.bottom), cor(tom, 1.0))?;
                        fim_texto -= ld + 10.0 * s;
                    }
                    self.texto(texto, &f_texto, ret(40.0 * s, r.top, fim_texto, r.bottom), cor(tinta, 1.0))?;
                }
                LinhaPainel::Programa { icone, nome, nivel, mudo, alvo } => {
                    let meio = (y + fim) / 2.0;
                    let lado = 14.0 * s;
                    let ri = ret(pad + 9.0 * s - lado / 2.0, meio - lado / 2.0, pad + 9.0 * s + lado / 2.0, meio + lado / 2.0);
                    if icone.is_invalid() {
                        self.glifo(G_PROGRAMA, &f_glifo, ret(pad - 4.0 * s, y, pad + 22.0 * s, fim), cor(apagado, 1.0))?;
                    } else {
                        self.icone(*icone, ri, 1.0);
                    }
                    self.texto(nome, &f_texto, ret(40.0 * s, y, 112.0 * s, fim), cor(apagado, 1.0))?;
                    let (l0, l1) = (120.0 * s, w - pad);
                    let e = 2.0 * s;
                    self.preencher(ret(l0, meio - e, l1, meio + e), cor(t.mudo, 1.0))?;
                    let cheio = if *mudo { t.texto_escuro } else if hover(*alvo) { t.texto_forte } else { t.destaque };
                    self.preencher(ret(l0, meio - e, l0 + (l1 - l0) * nivel.clamp(0.0, 1.0), meio + e), cor(cheio, 1.0))?;
                    alvos.push((ret(l0, y, l1, fim), *alvo));
                }
                LinhaPainel::Espaco(_) => {}
            }
            y = fim;
        }

        // borda: lados e embaixo (em cima fica a barra)
        let b = cor(t.mudo, 1.0);
        let e = 1.0 * s;
        self.preencher(ret(0.0, 0.0, e, h), b)?;
        self.preencher(ret(w - e, 0.0, w, h), b)?;
        self.preencher(ret(0.0, h - e, w, h), b)?;
        Ok(alvos)
    }

    /// Tamanho da dica (tooltip) para o texto.
    pub fn medir_dica(&self, texto: &str, s: f32) -> (i32, i32) {
        let Ok(f) = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER) else {
            return (0, 0);
        };
        let l = self.largura(texto, &f).unwrap_or(0.0).min(640.0 * s);
        ((l + 22.0 * s).ceil() as i32, (26.0 * s).ceil() as i32)
    }

    pub fn desenhar_dica(&mut self, dc: HDC, w: i32, h: i32, texto: &str, s: f32) -> Result<()> {
        self.no_dc(dc, w, h, |g| {
            let t = g.tema;
            unsafe { g.rt.Clear(Some(&cor(t.fundo, 1.0))) };
            g.borda(ret(0.0, 0.0, w as f32, h as f32), cor(t.destaque, 1.0), 2.0 * s)?;
            let f = g.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
            g.texto(texto, &f, ret(8.0 * s, 0.0, w as f32 - 8.0 * s, h as f32), cor(t.texto, 1.0))
        })
    }

    /// Tamanho (largura, altura) de um item do menu de contexto. O Windows ainda soma a
    /// coluna da marca à largura; o desenho usa o retângulo que ele entregar.
    pub fn medir_menu(&self, it: &ItemMenu, s: f32) -> (u32, u32) {
        if it.separador {
            return (1, (9.0 * s).round() as u32);
        }
        let Ok(f) = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING) else {
            return ((200.0 * s) as u32, (26.0 * s) as u32);
        };
        let mut l = 28.0 * s + self.largura(it.texto, &f).unwrap_or(0.0) + 24.0 * s;
        if !it.atalho.is_empty() {
            l += 32.0 * s + self.largura(it.atalho, &f).unwrap_or(0.0);
        }
        (l.ceil() as u32, (26.0 * s).round() as u32)
    }

    /// Item do menu de contexto no tema: fundo `fundo`, selecionado em `selecao`, marca e
    /// seta como glifos da Nerd Font, separador em linha `mudo`.
    pub fn desenhar_menu(&mut self, dc: HDC, r: RECT, it: &ItemMenu, s: f32) -> Result<()> {
        let (w, h) = ((r.right - r.left) as f32, (r.bottom - r.top) as f32);
        self.no_dc_em(dc, r, |g| {
            let t = g.tema;
            let fundo = if it.selecionado && it.habilitado { t.selecao } else { t.fundo };
            unsafe { g.rt.Clear(Some(&cor(fundo, 1.0))) };
            if it.separador {
                let y = (h / 2.0).floor();
                return g.preencher(ret(10.0 * s, y, w - 10.0 * s, y + 1.0), cor(t.mudo, 1.0));
            }
            let apagado = cor(mistura(t.texto, t.fundo, 0.3), 1.0);
            let tinta = if !it.habilitado {
                cor(t.mudo, 1.0)
            } else if it.selecionado {
                cor(t.texto_forte, 1.0)
            } else {
                cor(t.texto, 1.0)
            };
            let peso = if it.marcado { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
            let f = g.formato(12.0 * s, peso, DWRITE_TEXT_ALIGNMENT_LEADING)?;
            let f_dir = g.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
            let f_glifo = g.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
            if it.marcado {
                g.glifo(G_MARCA, &f_glifo, ret(0.0, 0.0, 28.0 * s, h), cor(t.destaque, 1.0))?;
            }
            let fim = w - 24.0 * s;
            g.texto(it.texto, &f, ret(28.0 * s, 0.0, fim, h), tinta)?;
            if !it.atalho.is_empty() {
                g.texto(it.atalho, &f_dir, ret(28.0 * s, 0.0, fim, h), if it.selecionado { cor(t.texto, 1.0) } else { apagado })?;
            }
            if it.submenu {
                g.glifo(G_SETA, &f_glifo, ret(fim, 0.0, w - 4.0 * s, h), if it.selecionado { cor(t.texto_forte, 1.0) } else { apagado })?;
            }
            Ok(())
        })
    }

    pub fn desenhar_visao(&mut self, janela: HWND, c: &CenaVisao) -> Result<()> {
        let tam = D2D_SIZE_U { width: c.w.max(1) as u32, height: c.h.max(1) as u32 };
        if self.visao.as_ref().is_none_or(|v| v.janela != janela) {
            let rt = unsafe {
                self.d2d.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                        dpiX: 96.0,
                        dpiY: 96.0,
                        ..Default::default()
                    },
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd: janela, pixelSize: tam, presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY },
                )?
            };
            unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
            self.visao = Some(AlvoJanela { janela, rt, icones: HashMap::new() });
        }
        let Some(v) = self.visao.as_mut() else { return Ok(()) };
        let atual = unsafe { v.rt.GetPixelSize() };
        if (atual.width, atual.height) != (tam.width, tam.height) {
            unsafe { v.rt.Resize(&tam)? };
        }
        let dc = std::mem::replace(&mut self.rt, v.rt.clone().into());
        std::mem::swap(&mut self.icones, &mut v.icones);
        unsafe { self.rt.BeginDraw() };
        let r = self.conteudo_visao(c);
        let fim = unsafe { self.rt.EndDraw(None, None) };
        self.rt = dc;
        if let Some(v) = self.visao.as_mut() {
            std::mem::swap(&mut self.icones, &mut v.icones);
        }
        if let Err(e) = fim {
            // dispositivo perdido: refaz o alvo na próxima pintura
            if e.code() == D2DERR_RECREATE_TARGET {
                self.visao = None;
            }
            return Err(e);
        }
        r
    }

    fn conteudo_visao(&mut self, c: &CenaVisao) -> Result<()> {
        let t = self.tema;
        let s = c.escala;
        let w = c.w as f32;
        let f_titulo = self.formato(15.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let f_dica = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let f_nome = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let f_qtd = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let f_min = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let f_novo = self.formato(13.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;

        unsafe { self.rt.Clear(Some(&cor(t.fundo, 1.0))) };

        // topo: a janela escolhida e, embaixo, como usar ou o filtro
        self.texto(&c.titulo, &f_titulo, ret(40.0 * s, 12.0 * s, w - 40.0 * s, 38.0 * s), cor(t.texto_forte, 1.0))?;
        let (dica, tinta) = if c.filtro.is_empty() && c.lista {
            ("Tab: próxima janela  ·  Shift+Tab: janela anterior  ·  Setas: navegar  ·  Solte Alt para abrir  ·  Esc: cancelar".to_string(), t.texto)
        } else if c.filtro.is_empty() {
            ("Setas: selecionar grupo  ·  Tab: selecionar janela  ·  Enter: abrir  ·  1–9: ir para o grupo  ·  +: criar grupo  ·  −: excluir grupo  ·  Shift+←/→: mover janela  ·  F2: renomear  ·  Digite para filtrar  ·  Esc: fechar".to_string(), t.texto)
        } else {
            (format!("Filtro: {}", c.filtro), t.destaque)
        };
        self.texto(&dica, &f_dica, ret(40.0 * s, 38.0 * s, w - 40.0 * s, 58.0 * s), cor(tinta, 1.0))?;

        let e = 2.0 * s;
        for k in &c.cartoes {
            let borda = if k.destino {
                t.amarelo
            } else if k.selecionado {
                t.texto_forte
            } else if k.ativo {
                t.destaque
            } else if k.hover {
                t.texto_escuro
            } else {
                t.mudo
            };
            if k.novo {
                self.borda(k.r, cor(borda, 1.0), e)?;
                let tinta = cor(if k.destino { t.amarelo } else { t.texto }, 1.0);
                self.glifo(&format!("{G_MAIS}  Criar grupo"), &f_novo, k.r, tinta)?;
                continue;
            }
            self.preencher(k.area, cor(t.fundo_claro, 1.0))?;
            self.borda(k.r, cor(borda, 1.0), e)?;
            let pad = 12.0 * s;
            let nome = if k.ativo { t.destaque } else { t.texto };
            self.texto(&k.nome, &f_nome, ret(k.cab.left + pad, k.cab.top, k.cab.right - 110.0 * s, k.cab.bottom), cor(nome, 1.0))?;
            let qtd = match k.qtd {
                0 => "Nenhuma janela".to_string(),
                1 => "1 janela".to_string(),
                n => format!("{n} janelas"),
            };
            self.texto(&qtd, &f_qtd, ret(k.cab.right - 140.0 * s, k.cab.top, k.cab.right - pad, k.cab.bottom), cor(t.texto, 1.0))?;
        }

        for j in &c.janelas {
            let a = if j.apagada { 0.3 } else { 1.0 };
            if let Some(rot) = j.rotulo {
                // Alt+Tab: quadro inteiro; minimizada mostra o ícone grande no meio
                if j.minimizada {
                    self.preencher(j.r, cor(t.fundo_claro, a))?;
                    let lado = 48.0 * s;
                    let (cx, cy) = ((j.r.left + j.r.right) / 2.0, (j.r.top + j.r.bottom) / 2.0);
                    self.icone(j.icone, ret(cx - lado / 2.0, cy - lado / 2.0, cx + lado / 2.0, cy + lado / 2.0), 0.7 * a);
                }
                let lado = 16.0 * s;
                let cy = (rot.top + rot.bottom) / 2.0 + 2.0 * s;
                let ri = ret(rot.left, cy - lado / 2.0, rot.left + lado, cy + lado / 2.0);
                self.icone(j.icone, ri, a);
                let lg = self.largura(&j.grupo, &f_min)?;
                let nome = if j.selecionada { t.texto_forte } else { t.texto };
                self.texto(&j.titulo, &f_min, ret(ri.right + 7.0 * s, rot.top + 4.0 * s, rot.right - lg - 12.0 * s, rot.bottom), cor(nome, a))?;
                self.texto(&j.grupo, &f_min, ret(rot.right - lg - 2.0 * s, rot.top + 4.0 * s, rot.right, rot.bottom), cor(mistura(t.texto, t.fundo, 0.3), a))?;
            } else if j.minimizada {
                // sem miniatura: ícone e título numa faixa
                self.preencher(j.r, cor(t.selecao, a))?;
                let lado = (14.0 * s).min(j.r.bottom - j.r.top - 4.0 * s);
                let ri = ret(j.r.left + 7.0 * s, (j.r.top + j.r.bottom - lado) / 2.0, j.r.left + 7.0 * s + lado, (j.r.top + j.r.bottom + lado) / 2.0);
                self.icone(j.icone, ri, 0.8 * a);
                self.texto(&j.titulo, &f_min, ret(ri.right + 6.0 * s, j.r.top, j.r.right - 6.0 * s, j.r.bottom), cor(t.texto, a))?;
            }
            // borda por fora, como a do Hyprland: a miniatura cobre o lado de dentro
            let (tinta, m) = if j.selecionada {
                (cor(t.destaque, 1.0), 2.0 * s)
            } else if j.hover {
                (cor(t.texto, 1.0), 2.0 * s)
            } else {
                (cor(t.mudo, a), 1.0 * s)
            };
            self.borda(ret(j.r.left - m, j.r.top - m, j.r.right + m, j.r.bottom + m), tinta, m)?;
        }
        Ok(())
    }
}

/// Ícone (HICON) com um glifo da Nerd Font na cor `cor` (0xRRGGBB), fundo transparente, para
/// pôr na barra de título de outras janelas (WM_SETICON).
pub fn icone_de_glifo(glifo: &str, tam: i32, cor_rgb: u32) -> Option<HICON> {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO};
    unsafe {
        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let rt = d2d
            .CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            })
            .ok()?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let dc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: tam,
                biHeight: -tam,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let antigo = SelectObject(dc, bmp.into());
        let s = tam as f32;
        let feito = (|| -> Result<()> {
            rt.BindDC(dc, &RECT { left: 0, top: 0, right: tam, bottom: tam })?;
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            // o glifo ocupa pouco do quadrado da fonte: tamanho maior e traço grosso para ler a 16 px
            let f = dw.CreateTextFormat(FONTE, None, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, s * 1.15, w!("pt-br"))?;
            f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            let b = rt.CreateSolidColorBrush(&cor(cor_rgb, 1.0), None)?;
            let u: Vec<u16> = glifo.encode_utf16().collect();
            rt.DrawText(&u, &f, &ret(0.0, 0.0, s, s), &b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            rt.EndDraw(None, None)
        })();
        // pré-multiplicado -> alfa direto, que o CreateIconIndirect espera
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (tam * tam) as usize);
        let mut direto: Vec<u32> = px
            .iter()
            .map(|&p| {
                let a = p >> 24;
                if a == 0 {
                    return 0;
                }
                let un = |c: u32| ((c * 255 + a / 2) / a).min(255);
                (a << 24) | (un((p >> 16) & 0xFF) << 16) | (un((p >> 8) & 0xFF) << 8) | un(p & 0xFF)
            })
            .collect();
        SelectObject(dc, antigo);
        _ = DeleteObject(bmp.into());
        _ = DeleteDC(dc);
        feito.ok()?;
        let cor_bmp = CreateBitmap(tam, tam, 1, 32, Some(direto.as_mut_ptr() as *const _));
        let mascara = CreateBitmap(tam, tam, 1, 1, None);
        let icone = CreateIconIndirect(&ICONINFO { fIcon: true.into(), hbmMask: mascara, hbmColor: cor_bmp, ..Default::default() }).ok();
        _ = DeleteObject(cor_bmp.into());
        _ = DeleteObject(mascara.into());
        icone
    }
}
