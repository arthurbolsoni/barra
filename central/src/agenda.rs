//! Sincronização do Google Agenda pela conta de serviço (a mesma do soul): a agenda precisa
//! estar compartilhada com o e-mail da conta. Grava `agenda.json` na pasta da central.
//!
//! Configuração em `%LOCALAPPDATA%\barra\agenda-config.json`:
//! `{ "chave": "<sa.json>", "agendas": ["<id da agenda>"], "intervalo_min": 5 }`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration as Dur, Local, NaiveDate};
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use serde_json::{json, Value};
use sha2::Sha256;
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForMultipleObjects, EVENT_MODIFY_STATE};

use crate::{base, gravar_atomico, pasta_central, registrar};

const ESCOPO: &str = "https://www.googleapis.com/auth/calendar.readonly";
/// Janela sincronizada: de duas semanas atrás até dois meses à frente.
const DIAS_ANTES: i64 = 14;
const DIAS_DEPOIS: i64 = 60;

struct Config {
    chave: String,
    agendas: Vec<String>,
    intervalo: Duration,
}

fn ler_config() -> Config {
    let padrao_chave = std::env::var("USERPROFILE").map(|u| format!("{u}\\.soul-google\\sa.json")).unwrap_or_default();
    let v: Value = std::fs::read(base().join("agenda-config.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    Config {
        chave: v["chave"].as_str().map(String::from).unwrap_or(padrao_chave),
        agendas: v["agendas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        intervalo: Duration::from_secs(v["intervalo_min"].as_u64().unwrap_or(5).max(1) * 60),
    }
}

struct Token {
    valor: String,
    expira: SystemTime,
    email: String,
}

fn agora_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Token de acesso pelo fluxo JWT da conta de serviço.
fn obter_token(chave: &str) -> Result<Token, String> {
    let sa: Value = std::fs::read(chave)
        .map_err(|e| format!("Chave da conta de serviço não encontrada em {chave}: {e}"))
        .and_then(|b| serde_json::from_slice(&b).map_err(|e| format!("Chave da conta de serviço inválida: {e}")))?;
    let email = sa["client_email"].as_str().ok_or("Chave sem client_email.")?.to_string();
    let uri = sa["token_uri"].as_str().unwrap_or("https://oauth2.googleapis.com/token");
    let pem = sa["private_key"].as_str().ok_or("Chave sem private_key.")?;
    let k = RsaPrivateKey::from_pkcs8_pem(pem).map_err(|e| format!("Chave privada inválida: {e}"))?;
    let iat = agora_s();
    let cab = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let corpo = URL_SAFE_NO_PAD.encode(json!({ "iss": email, "scope": ESCOPO, "aud": uri, "iat": iat, "exp": iat + 3600 }).to_string());
    let assinar = format!("{cab}.{corpo}");
    let assinatura = SigningKey::<Sha256>::new(k).sign(assinar.as_bytes());
    let jwt = format!("{assinar}.{}", URL_SAFE_NO_PAD.encode(assinatura.to_bytes()));
    let r: Value = ureq::post(uri)
        .timeout(Duration::from_secs(20))
        .send_form(&[("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"), ("assertion", &jwt)])
        .map_err(|e| format!("Falha ao obter o token do Google: {}", erro_http(e)))?
        .into_string()
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or("Resposta do token ilegível.")?;
    let valor = r["access_token"].as_str().ok_or("Resposta do token sem access_token.")?.to_string();
    let validade = r["expires_in"].as_u64().unwrap_or(3600);
    Ok(Token { valor, expira: SystemTime::now() + Duration::from_secs(validade.saturating_sub(120)), email })
}

fn erro_http(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(c, r) => {
            let corpo = r.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&corpo).ok().and_then(|v| v["error"]["message"].as_str().or(v["error_description"].as_str()).map(String::from));
            format!("HTTP {c}{}", msg.map(|m| format!(" ({m})")).unwrap_or_default())
        }
        ureq::Error::Transport(t) => t.to_string(),
    }
}

/// "2026-10-23T09:00:00-03:00" → "2026-10-23T09:00" no fuso local; "2026-10-23" fica igual.
fn local(v: &Value) -> Option<String> {
    if let Some(dt) = v["dateTime"].as_str() {
        let d = DateTime::parse_from_rfc3339(dt).ok()?.with_timezone(&Local);
        return Some(d.format("%Y-%m-%dT%H:%M").to_string());
    }
    v["date"].as_str().map(String::from)
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

fn eventos(token: &Token, agenda: &str) -> Result<Vec<Value>, String> {
    let hoje = Local::now().date_naive();
    let meia_noite = |d: NaiveDate| d.and_hms_opt(0, 0, 0).and_then(|x| x.and_local_timezone(Local).single()).map(|x| x.to_rfc3339());
    let ini = meia_noite(hoje - Dur::days(DIAS_ANTES)).unwrap_or_default();
    let fim = meia_noite(hoje + Dur::days(DIAS_DEPOIS)).unwrap_or_default();
    let url = format!("https://www.googleapis.com/calendar/v3/calendars/{}/events", agenda.replace('@', "%40").replace('#', "%23"));
    let mut saida = Vec::new();
    let mut pagina: Option<String> = None;
    loop {
        let mut req = ureq::get(&url)
            .timeout(Duration::from_secs(30))
            .set("Authorization", &format!("Bearer {}", token.valor))
            .query("timeMin", &ini)
            .query("timeMax", &fim)
            .query("singleEvents", "true")
            .query("orderBy", "startTime")
            .query("maxResults", "2500");
        if let Some(p) = &pagina {
            req = req.query("pageToken", p);
        }
        let r: Value = match req.call() {
            Ok(r) => r.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or("Resposta da agenda ilegível.")?,
            Err(ureq::Error::Status(404, _)) => {
                return Err(format!("A agenda {agenda} não está compartilhada com {}.", token.email));
            }
            Err(e) => return Err(format!("Falha ao ler a agenda {agenda}: {}", erro_http(e))),
        };
        for e in r["items"].as_array().into_iter().flatten() {
            if e["status"] == "cancelled" {
                continue;
            }
            let recusado = e["attendees"].as_array().is_some_and(|a| a.iter().any(|p| p["self"] == true && p["responseStatus"] == "declined"));
            if recusado {
                continue;
            }
            let (Some(inicio), Some(fim)) = (local(&e["start"]), local(&e["end"])) else { continue };
            let mut item = json!({
                "tipo": "evento",
                "id": format!("{agenda}/{}", e["id"].as_str().unwrap_or_default()),
                "titulo": e["summary"].as_str().unwrap_or("(sem título)"),
                "inicio": inicio,
                "fim": fim,
            });
            let campos = [
                ("local", e["location"].as_str().map(String::from)),
                ("descricao", e["description"].as_str().map(limpar).filter(|d| !d.is_empty())),
                ("link", e["htmlLink"].as_str().map(String::from)),
                ("reuniao", e["hangoutLink"].as_str().map(String::from)),
            ];
            for (k, v) in campos {
                if let Some(v) = v {
                    item[k] = Value::String(v);
                }
            }
            saida.push(item);
        }
        match r["nextPageToken"].as_str() {
            Some(p) => pagina = Some(p.to_string()),
            None => break,
        }
    }
    Ok(saida)
}

/// Grava `agenda.json`. Com erro, mantém os eventos da última sincronização que deu certo.
fn gravar(itens: &[Value], erro: Option<&str>, ultima_ok: Option<&str>) {
    let mut v = json!({
        "versao": 1,
        "fonte": "agenda",
        "descricao": "Google Agenda",
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
        let mut token: Option<Token> = None;
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
            let resultado = (|| -> Result<Vec<Value>, String> {
                if cfg.agendas.is_empty() {
                    return Err(format!("Nenhuma agenda configurada em {}.", base().join("agenda-config.json").display()));
                }
                if token.as_ref().is_none_or(|t| SystemTime::now() >= t.expira) {
                    token = Some(obter_token(&cfg.chave)?);
                }
                let t = token.as_ref().ok_or("Sem token.")?;
                let mut todos = Vec::new();
                for a in &cfg.agendas {
                    todos.extend(eventos(t, a)?);
                }
                Ok(todos)
            })();
            match resultado {
                Ok(itens) => {
                    let agora = Local::now().to_rfc3339();
                    gravar(&itens, None, Some(&agora));
                    ultima_ok = Some(agora);
                }
                Err(e) => {
                    registrar(&e);
                    token = None;
                    let (itens, _) = itens_atuais();
                    gravar(&itens, Some(&e), ultima_ok.as_deref());
                }
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
pub fn pedir_saida() {
    unsafe {
        if let Ok(e) = OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\barra-central-sair")) {
            _ = SetEvent(e);
            _ = CloseHandle(e);
        }
    }
}
