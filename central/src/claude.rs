//! Hook do Claude Code: cada evento chega no stdin como JSON (`session_id`, `cwd`,
//! `hook_event_name`, `message`…). O estado de cada sessão fica num arquivo próprio em
//! `claude\<sessão>.json`, para sessões simultâneas não disputarem o mesmo arquivo.
//!
//! Estados: `trabalhando` (pedido enviado), `permissao` (aguardando aprovação de ferramenta),
//! `concluida` (turno terminado) e `ociosa` (sessão aberta sem pedido).

use std::io::Read;

use chrono::Local;
use serde_json::{json, Value};

use crate::{gravar_atomico, pasta_central};

pub fn hook() {
    let mut entrada = String::new();
    if std::io::stdin().read_to_string(&mut entrada).is_err() {
        return;
    }
    let Ok(ev) = serde_json::from_str::<Value>(&entrada) else { return };
    let Some(sessao) = ev["session_id"].as_str() else { return };
    let nome: String = sessao.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if nome.is_empty() {
        return;
    }
    let caminho = pasta_central().join("claude").join(format!("{nome}.json"));
    let anterior: Value = std::fs::read(&caminho).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let estado_ant = anterior["estado"].as_str().unwrap_or("");

    let (estado, mensagem) = match ev["hook_event_name"].as_str().unwrap_or("") {
        "SessionEnd" => {
            _ = std::fs::remove_file(&caminho);
            return;
        }
        "SessionStart" if estado_ant.is_empty() => ("ociosa", None),
        "UserPromptSubmit" => ("trabalhando", None),
        // aprovada a ferramenta, ela roda e a sessão volta a trabalhar
        "PostToolUse" if estado_ant == "permissao" => ("trabalhando", None),
        "Notification" => {
            let msg = ev["message"].as_str().unwrap_or("");
            let tipo = ev["notification_type"].as_str().unwrap_or("");
            if tipo == "permission_prompt" || msg.contains("permission") {
                ("permissao", Some(msg.to_string()))
            } else {
                return;
            }
        }
        "Stop" => ("concluida", None),
        _ => return,
    };
    if estado == estado_ant && mensagem.is_none() {
        return;
    }
    let agora = Local::now().to_rfc3339();
    let mut v = json!({
        "versao": 1,
        "sessao": sessao,
        "pasta": ev["cwd"].as_str().or(anterior["pasta"].as_str()).unwrap_or(""),
        "estado": estado,
        "desde": agora,
        // começo do turno atual
        "inicio": if estado == "trabalhando" && estado_ant != "permissao" { &agora } else { anterior["inicio"].as_str().unwrap_or(&agora) },
    });
    if let Some(m) = mensagem {
        v["mensagem"] = Value::String(m);
    }
    _ = gravar_atomico(&caminho, v.to_string().as_bytes());
}
