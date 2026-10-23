//! Central: agenda, rotinas, pendências e sessões do Claude numa janela aberta pelo relógio.
//!
//! Cada fonte grava um JSON em `%LOCALAPPDATA%\barra\central\<fonte>.json` (formato no
//! README); as sessões do Claude ficam uma por arquivo em `central\claude\`. Aqui ficam a
//! leitura dessas pastas, o que foi marcado como concluído (`acoes.jsonl`, lido de volta pelas
//! fontes) e o estado da janela. O desenho é do `desenho::central`.

use std::cell::Cell;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime};
use serde_json::{json, Value};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

pub const G_AGENDA: &str = "\u{f073}";
pub const G_SOUL: &str = "\u{f02a0}";
pub const G_CLAUDE: &str = "\u{f06a9}";
pub const G_AGENTES: &str = "\u{f0a4d}";
pub const G_FONTE: &str = "\u{f0214}";

pub fn pasta() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("barra").join("central")
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Estado {
    Ok,
    Falha,
    Executando,
    Agendada,
}

impl Estado {
    fn de(s: &str) -> Estado {
        match s {
            "ok" | "concluida" | "concluída" => Estado::Ok,
            "falha" | "falhou" | "erro" | "error" | "timeout" => Estado::Falha,
            "executando" | "rodando" | "running" => Estado::Executando,
            _ => Estado::Agendada,
        }
    }

    pub fn glifo(self) -> &'static str {
        match self {
            Estado::Ok => "\u{f00c}",
            Estado::Falha => "\u{f00d}",
            Estado::Executando => "\u{f110}",
            Estado::Agendada => "\u{f017}",
        }
    }

    pub fn rotulo(self) -> &'static str {
        match self {
            Estado::Ok => "concluída",
            Estado::Falha => "com falha",
            Estado::Executando => "em execução",
            Estado::Agendada => "agendada",
        }
    }
}

/// Comando que a fonte oferece (executar as rotinas, executar uma tarefa).
#[derive(Clone, Debug)]
pub struct Executar {
    pub rotulo: String,
    pub pasta: String,
    pub comando: String,
}

impl Executar {
    fn de(v: &Value, rotulo_padrao: &str) -> Option<Executar> {
        let comando = v["comando"].as_str()?.trim().to_string();
        if comando.is_empty() {
            return None;
        }
        Some(Executar {
            rotulo: v["rotulo"].as_str().unwrap_or(rotulo_padrao).to_string(),
            pasta: v["pasta"].as_str().unwrap_or("").to_string(),
            comando,
        })
    }
}

pub struct Fonte {
    pub nome: String,
    pub descricao: String,
    pub glifo: &'static str,
    pub atualizado: Option<NaiveDateTime>,
    pub erro: Option<String>,
    pub executar: Option<Executar>,
}

pub struct Evento {
    pub fonte: usize,
    pub titulo: String,
    pub inicio: NaiveDateTime,
    pub fim: NaiveDateTime,
    pub dia_inteiro: bool,
    pub local: String,
    pub descricao: String,
    pub link: String,
    pub reuniao: String,
}

pub struct Execucao {
    pub fonte: usize,
    pub titulo: String,
    pub inicio: NaiveDateTime,
    pub fim: Option<NaiveDateTime>,
    pub estado: Estado,
    pub resumo: String,
    pub registro: String,
    pub executar: Option<Executar>,
}

pub struct Pendencia {
    pub fonte: usize,
    pub id: String,
    pub titulo: String,
    pub origem: String,
    pub prazo: Option<NaiveDate>,
    pub link: String,
    pub executar: Option<Executar>,
}

/// Pendência marcada como concluída na central, guardada em `concluidas.jsonl` para continuar
/// na aba de concluídas depois que a fonte a tira do arquivo dela.
#[derive(Clone)]
pub struct Concluida {
    pub fonte: String,
    pub id: String,
    pub titulo: String,
    pub origem: String,
    pub link: String,
    pub quando: NaiveDateTime,
}

pub struct Prazo {
    pub fonte: usize,
    pub titulo: String,
    pub data: NaiveDate,
    pub link: String,
}

pub struct Rotina {
    pub fonte: usize,
    pub titulo: String,
    pub quando: String,
    pub ultima: Option<(NaiveDate, Estado)>,
    pub proxima: Option<NaiveDate>,
    pub executar: Option<Executar>,
}

/// Grupos de rotina na central, na ordem em que aparecem.
pub const FREQUENCIAS: [&str; 5] = ["diárias", "semanais", "mensais", "anuais", "outras"];

impl Rotina {
    /// Índice em `FREQUENCIAS`, pelo texto de `quando` ("diária", "mensal, dia 10")
    /// ou pela expressão cron do claude-hooks ("cron 0 8 * * 1-5").
    pub fn frequencia(&self) -> usize {
        let q = self.quando.to_lowercase();
        for (i, p) in ["diári", "semana", "mensa", "anua"].iter().enumerate() {
            if q.starts_with(p) {
                return i;
            }
        }
        if let Some(cron) = q.strip_prefix("cron ") {
            let c: Vec<&str> = cron.split(" · ").next().unwrap_or("").split_whitespace().collect();
            if c.len() >= 5 {
                let livre = |x: &str| x == "*" || x == "?";
                return match (livre(c[2]), livre(c[3]), livre(c[4])) {
                    (true, true, true) => 0,
                    (true, true, false) => if c[4] == "1-5" || c[4] == "mon-fri" { 0 } else { 1 },
                    (false, true, _) => 2,
                    (false, false, _) => 3,
                    _ => 4,
                };
            }
        }
        4
    }

    /// `quando` sem a palavra do grupo ("mensal, dia 10" → "dia 10").
    pub fn detalhe_quando(&self) -> &str {
        let q = self.quando.as_str();
        let l = q.to_lowercase();
        for p in ["diária", "diario", "diário", "semanal", "mensal", "anual"] {
            if l.starts_with(p) {
                return q[p.len()..].trim_start_matches([',', ' ']);
            }
        }
        q
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum EstadoSessao {
    Trabalhando,
    Permissao,
    Concluida,
}

pub struct Sessao {
    pub pasta: String,
    pub estado: EstadoSessao,
    pub desde: NaiveDateTime,
    pub inicio: NaiveDateTime,
    pub mensagem: String,
}

impl Sessao {
    /// Nome da pasta da sessão ("barra", "soul").
    pub fn nome(&self) -> &str {
        self.pasta.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or(&self.pasta)
    }
}

#[derive(Default)]
pub struct Dados {
    pub fontes: Vec<Fonte>,
    pub eventos: Vec<Evento>,
    pub execucoes: Vec<Execucao>,
    pub pendencias: Vec<Pendencia>,
    pub prazos: Vec<Prazo>,
    pub rotinas: Vec<Rotina>,
    pub sessoes: Vec<Sessao>,
    /// (fonte, id) marcados como concluídos na central.
    pub concluidas: HashSet<(String, String)>,
    /// Concluídas dos últimos 30 dias, mais recentes primeiro.
    pub historico: Vec<Concluida>,
    carimbo: u64,
}

/// "2026-10-23" (dia inteiro), "2026-10-23T09:00", com segundos ou com fuso.
pub fn data_hora(s: &str) -> Option<(NaiveDateTime, bool)> {
    let s = s.trim();
    if s.len() == 10 {
        return NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)).map(|d| (d, true));
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some((d.with_timezone(&Local).naive_local(), false));
    }
    ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
        .iter()
        .find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())
        .map(|d| (d, false))
}

fn data(v: &Value) -> Option<NaiveDate> {
    v.as_str().and_then(data_hora).map(|(d, _)| d.date())
}

fn texto(v: &Value) -> String {
    v.as_str().unwrap_or("").trim().to_string()
}

/// Nome da fonte para a tela.
pub fn rotulo_da_fonte(nome: &str) -> String {
    match nome {
        "agenda" => "Agenda".into(),
        "soul" => "Soul".into(),
        "agentes" => "claude-hooks".into(),
        "claude" => "Claude Code".into(),
        outro => outro.to_string(),
    }
}

/// Tira de um texto "a · b · c" as partes que são caminho de arquivo ("notas/rotinas.md",
/// "D:\Desktop\soul"): na tela fica só o fato.
fn sem_caminho(s: &str) -> String {
    let caminho = |p: &str| {
        let p = p.trim();
        !p.contains(' ') && (p.contains('/') || p.contains('\\') || p.ends_with(".md") || p.ends_with(".json"))
            || p.len() > 2 && p.as_bytes()[1] == b':' && p.contains('\\')
    };
    s.split(" · ").filter(|p| !caminho(p)).map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ")
}

fn glifo_da_fonte(nome: &str) -> &'static str {
    match nome {
        "agenda" => G_AGENDA,
        "soul" => G_SOUL,
        "claude" => G_CLAUDE,
        "agentes" => G_AGENTES,
        _ => G_FONTE,
    }
}

/// Ordem das fontes nos filtros: agenda, soul, agentes, claude, depois as outras.
fn ordem(nome: &str) -> (u8, String) {
    let p = match nome {
        "agenda" => 0,
        "soul" => 1,
        "agentes" => 2,
        "claude" => 3,
        _ => 4,
    };
    (p, nome.to_string())
}

/// Arquivos lidos, para saber se algo mudou sem abrir nenhum.
fn arquivos() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for dir in [pasta(), pasta().join("claude")] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            v.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json" || x == "jsonl")));
        }
    }
    v.sort();
    v
}

fn carimbo() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in arquivos() {
        p.hash(&mut h);
        if let Ok(m) = std::fs::metadata(&p) {
            m.len().hash(&mut h);
            m.modified().ok().hash(&mut h);
        }
    }
    h.finish()
}

impl Dados {
    /// Relê as pastas se algum arquivo mudou. Devolve se mudou.
    pub fn recarregar(&mut self) -> bool {
        let c = carimbo();
        if c == self.carimbo && self.carimbo != 0 {
            return false;
        }
        *self = Dados::ler();
        self.carimbo = c;
        true
    }

    fn ler() -> Dados {
        let mut d = Dados::default();
        let mut fontes: Vec<(String, Value)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(pasta()) {
            for p in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")) {
                let Some(v) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
                let nome = v["fonte"].as_str().map(String::from).unwrap_or_else(|| p.file_stem().unwrap_or_default().to_string_lossy().into_owned());
                fontes.push((nome, v));
            }
        }
        let mut sessoes = ler_sessoes();
        if !sessoes.is_empty() {
            fontes.push(("claude".into(), json!({ "descricao": "Sessões do Claude Code" })));
        }
        fontes.sort_by_key(|(n, _)| ordem(n));
        for (i, (nome, v)) in fontes.iter().enumerate() {
            d.fontes.push(Fonte {
                nome: nome.clone(),
                descricao: v["descricao"].as_str().map(sem_caminho).filter(|d| !d.is_empty()).unwrap_or_else(|| rotulo_da_fonte(nome)),
                glifo: glifo_da_fonte(nome),
                atualizado: v["atualizado"].as_str().and_then(data_hora).map(|(d, _)| d),
                erro: v["erro"].as_str().map(String::from).filter(|e| !e.is_empty()),
                executar: Executar::de(&v["executar"], "executar"),
            });
            for it in v["itens"].as_array().into_iter().flatten() {
                d.item(i, it);
            }
            if nome == "claude" {
                d.sessoes = std::mem::take(&mut sessoes);
            }
        }
        d.eventos.sort_by_key(|e| e.inicio);
        d.execucoes.sort_by_key(|e| e.inicio);
        d.prazos.sort_by_key(|p| p.data);
        d.concluidas = ler_concluidas();
        d.historico = ler_historico();
        // concluídas que só estão no acoes.jsonl (marcadas antes do histórico existir)
        let marcadas = quando_marcadas();
        for p in &d.pendencias {
            let Some(f) = d.fontes.get(p.fonte) else { continue };
            let chave = (f.nome.clone(), p.id.clone());
            if d.concluidas.contains(&chave) && !d.historico.iter().any(|h| h.fonte == chave.0 && h.id == chave.1) {
                let quando = marcadas.get(&chave).copied().unwrap_or_else(|| Local::now().naive_local());
                d.historico.push(Concluida {
                    fonte: chave.0,
                    id: chave.1,
                    titulo: p.titulo.clone(),
                    origem: if p.origem.is_empty() { f.descricao.clone() } else { p.origem.clone() },
                    link: p.link.clone(),
                    quando,
                });
            }
        }
        d
    }

    fn item(&mut self, fonte: usize, it: &Value) {
        let titulo = texto(&it["titulo"]);
        if titulo.is_empty() {
            return;
        }
        let executar = Executar::de(&it["executar"], "executar agora");
        match it["tipo"].as_str().unwrap_or("") {
            "evento" => {
                let Some((inicio, dia_inteiro)) = it["inicio"].as_str().and_then(data_hora) else { return };
                let fim = it["fim"].as_str().and_then(data_hora).map(|(f, _)| f).filter(|f| *f > inicio);
                let fim = fim.unwrap_or(if dia_inteiro { inicio + Duration::days(1) } else { inicio + Duration::minutes(30) });
                self.eventos.push(Evento {
                    fonte,
                    titulo,
                    inicio,
                    fim,
                    dia_inteiro,
                    local: texto(&it["local"]),
                    descricao: texto(&it["descricao"]),
                    link: texto(&it["link"]),
                    reuniao: texto(&it["reuniao"]),
                });
            }
            "execucao" => {
                let Some((inicio, _)) = it["inicio"].as_str().and_then(data_hora) else { return };
                self.execucoes.push(Execucao {
                    fonte,
                    titulo,
                    inicio,
                    fim: it["fim"].as_str().and_then(data_hora).map(|(f, _)| f),
                    estado: Estado::de(it["estado"].as_str().unwrap_or("")),
                    resumo: texto(&it["resumo"]),
                    registro: texto(&it["registro"]),
                    executar,
                });
            }
            "pendencia" => self.pendencias.push(Pendencia {
                fonte,
                id: it["id"].as_str().map(String::from).unwrap_or_else(|| titulo.clone()),
                titulo,
                origem: sem_caminho(&texto(&it["origem"])),
                prazo: data(&it["prazo"]),
                link: texto(&it["link"]),
                executar,
            }),
            "prazo" => {
                let Some(dt) = data(&it["data"]) else { return };
                self.prazos.push(Prazo { fonte, titulo, data: dt, link: texto(&it["link"]) });
            }
            "rotina" => self.rotinas.push(Rotina {
                fonte,
                titulo,
                quando: texto(&it["quando"]),
                ultima: data(&it["ultima"]["data"]).map(|dt| (dt, Estado::de(it["ultima"]["estado"].as_str().unwrap_or("")))),
                proxima: data(&it["proxima"]),
                executar,
            }),
            _ => {}
        }
    }

    pub fn concluida(&self, p: &Pendencia) -> bool {
        self.fontes.get(p.fonte).is_some_and(|f| self.concluidas.contains(&(f.nome.clone(), p.id.clone())))
    }

    /// Marca (ou desmarca) a pendência e grava em `acoes.jsonl`, que a fonte lê na execução
    /// seguinte para atualizar o registro dela.
    pub fn alternar(&mut self, i: usize) {
        let Some(p) = self.pendencias.get(i) else { return };
        let Some(f) = self.fontes.get(p.fonte) else { return };
        let c = Concluida {
            fonte: f.nome.clone(),
            id: p.id.clone(),
            titulo: p.titulo.clone(),
            origem: if p.origem.is_empty() { f.descricao.clone() } else { p.origem.clone() },
            link: p.link.clone(),
            quando: Local::now().naive_local(),
        };
        let concluir = !self.concluidas.contains(&(c.fonte.clone(), c.id.clone()));
        self.registrar(c, concluir);
    }

    /// Reabre uma concluída da aba de concluídas (índice em `historico`).
    pub fn reabrir(&mut self, i: usize) {
        if let Some(c) = self.historico.get(i).cloned() {
            self.registrar(c, false);
        }
    }

    fn registrar(&mut self, c: Concluida, concluir: bool) {
        let acao = if concluir { "concluir" } else { "reabrir" };
        let agora = Local::now().to_rfc3339();
        let linha = json!({ "quando": agora, "fonte": c.fonte, "id": c.id, "titulo": c.titulo, "acao": acao });
        let hist = json!({ "quando": agora, "fonte": c.fonte, "id": c.id, "titulo": c.titulo, "origem": c.origem, "link": c.link, "acao": acao });
        _ = std::fs::create_dir_all(pasta());
        for (arq, l) in [("acoes.jsonl", linha), ("concluidas.jsonl", hist)] {
            if let Ok(mut a) = std::fs::OpenOptions::new().create(true).append(true).open(pasta().join(arq)) {
                _ = writeln!(a, "{l}");
            }
        }
        let chave = (c.fonte.clone(), c.id.clone());
        self.historico.retain(|h| (h.fonte.as_str(), h.id.as_str()) != (chave.0.as_str(), chave.1.as_str()));
        if concluir {
            self.concluidas.insert(chave);
            self.historico.insert(0, c);
        } else {
            self.concluidas.remove(&chave);
        }
    }
}

/// Momento da última marcação de cada (fonte, id) no `acoes.jsonl`.
fn quando_marcadas() -> std::collections::HashMap<(String, String), NaiveDateTime> {
    let mut m = std::collections::HashMap::new();
    let Ok(t) = std::fs::read_to_string(pasta().join("acoes.jsonl")) else { return m };
    for v in t.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        if let Some((q, _)) = v["quando"].as_str().and_then(data_hora) {
            m.insert((texto(&v["fonte"]), texto(&v["id"])), q);
        }
    }
    m
}

fn ler_concluidas() -> HashSet<(String, String)> {
    let mut s = HashSet::new();
    let Ok(t) = std::fs::read_to_string(pasta().join("acoes.jsonl")) else { return s };
    for v in t.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        let chave = (texto(&v["fonte"]), texto(&v["id"]));
        match v["acao"].as_str() {
            Some("concluir") => _ = s.insert(chave),
            Some("reabrir") => _ = s.remove(&chave),
            _ => {}
        }
    }
    s
}

/// `concluidas.jsonl`: para cada (fonte, id) vale a última linha; reaberta sai.
fn ler_historico() -> Vec<Concluida> {
    let limite = Local::now().naive_local() - Duration::days(30);
    let mut v: Vec<Concluida> = Vec::new();
    let Ok(t) = std::fs::read_to_string(pasta().join("concluidas.jsonl")) else { return v };
    for l in t.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        let (fonte, id) = (texto(&l["fonte"]), texto(&l["id"]));
        v.retain(|c| !(c.fonte == fonte && c.id == id));
        let Some((quando, _)) = l["quando"].as_str().and_then(data_hora) else { continue };
        if l["acao"] == "concluir" && quando > limite {
            v.push(Concluida { fonte, id, titulo: texto(&l["titulo"]), origem: texto(&l["origem"]), link: texto(&l["link"]), quando });
        }
    }
    v.reverse();
    v
}

/// Sessões das últimas 12 horas; concluídas, só as da última meia hora.
fn ler_sessoes() -> Vec<Sessao> {
    let agora = Local::now().naive_local();
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(pasta().join("claude")) else { return v };
    for p in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")) {
        let Some(j) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
        let Some((desde, _)) = j["desde"].as_str().and_then(data_hora) else { continue };
        let estado = match j["estado"].as_str() {
            Some("trabalhando") => EstadoSessao::Trabalhando,
            Some("permissao") => EstadoSessao::Permissao,
            Some("concluida") => EstadoSessao::Concluida,
            _ => continue,
        };
        let idade = agora - desde;
        if idade > Duration::hours(12) || (estado == EstadoSessao::Concluida && idade > Duration::minutes(30)) {
            continue;
        }
        v.push(Sessao {
            pasta: texto(&j["pasta"]),
            estado,
            desde,
            inicio: j["inicio"].as_str().and_then(data_hora).map(|(d, _)| d).unwrap_or(desde),
            mensagem: texto(&j["mensagem"]),
        });
    }
    v.sort_by_key(|s| (s.estado != EstadoSessao::Permissao, s.estado != EstadoSessao::Trabalhando, std::cmp::Reverse(s.desde)));
    v
}

// ---------- janela ----------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Vista {
    Dia,
    Semana,
}

/// Item escolhido na linha do tempo (detalhe no pé da coluna).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sel {
    Evento(usize),
    Execucao(usize),
}

/// O que fica embaixo do cursor na central.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AlvoCentral {
    Anterior,
    Seguinte,
    Vista(Vista),
    Fonte(usize),
    Evento(usize),
    Execucao(usize),
    /// Botão do detalhe: 0 = link principal, 1 = reunião, 2 = executar.
    Acao(u8),
    FecharDetalhe,
    Pendencia(usize),
    LinkPendencia(usize),
    ExecutarPendencia(usize),
    /// Pendência sintetizada: fonte com comando e sem execução hoje.
    ExecutarFonte(usize),
    Prazo(usize),
    MesAnterior,
    MesSeguinte,
    Dia(NaiveDate),
    Rotina(usize),
    /// Aba das pendências: false = em aberto, true = concluídas.
    AbaPendencias(bool),
    Reabrir(usize),
    LinkConcluida(usize),
    /// Linha inteira de uma rotina (o comando é `Rotina`).
    RotinaLinha(usize),
    Sessao(usize),
}

pub struct Central {
    pub hwnd: HWND,
    pub aberta: bool,
    pub dados: Dados,
    pub vista: Vista,
    pub dia: NaiveDate,
    /// Primeiro dia do mês mostrado no calendário pequeno.
    pub mes: NaiveDate,
    /// Fontes desligadas no filtro (por nome, para valer entre recargas).
    pub ocultas: HashSet<String>,
    pub filtro: String,
    pub sel: Option<Sel>,
    pub hover: Option<AlvoCentral>,
    pub alvos: Vec<(D2D_RECT_F, AlvoCentral)>,
    /// Janela em foco antes de abrir, para devolver ao fechar com Esc.
    pub anterior: isize,
    /// Rolagem da coluna de pendências e prazos, em pixels sem escala.
    pub rolagem: f32,
    /// Área da lista rolável e rolagem máxima, gravadas a cada pintura.
    pub lista: Cell<D2D_RECT_F>,
    pub rolagem_max: Cell<f32>,
    /// Aba das pendências mostrando as concluídas.
    pub aba_concluidas: bool,
    /// (fonte, id) marcados desde que a central abriu: seguem riscados na aba de abertas,
    /// para a lista não mudar embaixo do cursor.
    pub recem: HashSet<(String, String)>,
    /// Última cópia para a área de transferência (aviso "copiado" no rodapé).
    pub copiado: Option<std::time::Instant>,
}

impl Default for Central {
    fn default() -> Self {
        let hoje = Local::now().date_naive();
        Central {
            hwnd: HWND::default(),
            aberta: false,
            dados: Dados::default(),
            vista: Vista::Dia,
            dia: hoje,
            mes: hoje.with_day(1).unwrap_or(hoje),
            ocultas: HashSet::new(),
            filtro: String::new(),
            sel: None,
            hover: None,
            alvos: Vec::new(),
            anterior: 0,
            rolagem: 0.0,
            lista: Cell::new(D2D_RECT_F::default()),
            rolagem_max: Cell::new(0.0),
            aba_concluidas: false,
            recem: HashSet::new(),
            copiado: None,
        }
    }
}

impl Central {
    pub fn visivel(&self, fonte: usize, titulo: &str) -> bool {
        let Some(f) = self.dados.fontes.get(fonte) else { return false };
        if self.ocultas.contains(&f.nome) {
            return false;
        }
        self.filtro.is_empty() || titulo.to_lowercase().contains(&self.filtro.to_lowercase())
    }

    pub fn ir_dia(&mut self, d: NaiveDate) {
        self.dia = d;
        self.mes = d.with_day(1).unwrap_or(d);
        self.sel = None;
    }

    /// Dia anterior ou seguinte (semana, na vista de semana).
    pub fn passo(&mut self, n: i64) {
        let dias = if self.vista == Vista::Dia { n } else { n * 7 };
        self.ir_dia(self.dia + Duration::days(dias));
    }

    /// Rola a lista de pendências quando (x, y) está sobre ela; devolve se o cursor estava nela.
    pub fn rolar(&mut self, x: f32, y: f32, delta: f32) -> bool {
        let r = self.lista.get();
        if !(x >= r.left && x < r.right && y >= r.top && y < r.bottom) {
            return false;
        }
        self.rolagem = (self.rolagem - delta).clamp(0.0, self.rolagem_max.get());
        true
    }

    pub fn alvo_em(&self, x: f32, y: f32) -> Option<AlvoCentral> {
        self.alvos.iter().find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(_, a)| *a)
    }

    /// Próximo compromisso de hoje que começa em até 30 minutos: (título, minutos).
    pub fn proximo(&self, agora: NaiveDateTime) -> Option<(String, i64)> {
        self.dados
            .eventos
            .iter()
            .filter(|e| !e.dia_inteiro && e.inicio > agora && e.inicio - agora <= Duration::minutes(30))
            .min_by_key(|e| e.inicio)
            .map(|e| (e.titulo.clone(), (e.inicio - agora).num_minutes().max(0) + 1))
    }

    /// Execução de hoje com falha ou sessão aguardando permissão.
    /// Texto de um item da central para a área de transferência.
    pub fn texto_de(&self, a: AlvoCentral) -> Option<String> {
        let d = &self.dados;
        let hm = |t: NaiveDateTime| t.format("%d/%m/%Y %H:%M").to_string();
        let juntar = |partes: Vec<String>| Some(partes.into_iter().filter(|p| !p.trim().is_empty()).collect::<Vec<_>>().join("\n"));
        match a {
            AlvoCentral::Evento(i) => {
                let e = d.eventos.get(i)?;
                let quando = if e.dia_inteiro { e.inicio.format("%d/%m/%Y").to_string() } else { format!("{} – {}", hm(e.inicio), e.fim.format("%H:%M")) };
                juntar(vec![e.titulo.clone(), quando, e.local.clone(), e.reuniao.clone(), e.descricao.clone()])
            }
            AlvoCentral::Execucao(i) => {
                let e = d.execucoes.get(i)?;
                let quando = match e.fim {
                    Some(f) => format!("{} – {} · {}", hm(e.inicio), f.format("%H:%M"), e.estado.rotulo()),
                    None => format!("{} · {}", hm(e.inicio), e.estado.rotulo()),
                };
                juntar(vec![e.titulo.clone(), quando, e.resumo.clone(), e.registro.clone()])
            }
            AlvoCentral::Pendencia(i) | AlvoCentral::LinkPendencia(i) | AlvoCentral::ExecutarPendencia(i) => {
                let p = d.pendencias.get(i)?;
                let prazo = p.prazo.map(|z| format!("prazo {}", z.format("%d/%m/%Y"))).unwrap_or_default();
                juntar(vec![p.titulo.clone(), p.origem.clone(), prazo, p.link.clone()])
            }
            AlvoCentral::Prazo(i) => {
                let p = d.prazos.get(i)?;
                juntar(vec![p.titulo.clone(), p.data.format("%d/%m/%Y").to_string(), p.link.clone()])
            }
            AlvoCentral::Rotina(i) | AlvoCentral::RotinaLinha(i) => {
                let r = d.rotinas.get(i)?;
                let prox = r.proxima.map(|z| format!("próxima {}", z.format("%d/%m/%Y"))).unwrap_or_default();
                juntar(vec![r.titulo.clone(), r.quando.clone(), prox, r.executar.as_ref().map(|e| e.comando.clone()).unwrap_or_default()])
            }
            AlvoCentral::Reabrir(i) | AlvoCentral::LinkConcluida(i) => {
                let h = d.historico.get(i)?;
                juntar(vec![h.titulo.clone(), h.origem.clone(), format!("concluída em {}", hm(h.quando)), h.link.clone()])
            }
            AlvoCentral::ExecutarFonte(i) => d.fontes.get(i)?.executar.as_ref().map(|e| e.comando.clone()),
            AlvoCentral::Fonte(i) => {
                let f = d.fontes.get(i)?;
                juntar(vec![rotulo_da_fonte(&f.nome), f.descricao.clone(), f.erro.clone().unwrap_or_default()])
            }
            AlvoCentral::Sessao(i) => {
                let se = d.sessoes.get(i)?;
                let estado = match se.estado {
                    EstadoSessao::Trabalhando => format!("em execução desde {}", se.inicio.format("%H:%M")),
                    EstadoSessao::Permissao => format!("aguarda permissão desde {}", se.desde.format("%H:%M")),
                    EstadoSessao::Concluida => format!("concluída às {}", se.desde.format("%H:%M")),
                };
                juntar(vec![format!("Sessão do Claude Code em {}", se.nome()), se.pasta.clone(), estado, se.mensagem.clone()])
            }
            AlvoCentral::Dia(dia) => {
                let mut v = vec![dia.format("%d/%m/%Y").to_string()];
                for e in d.eventos.iter().filter(|e| e.inicio.date() <= dia && e.fim.date() >= dia && (e.fim.date() > dia || e.fim > e.inicio || e.inicio.date() == dia)) {
                    let h = if e.dia_inteiro { "dia inteiro".to_string() } else { format!("{}–{}", e.inicio.format("%H:%M"), e.fim.format("%H:%M")) };
                    v.push(format!("{h}  {}", e.titulo));
                }
                for e in d.execucoes.iter().filter(|e| e.inicio.date() == dia) {
                    v.push(format!("{}  {} ({})", e.inicio.format("%H:%M"), e.titulo, e.estado.rotulo()));
                }
                for p in d.prazos.iter().filter(|p| p.data == dia) {
                    v.push(format!("prazo: {}", p.titulo));
                }
                juntar(v)
            }
            // o fundo do detalhe vale pelo item aberto
            AlvoCentral::FecharDetalhe => self.texto_para_copiar_sel(),
            _ => None,
        }
    }

    /// Pasta de trabalho da fonte do item (a do comando dela; vazia = pasta do usuário).
    pub fn pasta_de(&self, a: AlvoCentral) -> String {
        let d = &self.dados;
        let fonte = match a {
            AlvoCentral::Evento(i) => d.eventos.get(i).map(|e| e.fonte),
            AlvoCentral::Execucao(i) => d.execucoes.get(i).map(|e| e.fonte),
            AlvoCentral::Pendencia(i) | AlvoCentral::LinkPendencia(i) | AlvoCentral::ExecutarPendencia(i) => d.pendencias.get(i).map(|p| p.fonte),
            AlvoCentral::Prazo(i) => d.prazos.get(i).map(|p| p.fonte),
            AlvoCentral::Rotina(i) | AlvoCentral::RotinaLinha(i) => d.rotinas.get(i).map(|r| r.fonte),
            AlvoCentral::Sessao(i) => return d.sessoes.get(i).map(|se| se.pasta.clone()).unwrap_or_default(),
            AlvoCentral::FecharDetalhe => return self.alvo_sel().map(|a| self.pasta_de(a)).unwrap_or_default(),
            AlvoCentral::Reabrir(i) | AlvoCentral::LinkConcluida(i) => d.historico.get(i).and_then(|h| d.fontes.iter().position(|f| f.nome == h.fonte)),
            AlvoCentral::Fonte(i) | AlvoCentral::ExecutarFonte(i) => Some(i),
            _ => None,
        };
        fonte.and_then(|f| d.fontes.get(f)).and_then(|f| f.executar.as_ref()).map(|e| e.pasta.clone()).unwrap_or_default()
    }

    fn alvo_sel(&self) -> Option<AlvoCentral> {
        self.sel.map(|s| match s {
            Sel::Evento(i) => AlvoCentral::Evento(i),
            Sel::Execucao(i) => AlvoCentral::Execucao(i),
        })
    }

    fn texto_para_copiar_sel(&self) -> Option<String> {
        self.alvo_sel().and_then(|a| self.texto_de(a))
    }

    /// O que o Ctrl+C copia: o item aberto no detalhe, senão o que está sob o cursor.
    pub fn texto_para_copiar(&self) -> Option<String> {
        self.texto_para_copiar_sel().or_else(|| self.hover.and_then(|a| self.texto_de(a)))
    }

    pub fn alerta(&self, hoje: NaiveDate) -> bool {
        self.dados.execucoes.iter().any(|e| e.inicio.date() == hoje && e.estado == Estado::Falha)
            || self.dados.sessoes.iter().any(|s| s.estado == EstadoSessao::Permissao)
    }

    /// Fontes com comando e sem nenhuma execução hoje (o soul só roda quando alguém pede).
    pub fn sem_execucao_hoje(&self, hoje: NaiveDate) -> Vec<usize> {
        (0..self.dados.fontes.len())
            .filter(|&i| self.dados.fontes[i].executar.is_some())
            .filter(|&i| !self.dados.execucoes.iter().any(|e| e.fonte == i && e.inicio.date() == hoje))
            .collect()
    }
}

/// Primeiro domingo da semana de `d`.
pub fn inicio_semana(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_sunday() as i64)
}

/// Número da semana ISO 8601.
pub fn semana_iso(d: NaiveDate) -> u32 {
    d.iso_week().week()
}

/// Pasta dos scripts e prompts passados aos terminais abertos pela central; os de mais de um
/// dia já foram usados e saem.
fn pasta_terminal() -> std::path::PathBuf {
    let dir = pasta().parent().map(|p| p.to_path_buf()).unwrap_or_else(std::env::temp_dir).join("terminal");
    _ = std::fs::create_dir_all(&dir);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let velho = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > 86_400);
            if velho {
                _ = std::fs::remove_file(e.path());
            }
        }
    }
    dir
}

/// Texto entre aspas simples do PowerShell.
fn aspas_ps(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Abre o comando (sintaxe do PowerShell) num pwsh novo, no console do Windows, como o atalho
/// do AutoHotkey abre o Claude. Sem elevação: a barra roda como administrador, e o que ela abre
/// direto herdaria isso; o Explorer abre o `.cmd` com o usuário comum. O comando vai num `.ps1`
/// para não passar pelas regras de aspas do cmd.
pub fn executar(e: &Executar) {
    let dir_scripts = pasta_terminal();
    let nome = Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
    let ps1 = dir_scripts.join(format!("{nome}.ps1"));
    let dir = if e.pasta.is_empty() { std::env::var("USERPROFILE").unwrap_or_default() } else { e.pasta.clone() };
    // BOM: o Windows PowerShell e o pwsh leem o .ps1 com acento certo
    let script = format!("\u{feff}Set-Location -LiteralPath {}\r\n{}\r\n", aspas_ps(&dir), e.comando);
    let cmd = dir_scripts.join(format!("{nome}.cmd"));
    let lancador = format!("@echo off\r\nstart \"\" /D \"{dir}\" pwsh.exe -NoExit -File \"{}\"\r\n", ps1.display());
    if std::fs::write(&ps1, script).is_err() || std::fs::write(&cmd, lancador).is_err() {
        return;
    }
    _ = std::process::Command::new("explorer.exe").arg(&cmd).spawn();
}

/// Abre o Claude Code num pwsh novo, sem elevação, em `dir_trabalho`, com `texto` já enviado
/// como primeira mensagem. O texto vai por arquivo, lido pelo pwsh e passado como um argumento
/// só: aspas, quebras de linha e `%` chegam intactos.
pub fn abrir_com_claude(texto: &str, dir_trabalho: &str) {
    let arq = pasta_terminal().join(format!("{}.txt", Local::now().format("%Y%m%d-%H%M%S-%3f")));
    if std::fs::write(&arq, texto).is_err() {
        return;
    }
    let comando = format!("claude --dangerously-skip-permissions (Get-Content -Raw -Encoding utf8 -LiteralPath {})", aspas_ps(&arq.display().to_string()));
    executar(&Executar { rotulo: String::new(), pasta: dir_trabalho.to_string(), comando });
}
