//! Painéis de rede, de volume e o calendário, que abrem embaixo do ícone (ou do relógio), no
//! lugar das configurações rápidas e do calendário do Windows. Aqui ficam os dados mostrados e as linhas de cada painel; o desenho é do
//! `desenho` e as mensagens da janela são do `main`.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::HICON;

use crate::audio::{self, Audio};
use crate::bluetooth::{self, Situacao};
use crate::desenho::{AlvoPainel, AlvosPainel, Dia, LinhaPainel, TipoDia, Tom};
use crate::rede;

/// Largura do painel a 100% de escala.
pub const LARGURA: f32 = 300.0;

const G_CONFIG: &str = "\u{f0493}";
const G_NOTIFICACOES: &str = "\u{f009a}";

/// (texto, URI) dos atalhos para as Configurações do Windows, no pé dos painéis.
pub const LINKS: [(&str, &str); 4] = [
    ("Configurações de som", "ms-settings:sound"),
    ("Configurações de Bluetooth", "ms-settings:bluetooth"),
    ("Configurações de rede", "ms-settings:network-status"),
    ("Configurações de data e hora", "ms-settings:dateandtime"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tipo {
    Volume,
    Rede,
    Calendario,
}

/// Pedido ao Bluetooth em andamento (roda numa thread à parte).
#[derive(Clone, Copy, PartialEq)]
pub enum Pedido {
    Radio,
    Conectar(u64),
    Desconectar(u64),
}

#[derive(Default)]
pub struct Painel {
    pub hwnd: HWND,
    pub tipo: Option<Tipo>,
    pub alvos: AlvosPainel,
    pub hover: Option<AlvoPainel>,
    /// Trilho sendo arrastado.
    pub arrasto: Option<AlvoPainel>,
    pub saidas: Vec<audio::Saida>,
    pub programas: Vec<audio::Programa>,
    /// Ícone de cada programa (da janela dele), na ordem de `programas`.
    pub icones: Vec<HICON>,
    pub placas: Vec<rede::Placa>,
    /// None enquanto a primeira leitura não volta.
    pub bt: Option<Situacao>,
    pub pedido: Option<Pedido>,
    /// Leitura do Bluetooth em andamento.
    pub lendo: bool,
    /// Segundos com o painel aberto (o Bluetooth é relido a cada 3).
    pub ticks: u32,
    /// (ano, mês 1..12) mostrado no calendário.
    pub mes: (i32, u32),
}

impl Painel {
    pub fn linhas(&self, audio: Option<&Audio>) -> Vec<LinhaPainel> {
        match self.tipo {
            Some(Tipo::Volume) => self.linhas_volume(audio),
            Some(Tipo::Rede) => self.linhas_rede(),
            Some(Tipo::Calendario) => self.linhas_calendario(),
            None => Vec::new(),
        }
    }

    fn link(i: usize, separado: bool) -> LinhaPainel {
        LinhaPainel::Item {
            glifo: G_CONFIG,
            texto: LINKS[i].0.into(),
            dir: String::new(),
            tom: Tom::Normal,
            marcado: false,
            sel: false,
            alvo: Some(AlvoPainel::Link(i)),
            separado,
        }
    }

    fn linhas_volume(&self, audio: Option<&Audio>) -> Vec<LinhaPainel> {
        let (nivel, mudo) = audio.and_then(|a| a.nivel()).unwrap_or((0.0, true));
        let num = if mudo { "mudo".to_string() } else { format!("{}%", (nivel * 100.0).round() as i32) };
        let mut v = vec![
            LinhaPainel::Titulo { texto: "Volume".into(), dir: num },
            LinhaPainel::Nivel { glifo: audio::glifo(nivel, mudo), mudo, nivel },
            LinhaPainel::Rotulo { texto: "saída".into(), chave: None },
        ];
        for (i, s) in self.saidas.iter().enumerate() {
            v.push(LinhaPainel::Item {
                glifo: s.glifo,
                texto: s.nome.clone(),
                dir: String::new(),
                tom: Tom::Normal,
                marcado: s.padrao,
                sel: s.padrao,
                alvo: Some(AlvoPainel::Saida(i)),
                separado: false,
            });
        }
        if !self.programas.is_empty() {
            v.push(LinhaPainel::Rotulo { texto: "programas".into(), chave: None });
            for (i, p) in self.programas.iter().enumerate() {
                v.push(LinhaPainel::Programa {
                    icone: self.icones.get(i).copied().unwrap_or_default(),
                    nome: p.nome.clone(),
                    nivel: p.nivel,
                    mudo: p.mudo,
                    alvo: AlvoPainel::Programa(i),
                });
            }
            v.push(LinhaPainel::Espaco(6.0));
        }
        v.push(Self::link(0, true));
        v
    }

    fn linhas_rede(&self) -> Vec<LinhaPainel> {
        let mut v = vec![LinhaPainel::Titulo { texto: "Rede".into(), dir: String::new() }];
        if self.placas.is_empty() {
            v.push(LinhaPainel::Item {
                glifo: rede::G_SEM_REDE,
                texto: "Nenhuma placa de rede".into(),
                dir: String::new(),
                tom: Tom::Apagado,
                marcado: false,
                sel: false,
                alvo: None,
                separado: false,
            });
        }
        for p in &self.placas {
            let (dir, tom) = if !p.conectada {
                ("desconectada".to_string(), Tom::Apagado)
            } else if p.ip.is_empty() {
                ("conectada".to_string(), Tom::Destaque)
            } else {
                (p.ip.clone(), if p.internet { Tom::Destaque } else { Tom::Normal })
            };
            v.push(LinhaPainel::Item { glifo: p.glifo, texto: p.nome.clone(), dir, tom, marcado: false, sel: p.internet, alvo: None, separado: false });
        }

        let nada = |texto: &str| LinhaPainel::Item {
            glifo: bluetooth::G_BLUETOOTH_DESLIGADO,
            texto: texto.into(),
            dir: String::new(),
            tom: Tom::Apagado,
            marcado: false,
            sel: false,
            alvo: None,
            separado: false,
        };
        match &self.bt {
            None => {
                v.push(LinhaPainel::Rotulo { texto: "bluetooth".into(), chave: None });
                v.push(nada("Lendo os dispositivos…"));
            }
            Some(Situacao { radio: None, .. }) => {
                v.push(LinhaPainel::Rotulo { texto: "bluetooth".into(), chave: None });
                v.push(nada("Sem adaptador Bluetooth"));
            }
            Some(Situacao { radio: Some(ligado), dispositivos }) => {
                let texto = if self.pedido == Some(Pedido::Radio) {
                    if *ligado { "Bluetooth  desligando…" } else { "Bluetooth  ligando…" }
                } else {
                    "Bluetooth"
                };
                v.push(LinhaPainel::Rotulo { texto: texto.into(), chave: Some(*ligado) });
                if !ligado {
                    v.push(nada("Desligado"));
                } else if dispositivos.is_empty() {
                    v.push(nada("Nenhum dispositivo pareado"));
                }
                for (i, d) in dispositivos.iter().enumerate().filter(|_| *ligado) {
                    let (dir, tom) = match self.pedido {
                        Some(Pedido::Conectar(e)) if e == d.endereco => ("conectando…".to_string(), Tom::Amarelo),
                        Some(Pedido::Desconectar(e)) if e == d.endereco => ("desconectando…".to_string(), Tom::Amarelo),
                        _ if d.conectado => ("conectado".to_string(), Tom::Destaque),
                        _ => (String::new(), Tom::Normal),
                    };
                    v.push(LinhaPainel::Item {
                        glifo: d.glifo,
                        texto: d.nome.clone(),
                        dir,
                        tom,
                        marcado: false,
                        sel: d.conectado,
                        alvo: Some(AlvoPainel::Bluetooth(i)),
                        separado: false,
                    });
                }
            }
        }
        v.push(Self::link(1, true));
        v.push(Self::link(2, false));
        v
    }

    /// Seis semanas a partir do domingo da semana do dia 1, sempre: o painel não muda de
    /// altura ao trocar de mês.
    fn linhas_calendario(&self) -> Vec<LinhaPainel> {
        let hoje = hoje();
        let (a, m) = self.mes;
        let nome = crate::MESES[(m as usize + 11) % 12];
        let mut inicial = nome.chars();
        let nome: String = inicial.next().map(|c| c.to_uppercase().chain(inicial).collect()).unwrap_or_default();
        let mut v = vec![
            LinhaPainel::Mes { texto: format!("{nome} {a}"), fora: (a, m) != (hoje.0, hoje.1) },
            LinhaPainel::DiasSemana,
        ];
        let (aa, ma) = somar_mes((a, m), -1);
        let (n_ant, n) = (dias_no_mes(aa, ma) as i32, dias_no_mes(a, m) as i32);
        let primeiro = dia_da_semana(a, m, 1) as i32;
        for semana in 0..6 {
            let mut dias = [Dia { n: 0, tipo: TipoDia::Fora }; 7];
            for (col, dia) in dias.iter_mut().enumerate() {
                let i = semana * 7 + col as i32 - primeiro;
                *dia = if i < 0 {
                    Dia { n: (n_ant + i + 1) as u32, tipo: TipoDia::Fora }
                } else if i >= n {
                    Dia { n: (i - n + 1) as u32, tipo: TipoDia::Fora }
                } else {
                    let d = (i + 1) as u32;
                    let tipo = if (a, m, d) == hoje {
                        TipoDia::Hoje
                    } else if col == 0 || col == 6 {
                        TipoDia::FimDeSemana
                    } else {
                        TipoDia::Util
                    };
                    Dia { n: d, tipo }
                };
            }
            v.push(LinhaPainel::Semana(dias));
        }
        v.push(LinhaPainel::Espaco(6.0));
        v.push(LinhaPainel::Item {
            glifo: G_NOTIFICACOES,
            texto: "Notificações".into(),
            dir: String::new(),
            tom: Tom::Normal,
            marcado: false,
            sel: false,
            alvo: Some(AlvoPainel::Notificacoes),
            separado: true,
        });
        v.push(Self::link(3, false));
        v
    }

    /// Avança ou volta `passo` meses no calendário.
    pub fn mudar_mes(&mut self, passo: i32) {
        self.mes = somar_mes(self.mes, passo);
    }

    pub fn ir_para_hoje(&mut self) {
        let (a, m, _) = hoje();
        self.mes = (a, m);
    }

    /// Nível 0..1 no ponto `x` do trilho `alvo`.
    pub fn nivel_em(&self, alvo: AlvoPainel, x: f32) -> Option<f32> {
        let (r, _) = self.alvos.iter().find(|(_, a)| *a == alvo)?;
        Some(((x - r.left) / (r.right - r.left)).clamp(0.0, 1.0))
    }

    /// Ajusta o trilho `alvo` (geral ou de um programa) ao nível `n`.
    pub fn definir_nivel(&mut self, audio: Option<&Audio>, alvo: AlvoPainel, n: f32) {
        let n = (n * 100.0).round() / 100.0;
        match alvo {
            AlvoPainel::Nivel => {
                if let Some(a) = audio {
                    a.definir(n);
                }
            }
            AlvoPainel::Programa(i) => {
                if let Some(p) = self.programas.get_mut(i) {
                    p.definir(n);
                    p.nivel = n;
                    p.mudo = false;
                }
            }
            _ => {}
        }
    }

    /// Soma `passo` ao trilho `alvo` (roda do mouse).
    pub fn somar(&mut self, audio: Option<&Audio>, alvo: AlvoPainel, passo: f32) {
        match alvo {
            AlvoPainel::Programa(i) => {
                if let Some(n) = self.programas.get(i).map(|p| p.nivel) {
                    self.definir_nivel(audio, alvo, n + passo);
                }
            }
            _ => {
                if let Some(a) = audio {
                    a.somar(passo);
                }
            }
        }
    }
}

/// (ano, mês, dia) de hoje.
pub fn hoje() -> (i32, u32, u32) {
    let t = unsafe { GetLocalTime() };
    (t.wYear as i32, t.wMonth as u32, t.wDay as u32)
}

fn somar_mes((a, m): (i32, u32), passo: i32) -> (i32, u32) {
    let i = a * 12 + m as i32 - 1 + passo;
    (i.div_euclid(12), (i.rem_euclid(12) + 1) as u32)
}

fn bissexto(a: i32) -> bool {
    (a % 4 == 0 && a % 100 != 0) || a % 400 == 0
}

fn dias_no_mes(a: i32, m: u32) -> u32 {
    match m {
        2 if bissexto(a) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// 0 = domingo (algoritmo de Sakamoto).
fn dia_da_semana(a: i32, m: u32, d: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let a = if m < 3 { a - 1 } else { a };
    (a + a.div_euclid(4) - a.div_euclid(100) + a.div_euclid(400) + T[m as usize - 1] + d as i32).rem_euclid(7) as u32
}

/// Número da semana ISO 8601 (começa na segunda; a primeira do ano tem a primeira quinta-feira).
pub fn semana_iso(a: i32, m: u32, d: u32) -> u32 {
    let dia_do_ano = (1..m).map(|k| dias_no_mes(a, k)).sum::<u32>() + d;
    let ds = (dia_da_semana(a, m, d) + 6) % 7 + 1; // 1 = segunda .. 7 = domingo
    let w = (dia_do_ano as i32 - ds as i32 + 10) / 7;
    let semanas = |a: i32| {
        let jan1 = dia_da_semana(a, 1, 1);
        if jan1 == 4 || (jan1 == 3 && bissexto(a)) { 53i32 } else { 52 }
    };
    if w < 1 {
        semanas(a - 1) as u32
    } else if w > semanas(a) {
        1
    } else {
        w as u32
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn datas() {
        assert_eq!(dia_da_semana(2026, 10, 23), 5);
        assert_eq!(dia_da_semana(2024, 2, 29), 4);
        assert_eq!(somar_mes((2026, 1), -1), (2025, 12));
        assert_eq!(somar_mes((2026, 12), 1), (2027, 1));
        assert_eq!(semana_iso(2026, 10, 23), 43);
        assert_eq!(semana_iso(2021, 1, 3), 53);
        assert_eq!(semana_iso(2024, 12, 30), 1);
        assert_eq!(semana_iso(2026, 1, 1), 1);
    }
}
