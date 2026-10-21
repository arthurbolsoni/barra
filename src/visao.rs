//! Visão geral: todos os grupos ao mesmo tempo, cada um num cartão com as janelas no lugar
//! onde ficam na tela. As miniaturas são as do DWM (DwmRegisterThumbnail): ao vivo, inclusive
//! das janelas estacionadas, que o Windows continua desenhando.

use std::collections::HashMap;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::grupos::{posicao_salva, Grupos};
use crate::janelas::hwnd;

pub struct Cartao {
    /// None = cartão "Novo grupo".
    pub grupo: Option<usize>,
    pub r: D2D_RECT_F,
    pub cab: D2D_RECT_F,
    pub area: D2D_RECT_F,
}

pub struct Item {
    pub hwnd: isize,
    pub grupo: usize,
    pub r: D2D_RECT_F,
    /// Parte da janela que aparece (coordenadas da janela), para a miniatura.
    pub origem: RECT,
    pub minimizada: bool,
    /// Alt+Tab: faixa do nome embaixo do quadro.
    pub rotulo: Option<D2D_RECT_F>,
}

pub struct Arrasto {
    pub item: usize,
    pub inicio: (i32, i32),
    pub atual: (i32, i32),
    pub ativo: bool,
}

#[derive(Default)]
pub struct Visao {
    pub janela: HWND,
    pub aberta: bool,
    pub cartoes: Vec<Cartao>,
    /// Do fundo para a frente (ordem de pintura das miniaturas).
    pub itens: Vec<Item>,
    pub miniaturas: HashMap<isize, isize>,
    pub selecao: Option<isize>,
    pub hover: Option<usize>,
    pub hover_cartao: Option<usize>,
    pub filtro: String,
    pub arrasto: Option<Arrasto>,
    /// Janela em foco antes de abrir, para voltar no Esc.
    pub anterior: isize,
    /// Grupos e janelas do último layout, para saber quando refazer.
    pub assinatura: Vec<(usize, isize, bool)>,
    /// Renomeando: (grupo, caixa de texto).
    pub editando: Option<(usize, HWND)>,
    /// Aberta pelo Alt+Tab: todas as janelas lado a lado, sem sobreposição.
    pub lista: bool,
    /// Quadro escolhido pelo teclado (grupo vazio ou "novo grupo"), no lugar de uma janela.
    pub cartao_sel: Option<usize>,
}

fn ret(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F { left, top, right, bottom }
}

fn dentro(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// Retângulo da janela na tela "normal" (estacionada: onde estava; minimizada: onde volta).
fn onde(h: isize) -> (RECT, bool) {
    unsafe {
        let j = hwnd(h);
        if IsIconic(j).as_bool() {
            let mut p = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
            _ = GetWindowPlacement(j, &mut p);
            return (p.rcNormalPosition, true);
        }
        if let Some(r) = posicao_salva(h) {
            return (r, false);
        }
        let mut r = RECT::default();
        _ = GetWindowRect(j, &mut r);
        (r, false)
    }
}

/// Ordem z das janelas (0 = da frente).
fn ordem_z() -> HashMap<isize, usize> {
    unsafe extern "system" fn junta(h: HWND, lp: LPARAM) -> windows::core::BOOL {
        let v = &mut *(lp.0 as *mut Vec<isize>);
        v.push(h.0 as isize);
        TRUE
    }
    let mut v: Vec<isize> = Vec::new();
    unsafe { _ = EnumWindows(Some(junta), LPARAM(&mut v as *mut _ as isize)) };
    v.into_iter().enumerate().map(|(i, h)| (h, i)).collect()
}

pub fn assinatura(g: &Grupos) -> Vec<(usize, isize, bool)> {
    g.lista
        .iter()
        .enumerate()
        .flat_map(|(i, gr)| gr.janelas.iter().map(move |&h| (i, h, unsafe { IsIconic(hwnd(h)).as_bool() })))
        .collect()
}

impl Visao {
    pub fn tirar_miniaturas(&mut self) {
        for (_, t) in self.miniaturas.drain() {
            unsafe { _ = DwmUnregisterThumbnail(t) };
        }
    }

    /// Monta os cartões e as janelas para a área `w`×`h` e registra as miniaturas.
    pub fn montar(&mut self, grupos: &Grupos, monitor: RECT, w: f32, h: f32, s: f32) {
        self.tirar_miniaturas();
        self.cartoes.clear();
        self.itens.clear();
        self.hover = None;
        self.hover_cartao = None;
        self.assinatura = assinatura(grupos);

        let mw = (monitor.right - monitor.left) as f32;
        let mh = (monitor.bottom - monitor.top) as f32;
        let n = grupos.lista.len() + 1;
        let topo = 70.0 * s;
        let pad = 20.0 * s;
        let vao = 10.0 * s;
        let cab = 32.0 * s;
        // faixa no pé do cartão para as minimizadas (a miniatura do DWM cobriria o que estiver embaixo dela)
        let pe = 30.0 * s;
        let (aw, ah) = (w - 2.0 * pad, h - topo - pad);

        // quantas colunas deixam as miniaturas maiores
        let mut melhor = (1usize, 0.0f32);
        for cols in 1..=n {
            let linhas = n.div_ceil(cols);
            let cw = (aw - vao * (cols - 1) as f32) / cols as f32;
            let ch = (ah - vao * (linhas - 1) as f32) / linhas as f32 - cab - pe;
            let k = (cw / mw).min(ch / mh);
            if k > melhor.1 {
                melhor = (cols, k);
            }
        }
        let (cols, k) = melhor;
        let linhas = n.div_ceil(cols);
        let cw = mw * k;
        let ch = mh * k + cab + pe;
        let total_w = cw * cols as f32 + vao * (cols - 1) as f32;
        let total_h = ch * linhas as f32 + vao * (linhas - 1) as f32;
        let x0 = (w - total_w) / 2.0;
        let y0 = topo + (ah - total_h) / 2.0;

        let z = ordem_z();
        for i in 0..n {
            let (c, l) = (i % cols, i / cols);
            let x = x0 + c as f32 * (cw + vao);
            let y = y0 + l as f32 * (ch + vao);
            let r = ret(x, y, x + cw, y + ch);
            let cab_r = ret(x, y, x + cw, y + cab);
            let area = ret(x + 10.0 * s, y + cab, x + cw - 10.0 * s, y + ch - pe);
            let grupo = (i < grupos.lista.len()).then_some(i);
            if let Some(g) = grupo {
                let ka = (area.right - area.left) / mw;
                let ka = ka.min((area.bottom - area.top) / mh);
                let mut js: Vec<isize> = grupos.lista[g].janelas.clone();
                // do fundo para a frente
                js.sort_by_key(|h| std::cmp::Reverse(z.get(h).copied().unwrap_or(usize::MAX)));
                let mut minimizadas = 0;
                let n_min = js.iter().filter(|&&h| unsafe { IsIconic(hwnd(h)).as_bool() }).count().max(1);
                for h in js {
                    let (rj, min) = onde(h);
                    if min {
                        // minimizadas: fila de cartõezinhos na faixa do pé
                        let alt = 22.0 * s;
                        let larg = ((area.right - area.left - 4.0 * s * (n_min - 1) as f32) / n_min as f32).min(220.0 * s);
                        let cx = area.left + minimizadas as f32 * (larg + 4.0 * s);
                        let cy = area.bottom + (pe - alt) / 2.0;
                        minimizadas += 1;
                        self.itens.push(Item { hwnd: h, grupo: g, r: ret(cx, cy, cx + larg, cy + alt), origem: RECT::default(), minimizada: true, rotulo: None });
                        continue;
                    }
                    // só a parte que cabe no monitor
                    let v = RECT {
                        left: rj.left.max(monitor.left),
                        top: rj.top.max(monitor.top),
                        right: rj.right.min(monitor.right),
                        bottom: rj.bottom.min(monitor.bottom),
                    };
                    if v.right <= v.left || v.bottom <= v.top {
                        continue;
                    }
                    let origem = RECT { left: v.left - rj.left, top: v.top - rj.top, right: v.right - rj.left, bottom: v.bottom - rj.top };
                    let m = |a: i32, b: i32| (a - b) as f32 * ka;
                    let dest = ret(
                        area.left + m(v.left, monitor.left),
                        area.top + m(v.top, monitor.top),
                        area.left + m(v.right, monitor.left),
                        area.top + m(v.bottom, monitor.top),
                    );
                    self.itens.push(Item { hwnd: h, grupo: g, r: dest, origem, minimizada: false, rotulo: None });
                }
            }
            self.cartoes.push(Cartao { grupo, r, cab: cab_r, area });
        }

        for it in &self.itens {
            if it.minimizada {
                continue;
            }
            if let Ok(t) = unsafe { DwmRegisterThumbnail(self.janela, hwnd(it.hwnd)) } {
                self.miniaturas.insert(it.hwnd, t);
            }
        }
        self.aplicar_miniaturas();
    }

    /// Alt+Tab: todas as janelas (na ordem de uso) num grid, cada uma no seu quadro, sem
    /// nenhuma por cima da outra; o nome vai numa faixa embaixo.
    pub fn montar_lista(&mut self, grupos: &Grupos, mru: &[isize], monitor: RECT, w: f32, h: f32, s: f32) {
        self.tirar_miniaturas();
        self.cartoes.clear();
        self.itens.clear();
        self.hover = None;
        self.hover_cartao = None;
        self.assinatura = assinatura(grupos);

        let js: Vec<(isize, usize)> = mru.iter().filter_map(|&j| grupos.grupo_de(j).map(|g| (j, g))).collect();
        let n = js.len();
        if n == 0 {
            return;
        }
        let (topo, pad, vao) = (70.0 * s, 20.0 * s, 10.0 * s);
        let rot = 26.0 * s;
        let (aw, ah) = (w - 2.0 * pad, h - topo - pad);
        // proporção do quadro = a mediana das janelas (terços são altos, maximizadas são largas),
        // para a miniatura encher o quadro em vez de encolher dentro dele
        let mut props: Vec<f32> = js
            .iter()
            .map(|&(j, _)| {
                let (r, _) = onde(j);
                ((r.bottom - r.top).max(1) as f32 / (r.right - r.left).max(1) as f32).clamp(0.35, 1.6)
            })
            .collect();
        props.sort_by(|a, b| a.total_cmp(b));
        let prop = props[props.len() / 2];
        // colunas que deixam o quadro maior (sem passar de 80% da altura útil)
        let mut melhor = (1usize, 0.0f32);
        for cols in 1..=n {
            let linhas = n.div_ceil(cols);
            let cw = (aw - vao * (cols - 1) as f32) / cols as f32;
            let ch = (ah - vao * (linhas - 1) as f32) / linhas as f32 - rot;
            let larg = cw.min(ch / prop).min(ah * 0.8 / prop);
            // empate (quadro já no limite): mais colunas, menos linhas
            if larg >= melhor.1 - 0.5 {
                melhor = (cols, larg);
            }
        }
        let (cols, cw) = melhor;
        let ch = cw * prop;
        let linhas = n.div_ceil(cols);
        let total_h = (ch + rot) * linhas as f32 + vao * (linhas - 1) as f32;
        let y0 = topo + (ah - total_h) / 2.0;
        for (i, &(j, g)) in js.iter().enumerate() {
            let (c, l) = (i % cols, i / cols);
            // a última linha, incompleta, fica centralizada
            let nessa = if l == linhas - 1 { n - l * cols } else { cols };
            let largura_linha = cw * nessa as f32 + vao * (nessa - 1) as f32;
            let x = pad + (aw - largura_linha) / 2.0 + c as f32 * (cw + vao);
            let y = y0 + l as f32 * (ch + rot + vao);
            let celula = ret(x, y, x + cw, y + ch);
            let rotulo = Some(ret(x, y + ch, x + cw, y + ch + rot));
            let (rj, min) = onde(j);
            if min {
                self.itens.push(Item { hwnd: j, grupo: g, r: celula, origem: RECT::default(), minimizada: true, rotulo });
                continue;
            }
            // a janela inteira, com a proporção dela, centrada no quadro
            let v = RECT {
                left: rj.left.max(monitor.left),
                top: rj.top.max(monitor.top),
                right: rj.right.min(monitor.right),
                bottom: rj.bottom.min(monitor.bottom),
            };
            let (vw, vh) = ((v.right - v.left).max(1) as f32, (v.bottom - v.top).max(1) as f32);
            let k = (cw / vw).min(ch / vh);
            let (dw, dh) = (vw * k, vh * k);
            let dx = x + (cw - dw) / 2.0;
            let dy = y + (ch - dh) / 2.0;
            let origem = RECT { left: v.left - rj.left, top: v.top - rj.top, right: v.right - rj.left, bottom: v.bottom - rj.top };
            self.itens.push(Item { hwnd: j, grupo: g, r: ret(dx, dy, dx + dw, dy + dh), origem, minimizada: false, rotulo });
        }
        for it in &self.itens {
            if it.minimizada {
                continue;
            }
            if let Ok(t) = unsafe { DwmRegisterThumbnail(self.janela, hwnd(it.hwnd)) } {
                self.miniaturas.insert(it.hwnd, t);
            }
        }
        self.aplicar_miniaturas();
    }

    pub fn casa(&self, h: isize) -> bool {
        let t = self.filtro.to_lowercase();
        t.is_empty() || crate::janelas::titulo(hwnd(h)).to_lowercase().contains(&t)
    }

    /// Posição, opacidade (filtro) e visibilidade das miniaturas.
    pub fn aplicar_miniaturas(&self) {
        for (i, it) in self.itens.iter().enumerate() {
            let Some(&t) = self.miniaturas.get(&it.hwnd) else { continue };
            let mut r = it.r;
            if let Some(a) = self.arrasto.as_ref().filter(|a| a.ativo && a.item == i) {
                let (dx, dy) = ((a.atual.0 - a.inicio.0) as f32, (a.atual.1 - a.inicio.1) as f32);
                r = ret(r.left + dx, r.top + dy, r.right + dx, r.bottom + dy);
            }
            let p = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_RECTSOURCE | DWM_TNP_OPACITY | DWM_TNP_VISIBLE | DWM_TNP_SOURCECLIENTAREAONLY,
                rcDestination: RECT { left: r.left as i32, top: r.top as i32, right: r.right as i32, bottom: r.bottom as i32 },
                rcSource: it.origem,
                opacity: if self.casa(it.hwnd) { 255 } else { 45 },
                fVisible: TRUE,
                fSourceClientAreaOnly: FALSE,
            };
            unsafe { _ = DwmUpdateThumbnailProperties(t, &p) };
        }
    }

    /// Janela embaixo do ponto (a da frente primeiro).
    pub fn item_em(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.itens.len()).rev().find(|&i| dentro(&self.itens[i].r, x, y))
    }

    pub fn cartao_em(&self, x: f32, y: f32) -> Option<usize> {
        self.cartoes.iter().position(|c| dentro(&c.r, x, y))
    }

    /// Ordem para andar com as setas: grupo a grupo, da esquerda para a direita.
    pub fn ordem_espacial(&self) -> Vec<isize> {
        let mut v: Vec<&Item> = self.itens.iter().filter(|i| self.casa(i.hwnd)).collect();
        if self.lista {
            // já estão na ordem de uso; as setas andam na leitura (linha a linha)
            v.sort_by_key(|i| (i.r.top as i32 / 8, i.r.left as i32));
        } else {
            v.sort_by(|a, b| (a.grupo, a.r.left as i32, a.r.top as i32).cmp(&(b.grupo, b.r.left as i32, b.r.top as i32)));
        }
        v.into_iter().map(|i| i.hwnd).collect()
    }
}
