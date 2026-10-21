//! Desenho da barra com Direct2D, num DC (o da janela ou um bitmap, no modo --preview).

use std::collections::HashMap;
use std::ffi::c_void;

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::WindowsAndMessaging::HICON;

/// O que fica embaixo do cursor.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Alvo {
    Iniciar,
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
    pub titulo: String,
    pub icone: HICON,
    pub ativa: bool,
    pub minimizada: bool,
    pub piscando: bool,
}

pub struct Cena {
    pub w: i32,
    pub h: i32,
    pub escala: f32,
    pub grupos: Vec<ItemGrupo>,
    pub janelas: Vec<ItemJanela>,
    pub hover: Option<Alvo>,
    pub hora: String,
    pub data: String,
}

pub type Alvos = Vec<(D2D_RECT_F, Alvo)>;

fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

fn branco(a: f32) -> D2D1_COLOR_F {
    rgba(1.0, 1.0, 1.0, a)
}

fn ret(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F { left, top, right, bottom }
}

/// Cor de destaque do Windows; se for escura demais para ler sobre a barra, branco.
fn destaque() -> D2D1_COLOR_F {
    let mut v = 0u32;
    let mut tam = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent"),
            w!("AccentColorMenu"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut _ as *mut c_void),
            Some(&mut tam),
        )
    };
    if ok.is_ok() {
        let r = (v & 0xFF) as f32 / 255.0;
        let g = ((v >> 8) & 0xFF) as f32 / 255.0;
        let b = ((v >> 16) & 0xFF) as f32 / 255.0;
        if 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.35 {
            return rgba(r, g, b, 1.0);
        }
    }
    branco(1.0)
}

pub struct Gfx {
    d2d: ID2D1Factory,
    dw: IDWriteFactory,
    wic: IWICImagingFactory,
    rt: ID2D1DCRenderTarget,
    icones: HashMap<isize, ID2D1Bitmap>,
    destaque: D2D1_COLOR_F,
}

struct Fontes {
    texto: IDWriteTextFormat,
    grupo: IDWriteTextFormat,
    qtd: IDWriteTextFormat,
    hora: IDWriteTextFormat,
    simbolo: IDWriteTextFormat,
}

impl Gfx {
    pub fn new() -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let rt = Self::alvo(&d2d)?;
            Ok(Self { d2d, dw, wic, rt, icones: HashMap::new(), destaque: destaque() })
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

    pub fn atualizar_destaque(&mut self) {
        self.destaque = destaque();
    }

    fn pincel(&self, c: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush> {
        unsafe { self.rt.CreateSolidColorBrush(&c, None) }
    }

    fn formato(&self, fonte: PCWSTR, tam: f32, peso: DWRITE_FONT_WEIGHT, alinhar: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
        unsafe {
            let f = self.dw.CreateTextFormat(fonte, None, peso, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, tam, w!("pt-br"))?;
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

    fn fontes(&self, s: f32) -> Result<Fontes> {
        let ui = w!("Segoe UI Variable Text");
        Ok(Fontes {
            texto: self.formato(ui, 12.5 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            grupo: self.formato(ui, 13.0 * s, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            qtd: self.formato(ui, 11.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?,
            hora: self.formato(ui, 12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            simbolo: self.formato(w!("Segoe Fluent Icons"), 15.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
        })
    }

    fn texto(&self, s: &str, f: &IDWriteTextFormat, r: D2D_RECT_F, c: D2D1_COLOR_F) -> Result<()> {
        let u: Vec<u16> = s.encode_utf16().collect();
        let b = self.pincel(c)?;
        unsafe { self.rt.DrawText(&u, f, &r, &b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL) };
        Ok(())
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

    fn caixa(&self, r: D2D_RECT_F, raio: f32, c: D2D1_COLOR_F) -> Result<()> {
        let rr = D2D1_ROUNDED_RECT { rect: r, radiusX: raio, radiusY: raio };
        unsafe { self.rt.FillRoundedRectangle(&rr, &self.pincel(c)?) };
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

    /// Desenha a cena no DC e devolve onde ficou cada alvo clicável.
    pub fn desenhar(&mut self, dc: HDC, cena: &Cena) -> Result<Alvos> {
        unsafe {
            self.rt.BindDC(dc, &RECT { left: 0, top: 0, right: cena.w, bottom: cena.h })?;
            self.rt.BeginDraw();
        }
        let alvos = self.conteudo(cena);
        let fim = unsafe { self.rt.EndDraw(None, None) };
        if let Err(e) = fim {
            // dispositivo perdido: refaz o alvo e o cache de ícones na próxima pintura
            if e.code() == D2DERR_RECREATE_TARGET {
                self.rt = Self::alvo(&self.d2d)?;
                self.icones.clear();
            }
            return Err(e);
        }
        alvos
    }

    fn conteudo(&mut self, c: &Cena) -> Result<Alvos> {
        let s = c.escala;
        let (w, h) = (c.w as f32, c.h as f32);
        let f = self.fontes(s)?;
        let acc = self.destaque;
        let mut alvos: Alvos = Vec::new();
        let hover = |a: Alvo| c.hover == Some(a);

        unsafe {
            self.rt.Clear(Some(&rgba(0.085, 0.085, 0.09, 1.0)));
            self.rt.FillRectangle(&ret(0.0, 0.0, w, 1.0 * s), &self.pincel(branco(0.07))?);
        }

        let topo = 4.0 * s;
        let base = h - 4.0 * s;
        let alt = base - topo;
        let raio = 6.0 * s;
        let mut x = 6.0 * s;

        // Iniciar: quatro quadrados
        let r = ret(x, topo, x + alt, base);
        if hover(Alvo::Iniciar) {
            self.caixa(r, raio, branco(0.07))?;
        }
        let lado = 7.0 * s;
        let vao = 1.5 * s;
        let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
        for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (-1.0, 0.0), (0.0, 0.0)] {
            let l = cx + dx * lado + if dx < 0.0 { -vao / 2.0 } else { vao / 2.0 };
            let t = cy + dy * lado + if dy < 0.0 { -vao / 2.0 } else { vao / 2.0 };
            self.caixa(ret(l, t, l + lado, t + lado), 1.2 * s, branco(0.9))?;
        }
        alvos.push((r, Alvo::Iniciar));
        x = r.right + 6.0 * s;

        // grupos: nome e quantidade de janelas
        for (i, g) in c.grupos.iter().enumerate() {
            let qtd = g.qtd.to_string();
            let wn = self.largura(&g.nome, &f.grupo)?;
            let wq = self.largura(&qtd, &f.qtd)?;
            let pad = 11.0 * s;
            let r = ret(x, topo, x + pad + wn + 6.0 * s + wq + pad, base);
            if g.ativo {
                self.caixa(r, raio, branco(0.11))?;
                let meio = (r.left + r.right) / 2.0;
                self.caixa(ret(meio - 9.0 * s, base - 3.0 * s, meio + 9.0 * s, base), 1.5 * s, acc)?;
            } else if hover(Alvo::Grupo(i)) {
                self.caixa(r, raio, branco(0.06))?;
            }
            let tinta = if g.ativo { branco(1.0) } else { branco(0.62) };
            self.texto(&g.nome, &f.grupo, ret(r.left + pad, topo, r.left + pad + wn + 1.0, base - 1.0 * s), tinta)?;
            let xq = r.left + pad + wn + 6.0 * s;
            self.texto(&qtd, &f.qtd, ret(xq, topo, xq + wq + 1.0, base - 1.0 * s), branco(if g.ativo { 0.55 } else { 0.38 }))?;
            alvos.push((r, Alvo::Grupo(i)));
            x = r.right + 2.0 * s;
        }
        let r = ret(x, topo, x + alt * 0.8, base);
        if hover(Alvo::NovoGrupo) {
            self.caixa(r, raio, branco(0.06))?;
        }
        self.texto("\u{E710}", &f.simbolo, ret(r.left, topo, r.right, base), branco(0.55))?;
        alvos.push((r, Alvo::NovoGrupo));
        x = r.right + 8.0 * s;
        unsafe {
            self.rt.FillRectangle(&ret(x, 11.0 * s, x + 1.0 * s, h - 11.0 * s), &self.pincel(branco(0.12))?);
        }
        x += 9.0 * s;

        // direita: relógio e configurações rápidas
        let largura_hora = 82.0 * s;
        let rh = ret(w - 6.0 * s - largura_hora, topo, w - 6.0 * s, base);
        let largura_sis = 64.0 * s;
        let rs = ret(rh.left - 2.0 * s - largura_sis, topo, rh.left - 2.0 * s, base);
        if hover(Alvo::Relogio) {
            self.caixa(rh, raio, branco(0.07))?;
        }
        let meio = (topo + base) / 2.0;
        self.texto(&c.hora, &f.hora, ret(rh.left, meio - 16.0 * s, rh.right, meio), branco(0.95))?;
        self.texto(&c.data, &f.hora, ret(rh.left, meio, rh.right, meio + 16.0 * s), branco(0.95))?;
        alvos.push((rh, Alvo::Relogio));
        if hover(Alvo::Sistema) {
            self.caixa(rs, raio, branco(0.07))?;
        }
        let ms = (rs.left + rs.right) / 2.0;
        self.texto("\u{E701}", &f.simbolo, ret(rs.left, topo, ms + 2.0 * s, base), branco(0.9))?;
        self.texto("\u{E767}", &f.simbolo, ret(ms - 2.0 * s, topo, rs.right, base), branco(0.9))?;
        alvos.push((rs, Alvo::Sistema));

        // janelas do grupo ativo
        let fim = rs.left - 10.0 * s;
        let n = c.janelas.len();
        if n > 0 {
            let ideal = 196.0 * s;
            let cabe = ((fim - x) / n as f32 - 2.0 * s).min(ideal).max(alt);
            for j in &c.janelas {
                if x + cabe > fim + 0.5 {
                    break;
                }
                let r = ret(x, topo, x + cabe, base);
                let fundo = if j.piscando {
                    Some(rgba(0.95, 0.62, 0.15, 0.38))
                } else if j.ativa {
                    Some(branco(0.11))
                } else if hover(Alvo::Janela(j.hwnd)) {
                    Some(branco(0.06))
                } else {
                    None
                };
                if let Some(cor) = fundo {
                    self.caixa(r, raio, cor)?;
                }
                let lado = 20.0 * s;
                let so_icone = cabe < 92.0 * s;
                let xi = if so_icone { (r.left + r.right - lado) / 2.0 } else { r.left + 10.0 * s };
                let ri = ret(xi, meio - lado / 2.0, xi + lado, meio + lado / 2.0);
                if let Some(b) = self.bitmap(j.icone) {
                    let opac = if j.minimizada { 0.6 } else { 1.0 };
                    unsafe { self.rt.DrawBitmap(&b, Some(&ri), opac, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
                }
                if !so_icone {
                    let tinta = if j.ativa { branco(1.0) } else if j.minimizada { branco(0.5) } else { branco(0.82) };
                    self.texto(&j.titulo, &f.texto, ret(ri.right + 8.0 * s, topo, r.right - 8.0 * s, base - 1.0 * s), tinta)?;
                }
                // marca embaixo: comprida na ativa, curta nas outras
                let meio_x = (r.left + r.right) / 2.0;
                let (meia, cor) = if j.ativa { (8.0 * s, acc) } else { (3.0 * s, branco(0.38)) };
                self.caixa(ret(meio_x - meia, base - 3.0 * s, meio_x + meia, base), 1.5 * s, cor)?;
                alvos.push((r, Alvo::Janela(j.hwnd)));
                x = r.right + 2.0 * s;
            }
        }
        Ok(alvos)
    }
}
