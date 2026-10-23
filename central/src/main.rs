//! Serviço da central da barra. Cada fonte da central grava um JSON em
//! `%LOCALAPPDATA%\barra\central\`; a barra lê a pasta e desenha. Este programa cuida de duas:
//!
//! - `barra-central` (sem argumento): sincroniza as agendas (iCal do Google e do Outlook) a cada poucos minutos
//!   (`agenda.json`) e lê as tarefas do claude-hooks a cada 30 segundos (`agentes.json`). Fica
//!   rodando; uma instância só. A barra abre o serviço ao iniciar.
//! - `barra-central hook`: chamado pelos hooks do Claude Code com o evento no stdin; grava o
//!   estado da sessão em `claude\<sessão>.json`.
//! - `barra-central agenda [de] [ate]`: lê as agendas na hora e imprime os eventos (skill `agenda`).

// programa de console: o shell dos hooks espera ele terminar (a barra abre o serviço sem janela)

mod agenda;
mod agentes;
mod ical;
mod claude;

use std::io::Write;
use std::path::PathBuf;

/// `%LOCALAPPDATA%\barra`.
pub fn base() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("barra")
}

/// Pasta lida pela barra.
pub fn pasta_central() -> PathBuf {
    base().join("central")
}

/// Grava por cima sem deixar o arquivo pela metade para quem lê ao mesmo tempo.
pub fn gravar_atomico(caminho: &std::path::Path, conteudo: &[u8]) -> std::io::Result<()> {
    if let Some(p) = caminho.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = caminho.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, conteudo)?;
    std::fs::rename(&tmp, caminho)
}

/// Linha no `central.log` (limitado a ~256 KB: passando disso, recomeça).
pub fn registrar(msg: &str) {
    let caminho = base().join("central.log");
    if std::fs::metadata(&caminho).is_ok_and(|m| m.len() > 256 * 1024) {
        _ = std::fs::remove_file(&caminho);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&caminho) {
        _ = writeln!(f, "{} {msg}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("hook") => claude::hook(),
        Some("sair") => agenda::pedir_saida(),
        Some("agentes") => agentes::sincronizar(),
        Some("agenda") => agenda::consultar(&args[2..]),
        _ => agenda::servico(),
    }
}
