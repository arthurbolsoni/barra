//! Painéis de rede e de volume que abrem embaixo do ícone, no lugar das configurações rápidas
//! do Windows. Aqui ficam os dados mostrados e as linhas de cada painel; o desenho é do
//! `desenho` e as mensagens da janela são do `main`.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::HICON;

use crate::audio::{self, Audio};
use crate::bluetooth::{self, Situacao};
use crate::desenho::{AlvoPainel, AlvosPainel, LinhaPainel, Tom};
use crate::rede;

/// Largura do painel a 100% de escala.
pub const LARGURA: f32 = 300.0;

const G_CONFIG: &str = "\u{f0493}";

/// (texto, URI) dos atalhos para as Configurações do Windows, no pé dos painéis.
pub const LINKS: [(&str, &str); 3] = [
    ("Configurações de som", "ms-settings:sound"),
    ("Configurações de Bluetooth", "ms-settings:bluetooth"),
    ("Configurações de rede", "ms-settings:network-status"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tipo {
    Volume,
    Rede,
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
}

impl Painel {
    pub fn linhas(&self, audio: Option<&Audio>) -> Vec<LinhaPainel> {
        match self.tipo {
            Some(Tipo::Volume) => self.linhas_volume(audio),
            Some(Tipo::Rede) => self.linhas_rede(),
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
