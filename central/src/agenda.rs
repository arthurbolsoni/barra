//! Sincronização das agendas pelo endereço iCal (o secreto do Google Agenda, o publicado do
//! Outlook.com). Grava `agenda.json` na pasta da central.
//!
//! Configuração em `%LOCALAPPDATA%arragenda-config.json`:
//! `{ "agendas": [{ "nome": "Google", "ical": "https://…" }, { "nome": "Outlook", "ical": "https://…" }], "intervalo_min": 5 }`.
//! Os endereços dão acesso de leitura à agenda inteira: não vão para o log nem para as mensagens de erro.

use std::time::Duration;

use chrono::{Duration as Dur, Local, NaiveDate};
use serde_json::{json, Value};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForMultipleObjects, EVENT_MODIFY_STATE};

use crate::{base, gravar_atomico, ical, pasta_central, registrar};

/// Janela sincronizada: de duas semanas atrás até dois meses à frente.
const DIAS_ANTES: i64 = 14;
const DIAS_DEPOIS: i64 = 60;

pub struct Agenda {
    pub nome: String,
    ical: String,
}

struct Config {
    agendas: Vec<Agenda>,
    intervalo: Duration,
}

fn ler_config() -> Config {
    let v: Value = std::fs::read(base().join("agenda-config.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let agendas = v["agendas"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let ical = a["ical"].as_str()?.trim();
            let ical = ical.strip_prefix("webcal://").map(|r| format!("https://{r}")).unwrap_or_else(|| ical.to_string());
            Some(Agenda { nome: a["nome"].as_str().unwrap_or("Agenda").to_string(), ical })
        })
        .collect();
    Config { agendas, intervalo: Duration::from_secs(v["intervalo_min"].as_u64().unwrap_or(5).max(1) * 60) }
}

/// Descrição do evento sem HTML e curta.
fn limpar(s: &str) -> String {
    let s = s.replace("<br>", "\n").replace("<br/>", "\n").replace("</p>", "\n");
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
    let out = out.trim();
    if out.chars().count() > 600 {
        out.chars().take(600).collect::<String>() + "…"
    } else {
        out.to_string()
    }
}

/// Baixa o calendário e devolve os eventos de [de, ate) no formato da central.
pub fn eventos(a: &Agenda, de: NaiveDate, ate: NaiveDate) -> Result<Vec<Value>, String> {
    let r = ureq::get(&a.ical).timeout(Duration::from_secs(30)).call().map_err(|e| match e {
        ureq::Error::Status(c, _) => format!("A agenda {} respondeu {c} no endereço iCal.", a.nome),
        ureq::Error::Transport(t) => format!("Falha de rede ao ler a agenda {}: {}.", a.nome, t.kind()),
    })?;
    let texto = r.into_string().map_err(|_| format!("Resposta ilegível da agenda {}.", a.nome))?;
    if !texto.contains("BEGIN:VCALENDAR") {
        return Err(format!("O endereço da agenda {} não devolveu um calendário iCal.", a.nome));
    }
    let mut v = Vec::new();
    for e in ical::eventos(&texto, de, ate) {
        let fmt = if e.dia_inteiro { "%Y-%m-%d" } else { "%Y-%m-%dT%H:%M" };
        let inicio = e.inicio.format(fmt).to_string();
        let mut item = json!({
            "tipo": "evento",
            "id": format!("{}/{}/{inicio}", a.nome, e.uid),
            "agenda": a.nome,
            "titulo": e.titulo,
            "inicio": inicio,
            "fim": e.fim.format(fmt).to_string(),
        });
        let descricao = limpar(&e.descricao);
        for (k, val) in [("local", e.local), ("descricao", descricao), ("link", e.link), ("reuniao", e.reuniao)] {
            if !val.is_empty() {
                item[k] = Value::String(val);
            }
        }
        v.push(item);
    }
    Ok(v)
}

/// Grava `agenda.json`. Com erro, mantém os eventos da última sincronização que deu certo.
fn gravar(itens: &[Value], descricao: &str, erro: Option<&str>, ultima_ok: Option<&str>) {
    let mut v = json!({
        "versao": 1,
        "fonte": "agenda",
        "descricao": descricao,
        "itens": itens,
    });
    // `atualizado` é a última sincronização que deu certo; sem nenhuma, fica de fora
    if let Some(u) = ultima_ok {
        v["atualizado"] = Value::String(u.to_string());
    }
    if let Some(e) = erro {
        v["erro"] = Value::String(e.to_string());
    }
    let caminho = pasta_central().join("agenda.json");
    if let Err(e) = gravar_atomico(&caminho, serde_json::to_string_pretty(&v).unwrap_or_default().as_bytes()) {
        registrar(&format!("não gravou {}: {e}", caminho.display()));
    }
}

/// Eventos do arquivo atual, para não apagar a agenda da barra quando a rede cai.
fn itens_atuais() -> (Vec<Value>, Option<String>) {
    let v: Value = std::fs::read(pasta_central().join("agenda.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let itens = v["itens"].as_array().cloned().unwrap_or_default();
    let quando = if v["erro"].is_null() { v["atualizado"].as_str().map(String::from) } else { None };
    (itens, quando)
}

pub fn servico() {
    unsafe {
        let Ok(_mutex) = CreateMutexW(None, true, w!("Local\\barra-central")) else { return };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        // sair: manual (a barra pede ao atualizar o executável); sincronizar: automático
        let Ok(sair) = CreateEventW(None, true, false, w!("Local\\barra-central-sair")) else { return };
        let Ok(sincronizar) = CreateEventW(None, false, false, w!("Local\\barra-central-sincronizar")) else { return };
        registrar("serviço iniciado");
        let (_, mut ultima_ok) = itens_atuais();
        let mut proxima_agenda = std::time::Instant::now();
        loop {
            crate::agentes::sincronizar();
            let cfg = ler_config();
            if std::time::Instant::now() < proxima_agenda {
                let r = WaitForMultipleObjects(&[sair, sincronizar], false, 30_000);
                if r == WAIT_OBJECT_0 {
                    break;
                }
                if r.0 != WAIT_OBJECT_0.0 + 1 {
                    continue;
                }
            }
            proxima_agenda = std::time::Instant::now() + cfg.intervalo;
            let hoje = Local::now().date_naive();
            let (de, ate) = (hoje - Dur::days(DIAS_ANTES), hoje + Dur::days(DIAS_DEPOIS));
            let descricao = if cfg.agendas.is_empty() { "Agenda".to_string() } else { cfg.agendas.iter().map(|a| a.nome.as_str()).collect::<Vec<_>>().join(" · ") };
            let (anteriores, _) = itens_atuais();
            let mut itens = Vec::new();
            let mut erros = Vec::new();
            if cfg.agendas.is_empty() {
                erros.push("Falta o endereço iCal das agendas no agenda-config.json.".to_string());
            }
            for a in &cfg.agendas {
                match eventos(a, de, ate) {
                    Ok(v) => itens.extend(v),
                    Err(e) => {
                        // a agenda que falhou fica com os eventos da última leitura
                        registrar(&e);
                        itens.extend(anteriores.iter().filter(|i| i["agenda"] == a.nome.as_str()).cloned());
                        erros.push(e);
                    }
                }
            }
            itens.sort_by(|x, y| x["inicio"].as_str().cmp(&y["inicio"].as_str()));
            if erros.is_empty() {
                let agora = Local::now().to_rfc3339();
                gravar(&itens, &descricao, None, Some(&agora));
                ultima_ok = Some(agora);
            } else {
                gravar(&itens, &descricao, Some(&erros.join(" ")), ultima_ok.as_deref());
            }
            // agentes a cada 30 s; a agenda quando vence o intervalo ou quando a barra pede
            let r = WaitForMultipleObjects(&[sair, sincronizar], false, 30_000);
            if r == WAIT_OBJECT_0 {
                break;
            }
            if r.0 == WAIT_OBJECT_0.0 + 1 {
                proxima_agenda = std::time::Instant::now();
            }
        }
        registrar("serviço encerrado");
        _ = CloseHandle(sair);
        _ = CloseHandle(sincronizar);
    }
}

/// `barra-central sair`: encerra o serviço em execução (a barra usa antes de trocar o executável).
/// `barra-central agenda [de] [ate]`: lê as agendas agora e imprime os eventos em JSON, um
/// objeto por linha (padrão: de hoje a 7 dias). Datas em AAAA-MM-DD; `ate` é exclusivo.
pub fn consultar(args: &[String]) {
    let hoje = Local::now().date_naive();
    let data = |i: usize| args.get(i).and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let de = data(0).unwrap_or(hoje);
    let ate = data(1).unwrap_or(de + Dur::days(7));
    let cfg = ler_config();
    if cfg.agendas.is_empty() {
        eprintln!("Nenhuma agenda configurada em {}.", base().join("agenda-config.json").display());
        std::process::exit(2);
    }
    let mut todos = Vec::new();
    let mut falhou = false;
    for a in &cfg.agendas {
        match eventos(a, de, ate) {
            Ok(v) => todos.extend(v),
            Err(e) => {
                eprintln!("{e}");
                falhou = true;
            }
        }
    }
    todos.sort_by(|x, y| x["inicio"].as_str().cmp(&y["inicio"].as_str()));
    for mut e in todos {
        if let Some(o) = e.as_object_mut() {
            o.remove("tipo");
        }
        println!("{e}");
    }
    if falhou {
        std::process::exit(1);
    }
}

pub fn pedir_saida() {
    unsafe {
        if let Ok(e) = OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\barra-central-sair")) {
            _ = SetEvent(e);
            _ = CloseHandle(e);
        }
    }
}
