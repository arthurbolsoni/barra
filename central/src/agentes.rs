//! Fonte `agentes`: as tarefas do claude-hooks (repositório claude-runner). Lê
//! `~/.claude-hooks` (ou `CLAUDE_HOOKS_HOME`) e grava `agentes.json`:
//!
//! - cada tarefa (`tasks/<nome>.toml`) vira uma rotina, com o comando `claude-hooks run`;
//! - cada execução dos últimos 14 dias (`runs/<tarefa>/<id>/meta.json`) vira uma execução;
//! - a execução mais recente de cada tarefa que terminou com `acao_necessaria` vira pendência.
//!
//! Sem a pasta do claude-hooks, o arquivo é apagado e a fonte some da central.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Local};
use serde_json::{json, Value};

use crate::{gravar_atomico, pasta_central, registrar};

fn home() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CLAUDE_HOOKS_HOME") {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("USERPROFILE").map(|u| PathBuf::from(u).join(".claude-hooks"))
}

fn local(s: Option<&str>) -> Option<String> {
    let d = DateTime::parse_from_rfc3339(s?).ok()?.with_timezone(&Local);
    Some(d.format("%Y-%m-%dT%H:%M").to_string())
}

fn estado(status: &str) -> &'static str {
    match status {
        "running" => "executando",
        "ok" => "ok",
        _ => "falha",
    }
}

/// Primeiras linhas do relatório, sem a linha de avaliação final.
fn inicio_do_relatorio(caminho: &Path) -> String {
    let Ok(t) = std::fs::read_to_string(caminho) else { return String::new() };
    t.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("CLAUDE_HOOKS_OUTCOME")).take(10).collect::<Vec<_>>().join("\n")
}

struct Tarefa {
    nome: String,
    titulo: String,
    quando: String,
}

fn ler_tarefas(home: &Path) -> Vec<Tarefa> {
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(home.join("tasks")) else { return v };
    for p in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")) {
        let Some(t) = std::fs::read_to_string(&p).ok().and_then(|s| s.parse::<toml::Table>().ok()) else { continue };
        let nome = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let gatilho = t.get("trigger").and_then(|g| g.as_table());
        let campo = |k: &str| gatilho.and_then(|g| g.get(k)).and_then(|x| x.as_str()).unwrap_or("");
        let mut quando = match campo("type") {
            "cron" => format!("cron {}", campo("schedule")),
            "command" => format!("checagem a cada {}", if campo("interval").is_empty() { "?" } else { campo("interval") }),
            "webhook" => "webhook".into(),
            outro => outro.to_string(),
        };
        if t.get("enabled").and_then(|e| e.as_bool()) == Some(false) {
            quando.push_str(" · desativada");
        }
        let titulo = t.get("description").and_then(|d| d.as_str()).filter(|d| !d.is_empty()).unwrap_or(&nome).to_string();
        v.push(Tarefa { nome, titulo, quando });
    }
    v.sort_by(|a, b| a.nome.cmp(&b.nome));
    v
}

/// (meta.json, pasta da execução) dos últimos 14 dias, mais recentes primeiro.
fn ler_execucoes(home: &Path, tarefa: &str) -> Vec<(Value, PathBuf)> {
    let limite = Local::now() - Duration::days(14);
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(home.join("runs").join(tarefa)) else { return v };
    for dir in rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Some(m) = std::fs::read(dir.join("meta.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
        let recente = m["started_at"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).is_some_and(|d| d > limite);
        if recente {
            v.push((m, dir));
        }
    }
    v.sort_by(|a, b| b.0["started_at"].as_str().cmp(&a.0["started_at"].as_str()));
    v
}

pub fn sincronizar() {
    let destino = pasta_central().join("agentes.json");
    let Some(home) = home().filter(|h| h.join("tasks").is_dir()) else {
        _ = std::fs::remove_file(&destino);
        return;
    };
    let mut itens: Vec<Value> = Vec::new();
    for t in ler_tarefas(&home) {
        let runs = ler_execucoes(&home, &t.nome);
        let comando = format!("claude-hooks run {}", t.nome);
        let mut rotina = json!({ "tipo": "rotina", "id": t.nome, "titulo": t.titulo, "quando": t.quando, "executar": { "rotulo": "executar agora", "comando": comando } });
        if let Some((m, _)) = runs.first() {
            if let Some(data) = local(m["started_at"].as_str()) {
                rotina["ultima"] = json!({ "data": &data[..10], "estado": estado(m["status"].as_str().unwrap_or("")) });
            }
        }
        itens.push(rotina);
        for (k, (m, dir)) in runs.iter().enumerate() {
            let Some(inicio) = local(m["started_at"].as_str()) else { continue };
            let run_id = m["run_id"].as_str().unwrap_or_default();
            let relatorio = dir.join("result.md");
            let mut resumo = Vec::new();
            for parte in [m["outcome"]["resumo"].as_str(), m["error"].as_str(), m["event"]["summary"].as_str()].into_iter().flatten() {
                if !parte.is_empty() {
                    resumo.push(parte.to_string());
                }
            }
            let rel = inicio_do_relatorio(&relatorio);
            if !rel.is_empty() {
                resumo.push(rel);
            }
            let mut ex = json!({
                "tipo": "execucao",
                "id": format!("{}/{run_id}", t.nome),
                "titulo": t.titulo,
                "inicio": inicio,
                "estado": estado(m["status"].as_str().unwrap_or("")),
                "resumo": resumo.join("\n"),
                "executar": { "rotulo": "executar novamente", "comando": comando },
            });
            if let Some(fim) = local(m["finished_at"].as_str()) {
                ex["fim"] = Value::String(fim);
            }
            if relatorio.exists() {
                ex["registro"] = Value::String(relatorio.display().to_string());
            }
            itens.push(ex);
            // só a mais recente de cada tarefa pode virar pendência
            let acao = m["outcome"]["acao_necessaria"].as_bool() == Some(true) && m["status"] == "ok";
            if k == 0 && acao {
                let sev = m["outcome"]["severidade"].as_str().map(|s| format!(" ({s})")).unwrap_or_default();
                let mut p = json!({
                    "tipo": "pendencia",
                    "id": format!("{}/{run_id}", t.nome),
                    "titulo": format!("{}: ação necessária{sev}", t.titulo),
                    "origem": m["outcome"]["resumo"].as_str().unwrap_or(&inicio),
                });
                if relatorio.exists() {
                    p["link"] = Value::String(relatorio.display().to_string());
                }
                itens.push(p);
            }
        }
    }
    let v = json!({
        "versao": 1,
        "fonte": "agentes",
        "descricao": "claude-hooks",
        "atualizado": Local::now().to_rfc3339(),
        "itens": itens,
    });
    let texto = serde_json::to_string_pretty(&v).unwrap_or_default();
    // só regrava quando muda algo além do horário, para a barra não reler à toa
    let sem_hora = |s: &str| s.lines().filter(|l| !l.contains("\"atualizado\"")).collect::<Vec<_>>().join("\n");
    let atual = std::fs::read_to_string(&destino).unwrap_or_default();
    if sem_hora(&atual) == sem_hora(&texto) {
        return;
    }
    if let Err(e) = gravar_atomico(&destino, texto.as_bytes()) {
        registrar(&format!("não gravou {}: {e}", destino.display()));
    }
}
