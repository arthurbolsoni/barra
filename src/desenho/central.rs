//! Desenho da central: cabeçalho com dia, vista e fontes; linha do tempo (agenda e rotinas) ou
//! semana; pendências e prazos; mês, rotinas, sessões do Claude e fontes com problema. Mesmo
//! vocabulário da barra: cantos retos, linhas de 1 px em `selecao`, estado por cor do tema.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};

use super::*;
use crate::central::{inicio_semana, semana_iso, AlvoCentral, Central, Estado, EstadoSessao, Sel, Vista};

const DIAS_C: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
const INICIAIS: [&str; 7] = ["D", "S", "T", "Q", "Q", "S", "S"];
const G_ATENCAO: &str = "\u{f071}";

pub struct CenaCentral<'a> {
    pub w: i32,
    pub h: i32,
    pub escala: f32,
    pub c: &'a Central,
    pub agora: NaiveDateTime,
}

pub type AlvosCentral = Vec<(D2D_RECT_F, AlvoCentral)>;

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|p| p.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn mes_nome(m: u32) -> &'static str {
    crate::MESES[(m as usize + 11) % 12]
}

fn dia_semana(d: NaiveDate) -> usize {
    d.weekday().num_days_from_sunday() as usize
}

/// Data relativa a hoje: "atrasado", "hoje", "amanhã", "sex 30", "30/10".
fn quando(d: NaiveDate, hoje: NaiveDate) -> (String, i64) {
    let n = (d - hoje).num_days();
    let s = match n {
        _ if n < 0 => "atrasado".into(),
        0 => "hoje".into(),
        1 => "amanhã".into(),
        2..=6 => format!("{} {}", DIAS_C[dia_semana(d)], d.day()),
        _ if d.year() == hoje.year() => format!("{}/{}", d.day(), d.month()),
        _ => format!("{}/{}/{}", d.day(), d.month(), d.year() % 100),
    };
    (s, n)
}

fn hm(d: NaiveDateTime) -> String {
    format!("{:02}:{:02}", d.hour(), d.minute())
}

fn duracao(m: i64) -> String {
    if m < 60 {
        format!("{m} min")
    } else if m % 60 == 0 {
        format!("{}h", m / 60)
    } else {
        format!("{}h{:02}", m / 60, m % 60)
    }
}

impl Gfx {
    pub fn desenhar_central(&mut self, dc: HDC, cena: &CenaCentral) -> Result<AlvosCentral> {
        self.no_dc(dc, cena.w, cena.h, |g| g.conteudo_central(cena))
    }

    /// Formato com quebra de linha e texto a partir do topo (descrições e erros).
    fn formato_quebra(&self, tam: f32) -> Result<IDWriteTextFormat> {
        let f = self.formato(tam, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        unsafe {
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
        }
        Ok(f)
    }

    fn conteudo_central(&mut self, cena: &CenaCentral) -> Result<AlvosCentral> {
        let t = self.tema;
        let s = cena.escala;
        let c = cena.c;
        let d = &c.dados;
        let (w, h) = (cena.w as f32, cena.h as f32);
        let hoje = cena.agora.date();
        let hover = |a: AlvoCentral| c.hover == Some(a);
        let mut alvos: AlvosCentral = Vec::new();

        let f = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let fb = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let fr = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let fc = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let fcb = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let ftit = self.formato(15.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let fg = self.formato(13.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let fq = self.formato_quebra(12.0 * s)?;

        let apagado = mistura(t.texto, t.fundo, 0.3);
        let rotulo = mistura(t.texto, t.fundo, 0.45);
        let k = |x: u32| cor(x, 1.0);
        let tinta_estado = |e: Estado| match e {
            Estado::Ok => t.verde,
            Estado::Falha => t.vermelho,
            Estado::Executando => t.amarelo,
            Estado::Agendada => t.texto_escuro,
        };
        let fonte_g = |i: usize| d.fontes.get(i).map(|f| f.glifo).unwrap_or("");

        unsafe { self.rt.Clear(Some(&k(t.fundo))) };

        // ---------- cabeçalho ----------
        let topo = 44.0 * s;
        let mut x = 16.0 * s;
        let (titulo, sub) = match c.vista {
            Vista::Dia => (
                format!("{}, {} de {}", crate::DIAS[dia_semana(c.dia)], c.dia.day(), mes_nome(c.dia.month())),
                if c.dia == hoje { format!("hoje · semana {}", semana_iso(c.dia)) } else { format!("semana {}", semana_iso(c.dia)) },
            ),
            Vista::Semana => {
                let ini = inicio_semana(c.dia);
                let fim = ini + Duration::days(6);
                let t = if ini.month() == fim.month() {
                    format!("{} a {} de {}", ini.day(), fim.day(), mes_nome(fim.month()))
                } else {
                    format!("{} de {} a {} de {}", ini.day(), mes_nome(ini.month()), fim.day(), mes_nome(fim.month()))
                };
                (t, format!("semana {}", semana_iso(c.dia)))
            }
        };
        let lt = self.largura(&titulo, &ftit)?;
        self.texto(&titulo, &ftit, ret(x, 0.0, x + lt + 4.0 * s, topo), k(t.texto_forte))?;
        x += lt + 12.0 * s;
        let ls = self.largura(&sub, &f)?;
        self.texto(&sub, &f, ret(x, 0.0, x + ls + 4.0 * s, topo), k(apagado))?;
        x += ls + 14.0 * s;
        let cel = 26.0 * s;
        for (al, g) in [(AlvoCentral::Anterior, "\u{f104}"), (AlvoCentral::Seguinte, "\u{f105}")] {
            let r = ret(x, 9.0 * s, x + cel, 35.0 * s);
            if hover(al) {
                self.preencher(r, k(t.fundo_claro))?;
            }
            self.glifo(g, &fg, r, k(if hover(al) { t.texto_forte } else { t.texto }))?;
            alvos.push((r, al));
            x += cel;
        }
        x += 14.0 * s;
        for (v, nome) in [(Vista::Dia, "dia"), (Vista::Semana, "semana")] {
            let lw = self.largura(nome, &fb)? + 20.0 * s;
            let r = ret(x, 0.0, x + lw, topo);
            let sim = c.vista == v;
            if sim {
                self.preencher(ret(r.left, topo - 2.0 * s, r.right, topo), k(t.destaque))?;
            }
            let tinta = if sim { t.destaque } else if hover(AlvoCentral::Vista(v)) { t.texto } else { apagado };
            self.texto(nome, if sim { &fcb } else { &fc }, r, k(tinta))?;
            alvos.push((r, AlvoCentral::Vista(v)));
            x += lw + 2.0 * s;
        }
        // fontes, da direita para a esquerda
        let mut xd = w - 16.0 * s;
        for (i, fo) in d.fontes.iter().enumerate().rev() {
            let rotulo_f = format!("{}  {}", fo.glifo, fo.nome);
            let lw = self.largura(&rotulo_f, &f)? + 16.0 * s;
            let r = ret(xd - lw, 10.0 * s, xd, 34.0 * s);
            let ligada = !c.ocultas.contains(&fo.nome);
            if ligada {
                self.preencher(r, k(t.selecao))?;
            }
            let tinta = if hover(AlvoCentral::Fonte(i)) {
                t.texto_forte
            } else if fo.erro.is_some() {
                t.amarelo
            } else if ligada {
                t.texto
            } else {
                apagado
            };
            self.texto(&rotulo_f, &fc, r, k(tinta))?;
            if !ligada {
                let m = (r.top + r.bottom) / 2.0;
                self.preencher(ret(r.left + 8.0 * s, m, r.right - 8.0 * s, m + 1.0 * s), k(t.mudo))?;
            }
            alvos.push((r, AlvoCentral::Fonte(i)));
            xd = r.left - 4.0 * s;
        }
        x += 18.0 * s;
        if xd > x {
            let rf = ret(x, 0.0, xd - 12.0 * s, topo);
            if c.filtro.is_empty() {
                self.texto("texto para filtrar", &f, rf, k(t.texto_escuro))?;
            } else {
                self.texto(&c.filtro, &f, rf, k(t.amarelo))?;
            }
        }
        self.preencher(ret(0.0, topo - 1.0 * s, w, topo), k(t.selecao))?;

        // ---------- pé ----------
        let borda = 2.0 * s;
        let pe0 = h - borda - 28.0 * s;
        self.preencher(ret(0.0, pe0, w, pe0 + 1.0 * s), k(t.selecao))?;
        let n_fontes = d.fontes.len().max(1);
        let teclas_fontes = if n_fontes == 1 { "1".to_string() } else { format!("1–{n_fontes}") };
        let dicas = [
            ("←→", "dia ou semana anterior e seguinte"),
            ("tab", "alternar dia e semana"),
            ("home", "hoje"),
            (teclas_fontes.as_str(), "fontes"),
            ("texto", "filtrar"),
            ("esc", "fechar"),
        ];
        let mut xp = 16.0 * s;
        for (tecla, o_que) in dicas {
            let lt = self.largura(tecla, &f)?;
            self.texto(tecla, &f, ret(xp, pe0, xp + lt + 2.0, h - borda), k(apagado))?;
            xp += lt + 6.0 * s;
            let lo = self.largura(o_que, &f)?;
            self.texto(o_que, &f, ret(xp, pe0, xp + lo + 2.0, h - borda), k(t.texto_escuro))?;
            xp += lo + 20.0 * s;
        }

        // ---------- colunas ----------
        let y0 = topo;
        let y1 = pe0;
        let c3 = w - borda - 330.0 * s;
        let c1 = if c.vista == Vista::Dia { borda + (c3 - borda) * 0.575 } else { c3 };
        if c.vista == Vista::Dia {
            self.preencher(ret(c1, y0, c1 + 1.0 * s, y1), k(t.selecao))?;
        }
        self.preencher(ret(c3, y0, c3 + 1.0 * s, y1), k(t.selecao))?;

        let pad = 16.0 * s;
        match c.vista {
            Vista::Dia => self.coluna_dia(cena, ret(borda, y0, c1, y1), &mut alvos)?,
            Vista::Semana => self.coluna_semana(cena, ret(borda, y0, c1, y1), &mut alvos)?,
        }

        // ---------- pendências e prazos ----------
        if c.vista == Vista::Dia {
            let (xa, xb) = (c1 + 1.0 * s + pad, c3 - pad);
            let mut y = y0 + 12.0 * s;
            let limite = y1 - 8.0 * s;
            let sinteticas: Vec<usize> = c.sem_execucao_hoje(hoje).into_iter().filter(|&i| !c.ocultas.contains(&d.fontes[i].nome)).collect();
            let sessoes_perm: Vec<usize> = (0..d.sessoes.len())
                .filter(|&i| d.sessoes[i].estado == EstadoSessao::Permissao)
                .filter(|_| d.fontes.iter().position(|f| f.nome == "claude").is_some_and(|i| c.visivel(i, "")))
                .collect();
            let mut pend: Vec<usize> = (0..d.pendencias.len()).filter(|&i| c.visivel(d.pendencias[i].fonte, &d.pendencias[i].titulo)).collect();
            // por prazo; a concluída fica no lugar (riscada), para a lista não mudar embaixo do cursor
            pend.sort_by_key(|&i| d.pendencias[i].prazo.unwrap_or(NaiveDate::MAX));
            let abertas = pend.iter().filter(|&&i| !d.concluida(&d.pendencias[i])).count() + sinteticas.len() + sessoes_perm.len();
            self.texto("Pendências", &fb, ret(xa, y, xb, y + 20.0 * s), k(t.texto_forte))?;
            if abertas > 0 {
                self.texto(&format!("{abertas} em aberto"), &fr, ret(xa, y, xb, y + 20.0 * s), k(apagado))?;
            }
            y += 26.0 * s;
            let linha = 18.0 * s;
            let mut sobra = 0;
            // sessões aguardando permissão
            for &i in &sessoes_perm {
                if y + 2.0 * linha > limite {
                    sobra += 1;
                    continue;
                }
                let se = &d.sessoes[i];
                self.glifo(G_ATENCAO, &fg, ret(xa - 4.0 * s, y, xa + 16.0 * s, y + linha), k(t.amarelo))?;
                self.texto(&format!("Sessão em {} aguarda permissão", se.nome()), &f, ret(xa + 22.0 * s, y, xb - 60.0 * s, y + linha), k(t.texto_forte))?;
                self.texto(&format!("desde {}", hm(se.desde)), &fr, ret(xa, y, xb, y + linha), k(t.amarelo))?;
                let msg = if se.mensagem.is_empty() { se.pasta.clone() } else { se.mensagem.clone() };
                self.texto(&format!("{}  {msg}", crate::central::G_CLAUDE), &f, ret(xa + 22.0 * s, y + linha, xb, y + 2.0 * linha), k(apagado))?;
                y += 2.0 * linha + 10.0 * s;
            }
            // fontes que oferecem execução e não rodaram hoje
            for &i in &sinteticas {
                if y + 3.0 * linha > limite {
                    sobra += 1;
                    continue;
                }
                let fo = &d.fontes[i];
                let ex = fo.executar.as_ref().map(|e| e.rotulo.clone()).unwrap_or_default();
                self.glifo(fo.glifo, &fg, ret(xa - 4.0 * s, y, xa + 16.0 * s, y + linha), k(t.amarelo))?;
                self.texto(&format!("Rotinas do {} sem execução hoje", fo.nome), &f, ret(xa + 22.0 * s, y, xb - 60.0 * s, y + linha), k(t.texto_forte))?;
                self.texto("hoje", &fr, ret(xa, y, xb, y + linha), k(t.amarelo))?;
                self.texto(&fo.descricao, &f, ret(xa + 22.0 * s, y + linha, xb, y + 2.0 * linha), k(apagado))?;
                let lw = self.largura(&ex, &f)? + 4.0 * s;
                let r = ret(xa + 22.0 * s, y + 2.0 * linha + 2.0 * s, xa + 22.0 * s + lw, y + 3.0 * linha + 2.0 * s);
                if hover(AlvoCentral::ExecutarFonte(i)) {
                    self.preencher(ret(r.left, r.bottom - 1.0 * s, r.right, r.bottom), k(t.destaque))?;
                }
                self.texto(&ex, &f, r, k(t.destaque))?;
                alvos.push((r, AlvoCentral::ExecutarFonte(i)));
                y += 3.0 * linha + 12.0 * s;
            }
            for &i in &pend {
                let p = &d.pendencias[i];
                let feita = d.concluida(p);
                // o quadrado marca como concluída; o resto da linha abre o registro
                let executa = !feita && p.executar.is_some();
                let linhas = if executa { 3.0 } else { 2.0 };
                if y + linhas * linha > limite {
                    sobra += 1;
                    continue;
                }
                let lin = ret(xa - 6.0 * s, y - 4.0 * s, xb + 6.0 * s, y + linhas * linha + 4.0 * s);
                let tem_link = !p.link.is_empty();
                if hover(AlvoCentral::Pendencia(i)) || (tem_link && hover(AlvoCentral::LinkPendencia(i))) {
                    self.preencher(lin, k(t.fundo_claro))?;
                }
                let q = 12.0 * s;
                let cx = ret(xa, y + (linha - q) / 2.0, xa + q, y + (linha + q) / 2.0);
                if feita {
                    self.preencher(cx, k(t.verde))?;
                } else {
                    self.borda(cx, k(apagado), 1.0 * s)?;
                }
                let mut fim_tit = xb;
                if let Some(pz) = p.prazo.filter(|_| !feita) {
                    let (q, n) = quando(pz, hoje);
                    let tinta = if n < 0 { t.vermelho } else if n == 0 { t.amarelo } else { apagado };
                    let lq = self.largura(&q, &f)?;
                    self.texto(&q, &fr, ret(xb - lq - 4.0 * s, y, xb, y + linha), k(tinta))?;
                    fim_tit = xb - lq - 14.0 * s;
                }
                let rt = ret(xa + 22.0 * s, y, fim_tit, y + linha);
                self.texto(&p.titulo, &f, rt, k(if feita { t.texto_escuro } else { t.texto_forte }))?;
                if feita {
                    let lw = self.largura(&p.titulo, &f)?.min(rt.right - rt.left);
                    let m = y + linha / 2.0;
                    self.preencher(ret(rt.left, m, rt.left + lw, m + 1.0 * s), k(t.texto_escuro))?;
                }
                let origem = if p.origem.is_empty() { d.fontes[p.fonte].descricao.clone() } else { p.origem.clone() };
                self.texto(&format!("{}  {origem}", fonte_g(p.fonte)), &f, ret(xa + 22.0 * s, y + linha, xb, y + 2.0 * linha), k(apagado))?;
                if executa {
                    let rot = p.executar.as_ref().map(|e| e.rotulo.clone()).unwrap_or_default();
                    let lw = self.largura(&rot, &f)? + 4.0 * s;
                    let r = ret(xa + 22.0 * s, y + 2.0 * linha + 2.0 * s, xa + 22.0 * s + lw, y + 3.0 * linha + 2.0 * s);
                    let al = AlvoCentral::ExecutarPendencia(i);
                    if hover(al) {
                        self.preencher(ret(r.left, r.bottom - 1.0 * s, r.right, r.bottom), k(t.destaque))?;
                    }
                    self.texto(&rot, &f, r, k(t.destaque))?;
                    alvos.push((r, al));
                }
                alvos.push((ret(lin.left, lin.top, xa + 18.0 * s, lin.bottom), AlvoCentral::Pendencia(i)));
                alvos.push((lin, if tem_link { AlvoCentral::LinkPendencia(i) } else { AlvoCentral::Pendencia(i) }));
                y += linhas * linha + 12.0 * s;
            }
            if abertas == 0 && pend.is_empty() {
                self.texto("Nenhuma pendência.", &f, ret(xa, y, xb, y + linha), k(t.texto_escuro))?;
                y += linha + 8.0 * s;
            }
            if sobra > 0 {
                self.texto(&format!("mais {sobra}"), &f, ret(xa + 22.0 * s, y, xb, y + linha), k(apagado))?;
                y += linha + 8.0 * s;
            }

            // prazos: os da fonte e os das pendências abertas, até 60 dias
            if y + 60.0 * s < limite {
                y += 6.0 * s;
                self.preencher(ret(xa, y, xb, y + 1.0 * s), k(t.selecao))?;
                y += 8.0 * s;
                self.texto("prazos", &f, ret(xa, y, xb, y + linha), k(rotulo))?;
                self.texto("próximos 60 dias", &fr, ret(xa, y, xb, y + linha), k(rotulo))?;
                y += linha + 8.0 * s;
                let mut lista: Vec<(NaiveDate, String, usize, Option<usize>)> = d
                    .prazos
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| c.visivel(p.fonte, &p.titulo))
                    .map(|(i, p)| (p.data, p.titulo.clone(), p.fonte, Some(i)))
                    .collect();
                lista.extend(d.pendencias.iter().filter(|p| !d.concluida(p) && c.visivel(p.fonte, &p.titulo)).filter_map(|p| p.prazo.map(|pz| (pz, p.titulo.clone(), p.fonte, None))));
                lista.retain(|(dt, _, _, _)| (*dt - hoje).num_days() <= 60);
                lista.sort_by_key(|(dt, _, _, _)| *dt);
                let alt = 24.0 * s;
                let mut resto = 0;
                for (dt, tit, fo, idx) in &lista {
                    if y + alt > limite {
                        resto += 1;
                        continue;
                    }
                    let r = ret(xa - 6.0 * s, y, xb + 6.0 * s, y + alt);
                    let tem_link = idx.is_some_and(|i| !d.prazos[i].link.is_empty());
                    if tem_link && hover(AlvoCentral::Prazo(idx.unwrap_or(0))) {
                        self.preencher(r, k(t.fundo_claro))?;
                    }
                    let (q, n) = quando(*dt, hoje);
                    let tinta = if n < 0 { t.vermelho } else if n == 0 { t.amarelo } else { apagado };
                    let lq = self.largura(&q, &f)?;
                    self.glifo(fonte_g(*fo), &fg, ret(xa - 4.0 * s, y, xa + 16.0 * s, y + alt), k(apagado))?;
                    self.texto(tit, &f, ret(xa + 22.0 * s, y, xb - lq - 14.0 * s, y + alt), k(t.texto))?;
                    self.texto(&q, &fr, ret(xa, y, xb, y + alt), k(tinta))?;
                    if let (true, Some(i)) = (tem_link, idx) {
                        alvos.push((r, AlvoCentral::Prazo(*i)));
                    }
                    y += alt;
                }
                if lista.is_empty() {
                    self.texto("Nenhum prazo.", &f, ret(xa, y, xb, y + alt), k(t.texto_escuro))?;
                } else if resto > 0 {
                    self.texto(&format!("mais {resto}"), &f, ret(xa + 22.0 * s, y, xb, y + alt), k(apagado))?;
                }
            }
        }

        // ---------- mês, rotinas, sessões, fontes ----------
        let (xa, xb) = (c3 + 1.0 * s + pad, w - borda - pad);
        let mut y = y0 + 12.0 * s;
        let limite = y1 - 8.0 * s;
        let nome_mes = format!("{} {}", cap(mes_nome(c.mes.month())), c.mes.year());
        self.texto(&nome_mes, &fb, ret(xa, y, xb, y + 26.0 * s), k(t.texto_forte))?;
        let rs = ret(xb + 4.0 * s - cel, y, xb + 4.0 * s, y + 26.0 * s);
        let ra = ret(rs.left - cel, y, rs.left, y + 26.0 * s);
        for (r, al, g) in [(ra, AlvoCentral::MesAnterior, "\u{f104}"), (rs, AlvoCentral::MesSeguinte, "\u{f105}")] {
            if hover(al) {
                self.preencher(r, k(t.fundo_claro))?;
            }
            self.glifo(g, &fg, r, k(if hover(al) { t.texto_forte } else { t.texto }))?;
            alvos.push((r, al));
        }
        y += 30.0 * s;
        self.preencher(ret(xa, y, xb, y + 1.0 * s), k(t.selecao))?;
        let col = (xb - xa) / 7.0;
        for (i, l) in INICIAIS.iter().enumerate() {
            let xx = xa + col * i as f32;
            self.texto(l, &fc, ret(xx, y + 2.0 * s, xx + col, y + 24.0 * s), k(rotulo))?;
        }
        y += 24.0 * s;
        let primeiro = inicio_semana(c.mes);
        let alt_dia = 26.0 * s;
        for k_dia in 0..42 {
            let dd = primeiro + Duration::days(k_dia);
            let (lin, colu) = ((k_dia / 7) as f32, (k_dia % 7) as f32);
            let r = ret(xa + col * colu, y + alt_dia * lin, xa + col * (colu + 1.0), y + alt_dia * (lin + 1.0));
            let al = AlvoCentral::Dia(dd);
            if hover(al) {
                self.preencher(r, k(t.fundo_claro))?;
            }
            if c.vista == Vista::Dia && dd == c.dia && dd != hoje {
                self.borda(r, k(t.mudo), 1.0 * s)?;
            }
            let fora = dd.month() != c.mes.month();
            let fds = matches!(dia_semana(dd), 0 | 6);
            let n = dd.day().to_string();
            if dd == hoje {
                self.texto(&n, &fcb, r, k(t.destaque))?;
                let cx = (r.left + r.right) / 2.0;
                self.preencher(ret(cx - 8.0 * s, r.bottom - 4.0 * s, cx + 8.0 * s, r.bottom - 2.0 * s), k(t.destaque))?;
            } else {
                let tinta = if fora { t.texto_escuro } else if fds { apagado } else { t.texto };
                self.texto(&n, &fc, r, k(tinta))?;
            }
            let tem = d.eventos.iter().any(|e| c.visivel(e.fonte, &e.titulo) && e.inicio.date() <= dd && (dd < e.fim.date() || e.inicio.date() == dd))
                || d.execucoes.iter().any(|e| c.visivel(e.fonte, &e.titulo) && e.inicio.date() == dd);
            if tem {
                let falha = d.execucoes.iter().any(|e| c.visivel(e.fonte, &e.titulo) && e.inicio.date() == dd && e.estado == Estado::Falha);
                let p = 4.0 * s;
                self.preencher(ret(r.right - 8.0 * s, r.top + 4.0 * s, r.right - 8.0 * s + p, r.top + 4.0 * s + p), k(if falha { t.vermelho } else { apagado }))?;
            }
            alvos.push((r, al));
        }
        y += alt_dia * 6.0 + 10.0 * s;

        // rotinas
        let rot: Vec<usize> = (0..d.rotinas.len()).filter(|&i| c.visivel(d.rotinas[i].fonte, &d.rotinas[i].titulo)).collect();
        let linha = 18.0 * s;
        if !rot.is_empty() && y + 3.0 * linha < limite {
            self.preencher(ret(xa, y, xb, y + 1.0 * s), k(t.selecao))?;
            y += 8.0 * s;
            self.texto("rotinas", &f, ret(xa, y, xb, y + linha), k(rotulo))?;
            let falhas = rot.iter().filter(|&&i| d.rotinas[i].ultima.is_some_and(|(_, e)| e == Estado::Falha)).count();
            if falhas > 0 {
                self.texto(&format!("{falhas} com falha"), &fr, ret(xa, y, xb, y + linha), k(t.vermelho))?;
            } else {
                self.texto(&rot.len().to_string(), &fr, ret(xa, y, xb, y + linha), k(rotulo))?;
            }
            y += linha + 6.0 * s;
            let mut resto = 0;
            for &i in &rot {
                let ro = &d.rotinas[i];
                if y + 2.0 * linha > limite {
                    resto += 1;
                    continue;
                }
                let est = ro.ultima.map(|(_, e)| e).unwrap_or(Estado::Agendada);
                self.glifo(est.glifo(), &fg, ret(xa - 4.0 * s, y, xa + 14.0 * s, y + linha), k(tinta_estado(est)))?;
                let prox = ro.proxima.map(|p| quando(p, hoje).0).unwrap_or_default();
                let lp = if prox.is_empty() { 0.0 } else { self.largura(&prox, &f)? + 10.0 * s };
                self.texto(&ro.titulo, &f, ret(xa + 20.0 * s, y, xb - lp, y + linha), k(t.texto))?;
                self.texto(&prox, &fr, ret(xa, y, xb, y + linha), k(apagado))?;
                let fo = d.fontes.get(ro.fonte).map(|f| f.nome.as_str()).unwrap_or("");
                let sub = if ro.quando.is_empty() { format!("{}  {fo}", fonte_g(ro.fonte)) } else { format!("{}  {fo} · {}", fonte_g(ro.fonte), ro.quando) };
                let mut fim_sub = xb;
                if let Some(ex) = &ro.executar {
                    let lw = self.largura(&ex.rotulo, &f)? + 4.0 * s;
                    let r = ret(xb - lw, y + linha, xb, y + 2.0 * linha);
                    if hover(AlvoCentral::Rotina(i)) {
                        self.preencher(ret(r.left, r.bottom - 1.0 * s, r.right, r.bottom), k(t.destaque))?;
                    }
                    self.texto(&ex.rotulo, &fr, r, k(t.destaque))?;
                    alvos.push((r, AlvoCentral::Rotina(i)));
                    fim_sub = r.left - 10.0 * s;
                }
                self.texto(&sub, &f, ret(xa + 20.0 * s, y + linha, fim_sub, y + 2.0 * linha), k(apagado))?;
                y += 2.0 * linha + 8.0 * s;
            }
            if resto > 0 {
                self.texto(&format!("mais {resto}"), &f, ret(xa + 20.0 * s, y, xb, y + linha), k(apagado))?;
                y += linha + 6.0 * s;
            }
        }

        // sessões do Claude
        let claude_visivel = d.fontes.iter().position(|f| f.nome == "claude").is_some_and(|i| c.visivel(i, ""));
        if claude_visivel && !d.sessoes.is_empty() && y + 3.0 * linha < limite {
            self.preencher(ret(xa, y, xb, y + 1.0 * s), k(t.selecao))?;
            y += 8.0 * s;
            self.texto("sessões do Claude", &f, ret(xa, y, xb, y + linha), k(rotulo))?;
            y += linha + 6.0 * s;
            for se in &d.sessoes {
                if y + linha > limite {
                    break;
                }
                let (g, tinta, estado) = match se.estado {
                    EstadoSessao::Trabalhando => ("\u{f110}", t.amarelo, format!("em execução há {}", duracao((cena.agora - se.inicio).num_minutes().max(0)))),
                    EstadoSessao::Permissao => (G_ATENCAO, t.amarelo, "aguarda permissão".to_string()),
                    EstadoSessao::Concluida => ("\u{f00c}", t.verde, format!("concluída às {}", hm(se.desde))),
                };
                self.glifo(g, &fg, ret(xa - 4.0 * s, y, xa + 14.0 * s, y + linha), k(tinta))?;
                let le = self.largura(&estado, &f)? + 10.0 * s;
                self.texto(se.nome(), &f, ret(xa + 20.0 * s, y, xb - le, y + linha), k(t.texto))?;
                self.texto(&estado, &fr, ret(xa, y, xb, y + linha), k(apagado))?;
                y += linha + 6.0 * s;
            }
        }

        // fontes com problema
        let problemas: Vec<&crate::central::Fonte> = d.fontes.iter().filter(|f| f.erro.is_some()).collect();
        if !problemas.is_empty() && y + 4.0 * linha < limite {
            y += 4.0 * s;
            self.preencher(ret(xa, y, xb, y + 1.0 * s), k(t.selecao))?;
            y += 8.0 * s;
            self.texto("fontes", &f, ret(xa, y, xb, y + linha), k(rotulo))?;
            y += linha + 6.0 * s;
            for fo in problemas {
                if y + 3.0 * linha > limite {
                    break;
                }
                self.glifo(fo.glifo, &fg, ret(xa - 4.0 * s, y, xa + 14.0 * s, y + linha), k(t.amarelo))?;
                self.texto(&fo.descricao, &f, ret(xa + 20.0 * s, y, xb, y + linha), k(t.texto))?;
                let quando_ok = fo.atualizado.map(|a| format!(" Última sincronização: {} {}.", a.format("%d/%m"), hm(a))).unwrap_or_default();
                let msg = format!("{}{quando_ok}", fo.erro.clone().unwrap_or_default());
                let alt = (limite - y - linha).min(4.0 * linha);
                self.texto(&msg, &fq, ret(xa + 20.0 * s, y + linha + 2.0 * s, xb, y + linha + alt), k(apagado))?;
                y += linha + alt + 8.0 * s;
            }
        }

        // borda: lados e embaixo (em cima fica a barra)
        let b = k(t.destaque);
        self.preencher(ret(0.0, 0.0, borda, h), b)?;
        self.preencher(ret(w - borda, 0.0, w, h), b)?;
        self.preencher(ret(0.0, h - borda, w, h), b)?;
        Ok(alvos)
    }

    /// Linha do tempo do dia: compromissos à esquerda, execuções à direita, hora atual em
    /// `destaque` e o detalhe do item escolhido preso no pé da coluna.
    fn coluna_dia(&mut self, cena: &CenaCentral, area: D2D_RECT_F, alvos: &mut AlvosCentral) -> Result<()> {
        let t = self.tema;
        let s = cena.escala;
        let c = cena.c;
        let d = &c.dados;
        let hoje = cena.agora.date();
        let hover = |a: AlvoCentral| c.hover == Some(a);
        let k = |x: u32| cor(x, 1.0);
        let apagado = mistura(t.texto, t.fundo, 0.3);
        let rotulo = mistura(t.texto, t.fundo, 0.45);
        let grade = mistura(t.fundo, t.selecao, 0.55);
        let f = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let fb = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let fr = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let frb = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let fg = self.formato(13.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let fq = self.formato_quebra(12.0 * s)?;
        let tinta_estado = |e: Estado| match e {
            Estado::Ok => t.verde,
            Estado::Falha => t.vermelho,
            Estado::Executando => t.amarelo,
            Estado::Agendada => t.texto_escuro,
        };

        let pad = 16.0 * s;
        let (xa, xb) = (area.left + pad, area.right - pad);
        let mut y = area.top + 12.0 * s;
        let dia = c.dia;
        let evs: Vec<usize> = (0..d.eventos.len())
            .filter(|&i| {
                let e = &d.eventos[i];
                let no_dia = if e.dia_inteiro { e.inicio.date() <= dia && dia < e.fim.date() } else { e.inicio.date() == dia };
                no_dia && c.visivel(e.fonte, &e.titulo)
            })
            .collect();
        let (inteiros, evs): (Vec<usize>, Vec<usize>) = evs.into_iter().partition(|&i| d.eventos[i].dia_inteiro);
        let exs: Vec<usize> = (0..d.execucoes.len()).filter(|&i| d.execucoes[i].inicio.date() == dia && c.visivel(d.execucoes[i].fonte, &d.execucoes[i].titulo)).collect();

        let titulo = if dia == hoje { "Hoje".to_string() } else { crate::DIAS[dia_semana(dia)].to_string() };
        self.texto(&titulo, &fb, ret(xa, y, xb, y + 20.0 * s), k(t.texto_forte))?;
        let n_ev = evs.len() + inteiros.len();
        let resumo = format!(
            "{} compromisso{} · {} execuç{}",
            n_ev,
            if n_ev == 1 { "" } else { "s" },
            exs.len(),
            if exs.len() == 1 { "ão" } else { "ões" }
        );
        self.texto(&resumo, &fr, ret(xa, y, xb, y + 20.0 * s), k(apagado))?;
        y += 26.0 * s;

        // compromissos do dia inteiro, acima da grade
        let col_h = 44.0 * s;
        let lx0 = xa + col_h;
        let meio = lx0 + (xb - lx0) * 0.55;
        for &i in inteiros.iter().take(3) {
            let e = &d.eventos[i];
            let r = ret(lx0 + 6.0 * s, y, meio - 6.0 * s, y + 22.0 * s);
            let sel = c.sel == Some(Sel::Evento(i));
            self.preencher(r, k(if sel { t.selecao } else { t.fundo_claro }))?;
            self.borda(r, k(if sel || hover(AlvoCentral::Evento(i)) { t.texto } else { t.mudo }), 1.0 * s)?;
            self.texto(&e.titulo, &fb, ret(r.left + 8.0 * s, r.top, r.right - 6.0 * s, r.bottom), k(t.texto_forte))?;
            self.texto("dia inteiro", &f, ret(xa, r.top, lx0, r.bottom), k(t.texto_escuro))?;
            alvos.push((r, AlvoCentral::Evento(i)));
            y += 26.0 * s;
        }

        self.texto("agenda", &f, ret(lx0 + 8.0 * s, y, meio, y + 18.0 * s), k(rotulo))?;
        self.texto("rotinas", &f, ret(meio + 8.0 * s, y, xb, y + 18.0 * s), k(rotulo))?;
        y += 22.0 * s;

        // detalhe
        let sel = c.sel.filter(|sl| match *sl {
            Sel::Evento(i) => evs.contains(&i) || inteiros.contains(&i),
            Sel::Execucao(i) => exs.contains(&i),
        });
        // detalhe: cabeçalho, botões e o texto (linhas estimadas pela largura da coluna)
        let corpo_sel = match sel {
            Some(Sel::Evento(i)) => d.eventos[i].descricao.as_str(),
            Some(Sel::Execucao(i)) => d.execucoes[i].resumo.as_str(),
            None => "",
        };
        let por_linha = (((xb - xa - 20.0 * s) / (7.3 * s)) as usize).max(20);
        let n_linhas: usize = corpo_sel.lines().map(|l| l.chars().count().div_ceil(por_linha).max(1)).sum();
        let alt_corpo = if corpo_sel.is_empty() { 0.0 } else { (n_linhas.min(10) as f32 * 16.0 + 22.0) * s };
        let alt_det = if sel.is_some() { 108.0 * s + alt_corpo } else { 0.0 };
        let ty0 = y + 8.0 * s;
        let ty1 = area.bottom - 12.0 * s - alt_det;

        // horas: 7 às 23, ampliado se algum item cai fora
        let mut h0 = 7u32;
        let mut h1 = 23u32;
        for &i in &evs {
            h0 = h0.min(d.eventos[i].inicio.hour());
            let fim = d.eventos[i].fim;
            let hf = if fim.date() > dia { 24 } else { fim.hour() + (fim.minute() > 0) as u32 };
            h1 = h1.max(hf);
        }
        for &i in &exs {
            h0 = h0.min(d.execucoes[i].inicio.hour());
            h1 = h1.max(d.execucoes[i].inicio.hour() + 1);
        }
        let h1 = h1.min(24);
        let px = ((ty1 - ty0) / (h1 - h0).max(1) as f32).max(8.0 * s);
        let y_de = |dt: NaiveDateTime| {
            let m = if dt.date() > dia { 24 * 60 } else if dt.date() < dia { 0 } else { dt.hour() * 60 + dt.minute() } as f32;
            ty0 + (m - h0 as f32 * 60.0) / 60.0 * px
        };
        let fhora = self.formato(11.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        for hh in h0..=h1 {
            let yy = ty0 + (hh - h0) as f32 * px;
            self.preencher(ret(lx0, yy, xb, yy + 1.0 * s), k(grade))?;
            if hh < h1 {
                self.texto(&format!("{hh:02}"), &fhora, ret(xa, yy - 8.0 * s, lx0 - 8.0 * s, yy + 8.0 * s), k(t.texto_escuro))?;
            }
        }
        self.preencher(ret(lx0, ty0, lx0 + 1.0 * s, ty0 + (h1 - h0) as f32 * px), k(t.selecao))?;
        self.preencher(ret(meio, ty0, meio + 1.0 * s, ty0 + (h1 - h0) as f32 * px), k(t.selecao))?;

        // compromissos: os que se sobrepõem dividem a largura
        let mut colunas: Vec<(usize, usize, usize)> = Vec::new(); // (evento, coluna, total do grupo)
        let mut grupo: Vec<(usize, usize)> = Vec::new();
        let mut fim_grupo: Option<NaiveDateTime> = None;
        let fechar = |grupo: &mut Vec<(usize, usize)>, colunas: &mut Vec<(usize, usize, usize)>| {
            let total = grupo.iter().map(|(_, c)| c + 1).max().unwrap_or(1);
            colunas.extend(grupo.drain(..).map(|(e, c)| (e, c, total)));
        };
        for &i in &evs {
            let e = &d.eventos[i];
            if fim_grupo.is_some_and(|fg| e.inicio >= fg) {
                fechar(&mut grupo, &mut colunas);
                fim_grupo = None;
            }
            let livre = (0..).find(|cc| !grupo.iter().any(|&(j, c2)| c2 == *cc && d.eventos[j].fim > e.inicio)).unwrap_or(0);
            grupo.push((i, livre));
            fim_grupo = Some(fim_grupo.map_or(e.fim, |fg| fg.max(e.fim)));
        }
        fechar(&mut grupo, &mut colunas);
        let agora = cena.agora;
        for (i, cc, total) in colunas {
            let e = &d.eventos[i];
            let larg = (meio - lx0 - 12.0 * s) / total as f32;
            let left = lx0 + 6.0 * s + larg * cc as f32;
            let top = y_de(e.inicio) + 1.0 * s;
            let bottom = (y_de(e.fim) - 1.0 * s).max(top + 18.0 * s);
            let r = ret(left, top, left + larg - 2.0 * s, bottom);
            let escolhido = c.sel == Some(Sel::Evento(i));
            let acontecendo = dia == hoje && e.inicio <= agora && agora < e.fim;
            let passou = dia < hoje || (dia == hoje && e.fim <= agora);
            self.preencher(r, k(if escolhido { t.selecao } else { t.fundo_claro }))?;
            let (borda_c, esp) = if acontecendo {
                (t.destaque, 2.0 * s)
            } else if escolhido || hover(AlvoCentral::Evento(i)) {
                (t.texto, 1.0 * s)
            } else {
                (t.mudo, 1.0 * s)
            };
            self.borda(r, k(borda_c), esp)?;
            let tinta = if passou { apagado } else { t.texto_forte };
            let alt = r.bottom - r.top;
            let (tl, tr) = (r.left + 8.0 * s, r.right - 6.0 * s);
            self.texto(&e.titulo, &fb, ret(tl, r.top, tr, (r.top + 20.0 * s).min(r.bottom)), k(tinta))?;
            if alt > 36.0 * s {
                let det = if e.local.is_empty() { format!("{}–{}", hm(e.inicio), hm(e.fim)) } else { format!("{}–{} · {}", hm(e.inicio), hm(e.fim), e.local) };
                self.texto(&det, &f, ret(tl, r.top + 18.0 * s, tr, r.top + 36.0 * s), k(apagado))?;
            }
            alvos.push((r, AlvoCentral::Evento(i)));
        }

        // hora atual (antes das execuções, que ficam por cima dela)
        if dia == hoje && agora.hour() >= h0 && agora.hour() < h1 {
            let yy = y_de(agora);
            self.preencher(ret(lx0, yy - 1.0 * s, xb, yy + 1.0 * s), k(t.destaque))?;
            let rh = ret(xa - 4.0 * s, yy - 9.0 * s, lx0 - 4.0 * s, yy + 9.0 * s);
            self.preencher(rh, k(t.fundo))?;
            self.texto(&hm(agora), &frb, ret(rh.left, rh.top, lx0 - 8.0 * s, rh.bottom), k(t.destaque))?;
        }
        // execuções: uma linha cada, empurradas para baixo quando caem juntas
        let alt_ex = 20.0 * s;
        let mut ult = f32::MIN;
        for &i in &exs {
            let e = &d.execucoes[i];
            let mut top = y_de(e.inicio) - alt_ex / 2.0;
            if top < ult + alt_ex + 2.0 * s {
                top = ult + alt_ex + 2.0 * s;
            }
            ult = top;
            let r = ret(meio + 6.0 * s, top, xb, top + alt_ex);
            let realce = c.sel == Some(Sel::Execucao(i)) || hover(AlvoCentral::Execucao(i));
            self.preencher(r, k(if realce { t.fundo_claro } else { t.fundo }))?;
            self.glifo(e.estado.glifo(), &fg, ret(r.left + 2.0 * s, top, r.left + 20.0 * s, top + alt_ex), k(tinta_estado(e.estado)))?;
            let hora = hm(e.inicio);
            let lh = self.largura(&hora, &f)?;
            self.texto(&hora, &f, ret(r.left + 24.0 * s, top, r.left + 24.0 * s + lh + 2.0, top + alt_ex), k(apagado))?;
            self.texto(&e.titulo, &f, ret(r.left + 32.0 * s + lh, top, r.right - 4.0 * s, top + alt_ex), k(t.texto))?;
            alvos.push((r, AlvoCentral::Execucao(i)));
        }

        if evs.is_empty() && exs.is_empty() && inteiros.is_empty() {
            let filtrado = !c.filtro.is_empty() || !c.ocultas.is_empty();
            let msg = if filtrado { "Nenhum item neste dia com os filtros atuais." } else { "Nenhum item neste dia." };
            self.texto(msg, &f, ret(lx0 + 8.0 * s, ty0 + 6.0 * s, xb, ty0 + 26.0 * s), k(t.texto_escuro))?;
        }

        // detalhe no pé
        if let Some(sl) = sel {
            let top = area.bottom - alt_det;
            let r = ret(area.left, top, area.right, area.bottom);
            self.preencher(r, k(t.fundo))?;
            self.preencher(ret(r.left, top, r.right, top + 2.0 * s), k(t.destaque))?;
            let mut yy = top + 12.0 * s;
            let mut botoes: Vec<(u8, String)> = Vec::new();
            let (tit, dir, dir_tinta, meta, corpo) = match sl {
                Sel::Evento(i) => {
                    let e = &d.eventos[i];
                    let fo = &d.fontes[e.fonte];
                    let quando = if e.dia_inteiro { "dia inteiro".to_string() } else { format!("{}–{}", hm(e.inicio), hm(e.fim)) };
                    let meta = if e.local.is_empty() { format!("{}  {}", fo.glifo, fo.descricao) } else { format!("{}  {} · {}", fo.glifo, fo.descricao, e.local) };
                    if !e.link.is_empty() {
                        botoes.push((0, "abrir no Google Agenda".into()));
                    }
                    if !e.reuniao.is_empty() {
                        botoes.push((1, "entrar na reunião".into()));
                    }
                    (e.titulo.clone(), quando, apagado, meta, e.descricao.clone())
                }
                Sel::Execucao(i) => {
                    let e = &d.execucoes[i];
                    let fo = &d.fontes[e.fonte];
                    let dur = match (e.fim, e.estado) {
                        (Some(fim), _) => format!(" · {}", duracao((fim - e.inicio).num_minutes().max(0))),
                        (None, Estado::Executando) if e.inicio.date() == hoje => format!(" · {} até agora", duracao((agora - e.inicio).num_minutes().max(0))),
                        _ => String::new(),
                    };
                    let meta = format!("{}  {} · {}{dur}", fo.glifo, fo.descricao, hm(e.inicio));
                    if !e.registro.is_empty() {
                        botoes.push((0, "abrir registro".into()));
                    }
                    if let Some(ex) = e.executar.as_ref().or(fo.executar.as_ref()) {
                        let r = if e.executar.is_some() { ex.rotulo.clone() } else if e.estado == Estado::Agendada { "executar agora".into() } else { "executar novamente".into() };
                        botoes.push((2, r));
                    }
                    (e.titulo.clone(), format!("{}  {}", e.estado.glifo(), e.estado.rotulo()), tinta_estado(e.estado), meta, e.resumo.clone())
                }
            };
            let ld = self.largura(&dir, &f)?;
            self.texto(&tit, &fb, ret(xa, yy, xb - ld - 16.0 * s, yy + 20.0 * s), k(t.texto_forte))?;
            self.texto(&dir, &fr, ret(xa, yy, xb, yy + 20.0 * s), k(dir_tinta))?;
            yy += 20.0 * s;
            self.texto(&meta, &f, ret(xa, yy, xb, yy + 18.0 * s), k(apagado))?;
            yy += 24.0 * s;
            let base_botoes = area.bottom - 12.0 * s - 24.0 * s;
            if !corpo.is_empty() {
                let rc = ret(xa, yy, xb, base_botoes - 10.0 * s);
                self.preencher(rc, k(mistura(t.fundo, 0x000000, 0.25)))?;
                self.texto(&corpo, &fq, ret(rc.left + 10.0 * s, rc.top + 6.0 * s, rc.right - 10.0 * s, rc.bottom - 6.0 * s), k(t.texto))?;
            }
            let mut xx = xa;
            for (id, rot) in botoes {
                let lw = self.largura(&rot, &f)? + 20.0 * s;
                let rb = ret(xx, base_botoes, xx + lw, base_botoes + 24.0 * s);
                let al = AlvoCentral::Acao(id);
                self.preencher(rb, k(if hover(al) { t.mudo } else { t.selecao }))?;
                let fc = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
                self.texto(&rot, &fc, rb, k(if hover(al) { t.texto_forte } else { t.texto }))?;
                alvos.push((rb, al));
                xx = rb.right + 6.0 * s;
            }
            // clique no resto do detalhe não atravessa para a grade
            alvos.push((r, AlvoCentral::FecharDetalhe));
        }
        Ok(())
    }

    /// Semana do domingo ao sábado: compromissos com hora e título, execuções resumidas.
    fn coluna_semana(&mut self, cena: &CenaCentral, area: D2D_RECT_F, alvos: &mut AlvosCentral) -> Result<()> {
        let t = self.tema;
        let s = cena.escala;
        let c = cena.c;
        let d = &c.dados;
        let hoje = cena.agora.date();
        let k = |x: u32| cor(x, 1.0);
        let apagado = mistura(t.texto, t.fundo, 0.3);
        let f = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let fb = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let fr = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let frb = self.formato(12.0 * s, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let fg = self.formato(13.0 * s, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let pad = 16.0 * s;
        let (xa, xb) = (area.left + pad, area.right - pad);
        let mut y = area.top + 12.0 * s;
        self.texto("Semana", &fb, ret(xa, y, xb, y + 20.0 * s), k(t.texto_forte))?;
        self.texto("selecione um dia para detalhar", &fr, ret(xa, y, xb, y + 20.0 * s), k(apagado))?;
        y += 28.0 * s;
        self.preencher(ret(xa, y, xb, y + 1.0 * s), k(t.selecao))?;
        let ini = inicio_semana(c.dia);
        let col = (xb - xa) / 7.0;
        let fundo_dia = area.bottom - 12.0 * s;
        for kd in 0..7 {
            let dd = ini + Duration::days(kd);
            let x0 = xa + col * kd as f32;
            let r = ret(x0, y, x0 + col, fundo_dia);
            let al = AlvoCentral::Dia(dd);
            if c.hover == Some(al) {
                self.preencher(r, k(mistura(t.fundo, t.fundo_claro, 0.5)))?;
            }
            if kd > 0 {
                self.preencher(ret(x0, y, x0 + 1.0 * s, fundo_dia), k(t.selecao))?;
            }
            let (ca, cb) = (x0 + 6.0 * s, x0 + col - 6.0 * s);
            let mut yy = y + 6.0 * s;
            let eh_hoje = dd == hoje;
            self.texto(DIAS_C[dia_semana(dd)], if eh_hoje { &fb } else { &f }, ret(ca, yy, cb, yy + 20.0 * s), k(if eh_hoje { t.destaque } else { apagado }))?;
            let n = dd.day().to_string();
            self.texto(&n, if eh_hoje { &frb } else { &fr }, ret(ca, yy, cb, yy + 20.0 * s), k(if eh_hoje { t.destaque } else { apagado }))?;
            if eh_hoje {
                let ln = self.largura(&n, &frb)?;
                self.preencher(ret(cb - ln, yy + 18.0 * s, cb, yy + 20.0 * s), k(t.destaque))?;
            }
            yy += 28.0 * s;
            let evs: Vec<&crate::central::Evento> = d
                .eventos
                .iter()
                .filter(|e| c.visivel(e.fonte, &e.titulo))
                .filter(|e| if e.dia_inteiro { e.inicio.date() <= dd && dd < e.fim.date() } else { e.inicio.date() == dd })
                .collect();
            let exs: Vec<&crate::central::Execucao> = d.execucoes.iter().filter(|e| e.inicio.date() == dd && c.visivel(e.fonte, &e.titulo)).collect();
            let mut resto = 0;
            for e in &evs {
                let alt = if e.dia_inteiro { 22.0 * s } else { 40.0 * s };
                if yy + alt > fundo_dia - 24.0 * s {
                    resto += 1;
                    continue;
                }
                let rb = ret(ca, yy, cb, yy + alt);
                let passou = e.fim <= cena.agora;
                self.preencher(rb, k(t.fundo_claro))?;
                self.borda(rb, k(t.mudo), 1.0 * s)?;
                let tinta = if passou { apagado } else { t.texto_forte };
                if e.dia_inteiro {
                    self.texto(&e.titulo, &f, ret(rb.left + 6.0 * s, rb.top, rb.right - 4.0 * s, rb.bottom), k(tinta))?;
                } else {
                    self.texto(&hm(e.inicio), &f, ret(rb.left + 6.0 * s, rb.top + 2.0 * s, rb.right - 4.0 * s, rb.top + 20.0 * s), k(apagado))?;
                    self.texto(&e.titulo, &f, ret(rb.left + 6.0 * s, rb.top + 19.0 * s, rb.right - 4.0 * s, rb.bottom - 2.0 * s), k(tinta))?;
                }
                yy += alt + 4.0 * s;
            }
            if resto > 0 {
                self.texto(&format!("mais {resto}"), &f, ret(ca, yy, cb, yy + 18.0 * s), k(apagado))?;
                yy += 22.0 * s;
            }
            if !exs.is_empty() {
                let falhas = exs.iter().filter(|e| e.estado == Estado::Falha).count();
                let (g, tinta) = if falhas > 0 { (Estado::Falha.glifo(), t.vermelho) } else { (Estado::Ok.glifo(), apagado) };
                self.glifo(g, &fg, ret(ca - 2.0 * s, yy, ca + 14.0 * s, yy + 18.0 * s), k(tinta))?;
                let txt = if falhas > 0 { format!("{} · {falhas} com falha", exs.len()) } else { format!("{} execuç{}", exs.len(), if exs.len() == 1 { "ão" } else { "ões" }) };
                self.texto(&txt, &f, ret(ca + 18.0 * s, yy, cb, yy + 18.0 * s), k(apagado))?;
            } else if evs.is_empty() {
                self.texto("—", &f, ret(ca, yy, cb, yy + 18.0 * s), k(t.texto_escuro))?;
            }
            alvos.push((r, al));
        }
        Ok(())
    }
}
