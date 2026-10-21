//! Desenho da barra, da dica e da visão geral com Direct2D, num DC (o da janela ou um bitmap,
//! no modo --preview). Visual do Omarchy: faixa fina em cima, texto e glifos em JetBrainsMono
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

const FONTE: PCWSTR = w!("JetBrainsMono Nerd Font");

// glifos da Nerd Font
const G_MAIS: &str = "\u{f067}";
const G_WIFI: &str = "\u{f05a9}";
const G_VOLUME: &str = "\u{f057e}";

/// O que fica embaixo do cursor.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Alvo {
    Bandeja(usize),
    Grupo(usize),
    NovoGrupo,
    Janela(isize),
    Sistema,
    Relogio,
}

pub struct ItemGrupo {
    pub nome: String,
    pub qtd: usize,
    pub ativo: bool,
}

pub struct ItemJanela {
    pub hwnd: isize,
    pub icone: HICON,
    pub ativa: bool,
    pub minimizada: bool,
    pub piscando: bool,
    /// Selo do ITaskbarList3 (pode ser nulo).
    pub selo: HICON,
    /// (estado TBPF, fração 0..1) quando o programa mostra progresso.
    pub progresso: Option<(u32, f32)>,
}

pub struct ItemBandeja {
    pub indice: usize,
    pub icone: HICON,
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
}

pub struct JanelaV {
    pub r: D2D_RECT_F,
    pub titulo: String,
    pub icone: HICON,
    pub minimizada: bool,
    pub selecionada: bool,
    pub hover: bool,
    pub apagada: bool,
}

pub struct CenaVisao {
    pub w: i32,
    pub h: i32,
    pub escala: f32,
    pub cartoes: Vec<CartaoV>,
    pub janelas: Vec<JanelaV>,
    pub filtro: String,
    pub titulo: String,
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
    rt: ID2D1DCRenderTarget,
    icones: HashMap<isize, ID2D1Bitmap>,
    pub tema: &'static Tema,
}

impl Gfx {
    pub fn new(tema: &'static Tema) -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let rt = Self::alvo(&d2d)?;
            Ok(Self { d2d, dw, wic, rt, icones: HashMap::new(), tema })
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
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
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

    /// Glifo da Nerd Font: sem ClearType, que deixa franja colorida nos traços finos.
    fn glifo(&self, s: &str, f: &IDWriteTextFormat, r: D2D_RECT_F, c: D2D1_COLOR_F) -> Result<()> {
        unsafe { self.rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
        let feito = self.texto(s, f, r, c);
        unsafe { self.rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE) };
        feito
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

    fn bitmap(&mut self, icone: HICON) -> Option<ID2D1Bitmap> {
        if icone.is_invalid() {
            return None;
        }
        let chave = icone.0 as isize;
        if let Some(b) = self.icones.get(&chave) {
            return Some(b.clone());
        }
        if self.icones.len() > 400 {
            self.icones.clear();
        }
        let b = unsafe {
            let wb = self.wic.CreateBitmapFromHICON(icone).ok()?;
            let conv = self.wic.CreateFormatConverter().ok()?;
            conv.Initialize(&wb, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
            self.rt.CreateBitmapFromWicBitmap(&conv, None).ok()?
        };
        self.icones.insert(chave, b.clone());
        Some(b)
    }

    fn icone(&mut self, ic: HICON, r: D2D_RECT_F, opac: f32) {
        if let Some(b) = self.bitmap(ic) {
            unsafe { self.rt.DrawBitmap(&b, Some(&r), opac, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
        }
    }

    fn no_dc<R>(&mut self, dc: HDC, w: i32, h: i32, f: impl FnOnce(&mut Self) -> Result<R>) -> Result<R> {
        unsafe {
            self.rt.BindDC(dc, &RECT { left: 0, top: 0, right: w, bottom: h })?;
            self.rt.BeginDraw();
        }
        let r = f(self);
        let fim = unsafe { self.rt.EndDraw(None, None) };
        if let Err(e) = fim {
            // dispositivo perdido: refaz o alvo e o cache de ícones na próxima pintura
            if e.code() == D2DERR_RECREATE_TARGET {
                self.rt = Self::alvo(&self.d2d)?;
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
        let f_ativo = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let f_glifo = self.formato(14.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let mut alvos: Alvos = Vec::new();
        let hover = |a: Alvo| c.hover == Some(a);
        let realce = cor(t.fundo_claro, 1.0);

        unsafe { self.rt.Clear(Some(&cor(t.fundo, 1.0))) };

        // grupos, como os workspaces da Waybar: só o nome; ativo no destaque, vazio apagado
        let mut x = 6.0 * s;
        for (i, g) in c.grupos.iter().enumerate() {
            let fonte = if g.ativo { &f_ativo } else { &f_texto };
            let larg = self.largura(&g.nome, fonte)?.max(7.0 * s) + 14.0 * s;
            let r = ret(x, 0.0, x + larg, h);
            if hover(Alvo::Grupo(i)) && !g.ativo {
                self.preencher(r, realce)?;
            }
            let tinta = if g.ativo {
                cor(t.destaque, 1.0)
            } else if g.qtd == 0 {
                // apagado, mas legível (texto_escuro fica abaixo de 3:1 em vários temas)
                cor(mistura(t.texto, t.fundo, 0.3), 1.0)
            } else {
                cor(t.texto, 1.0)
            };
            self.texto(&g.nome, fonte, r, tinta)?;
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

        // janelas do grupo: só ícones; a com foco ganha a linha do destaque embaixo
        let lado = 16.0 * s;
        let celula = 28.0 * s;
        let meio = h / 2.0;
        for j in &c.janelas {
            let r = ret(x, 0.0, x + celula, h);
            if j.piscando {
                self.preencher(r, cor(t.amarelo, 0.28))?;
            } else if hover(Alvo::Janela(j.hwnd)) {
                self.preencher(r, realce)?;
            }
            let cx = (r.left + r.right) / 2.0;
            let ri = ret(cx - lado / 2.0, meio - lado / 2.0, cx + lado / 2.0, meio + lado / 2.0);
            let apagado = if t.claro { 0.55 } else { 0.4 };
            self.icone(j.icone, ri, if j.minimizada { apagado } else { 1.0 });
            if !j.selo.is_invalid() {
                let l = 9.0 * s;
                self.icone(j.selo, ret(ri.right - l * 0.6, ri.bottom - l * 0.7, ri.right + l * 0.4, ri.bottom + l * 0.3), 1.0);
            }
            if j.ativa {
                self.preencher(ret(r.left + 4.0 * s, h - 2.0 * s, r.right - 4.0 * s, h), cor(t.destaque, 1.0))?;
            } else if j.piscando {
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
        let rs = ret(w - 6.0 * s - 56.0 * s, 0.0, w - 6.0 * s, h);
        if hover(Alvo::Sistema) {
            self.preencher(rs, realce)?;
        }
        let ms = (rs.left + rs.right) / 2.0;
        self.glifo(G_WIFI, &f_glifo, ret(rs.left, 0.0, ms, h), cor(t.texto, 1.0))?;
        self.glifo(G_VOLUME, &f_glifo, ret(ms, 0.0, rs.right, h), cor(t.texto, 1.0))?;
        alvos.push((rs, Alvo::Sistema));
        let mut xb = rs.left - 6.0 * s;
        for ic in c.bandeja.iter().rev() {
            let r = ret(xb - 24.0 * s, 0.0, xb, h);
            if hover(Alvo::Bandeja(ic.indice)) {
                self.preencher(r, realce)?;
            }
            let cx = (r.left + r.right) / 2.0;
            let l = 16.0 * s;
            self.icone(ic.icone, ret(cx - l / 2.0, meio - l / 2.0, cx + l / 2.0, meio + l / 2.0), 1.0);
            alvos.push((r, Alvo::Bandeja(ic.indice)));
            xb = r.left;
        }

        // centro: dia e hora (some se as janelas chegarem até ele)
        let lh = self.largura(&c.hora, &f_texto)? + 16.0 * s;
        let rh = ret(w / 2.0 - lh / 2.0, 0.0, w / 2.0 + lh / 2.0, h);
        if rh.left > fim_esquerda && rh.right < xb {
            if hover(Alvo::Relogio) {
                self.preencher(rh, realce)?;
            }
            self.texto(&c.hora, &f_texto, rh, cor(t.texto, 1.0))?;
            alvos.push((rh, Alvo::Relogio));
        }
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

    pub fn desenhar_visao(&mut self, dc: HDC, c: &CenaVisao) -> Result<()> {
        self.no_dc(dc, c.w, c.h, |g| g.conteudo_visao(c))
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
        let f_novo = self.formato(16.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;

        unsafe { self.rt.Clear(Some(&cor(t.fundo, 1.0))) };

        // topo: a janela escolhida e, embaixo, como usar ou o filtro
        self.texto(&c.titulo, &f_titulo, ret(40.0 * s, 12.0 * s, w - 40.0 * s, 38.0 * s), cor(t.texto_forte, 1.0))?;
        let (dica, tinta) = if c.filtro.is_empty() {
            ("digite para filtrar · enter abre · arraste para outro grupo · duplo clique renomeia · esc fecha".to_string(), t.texto)
        } else {
            (format!("filtro: {}", c.filtro), t.destaque)
        };
        self.texto(&dica, &f_dica, ret(40.0 * s, 38.0 * s, w - 40.0 * s, 58.0 * s), cor(tinta, 1.0))?;

        let e = 2.0 * s;
        for k in &c.cartoes {
            let borda = if k.destino {
                t.amarelo
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
                let cy = (k.r.top + k.r.bottom) / 2.0;
                self.glifo(G_MAIS, &f_novo, ret(k.r.left, cy - 34.0 * s, k.r.right, cy - 8.0 * s), tinta)?;
                self.texto("novo", &f_dica, ret(k.r.left, cy - 4.0 * s, k.r.right, cy + 12.0 * s), tinta)?;
                self.texto("grupo", &f_dica, ret(k.r.left, cy + 12.0 * s, k.r.right, cy + 28.0 * s), tinta)?;
                continue;
            }
            self.preencher(k.area, cor(t.fundo_claro, 1.0))?;
            self.borda(k.r, cor(borda, 1.0), e)?;
            let pad = 12.0 * s;
            let nome = if k.ativo { t.destaque } else { t.texto };
            self.texto(&k.nome, &f_nome, ret(k.cab.left + pad, k.cab.top, k.cab.right - 110.0 * s, k.cab.bottom), cor(nome, 1.0))?;
            let qtd = if k.qtd == 1 { "1 janela".to_string() } else { format!("{} janelas", k.qtd) };
            self.texto(&qtd, &f_qtd, ret(k.cab.right - 140.0 * s, k.cab.top, k.cab.right - pad, k.cab.bottom), cor(t.texto, 1.0))?;
        }

        for j in &c.janelas {
            let a = if j.apagada { 0.3 } else { 1.0 };
            if j.minimizada {
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
