//! Grupos de janelas. Só as janelas do grupo ativo ficam na tela; as dos outros ficam
//! estacionadas à direita, fora de todos os monitores. Não são minimizadas nem escondidas:
//! o Windows continua desenhando, e a miniatura delas segue ao vivo.

use std::path::PathBuf;
use std::sync::Mutex;

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::janelas::{existe, hwnd};

pub struct Grupo {
    pub nome: String,
    pub janelas: Vec<isize>,
    /// Última janela em foco no grupo, para voltar a ela.
    pub foco: isize,
}

impl Grupo {
    fn novo(nome: String) -> Self {
        Self { nome, janelas: Vec::new(), foco: 0 }
    }
}

pub struct Grupos {
    pub lista: Vec<Grupo>,
    pub ativo: usize,
    contador: usize,
}

/// Janela estacionada e onde ela estava. Fora do estado da barra para o gancho de pânico
/// conseguir trazer tudo de volta mesmo no meio de outra operação.
static ESTACIONADAS: Mutex<Vec<(isize, RECT)>> = Mutex::new(Vec::new());

fn arquivo() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("barra").join("estacionadas.txt")
}

/// Grava as estacionadas: se a barra morrer, a próxima execução traz todas de volta.
fn salvar(lista: &[(isize, RECT)]) {
    let caminho = arquivo();
    if lista.is_empty() {
        _ = std::fs::remove_file(caminho);
        return;
    }
    let txt: String = lista.iter().map(|(h, r)| format!("{} {} {} {} {}\n", h, r.left, r.top, r.right, r.bottom)).collect();
    if let Some(dir) = caminho.parent() {
        _ = std::fs::create_dir_all(dir);
    }
    _ = std::fs::write(caminho, txt);
}

fn mover_para(h: isize, x: i32, y: i32) {
    unsafe {
        _ = SetWindowPos(hwnd(h), None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
    }
}

fn retangulo(h: isize) -> RECT {
    let mut r = RECT::default();
    unsafe { _ = GetWindowRect(hwnd(h), &mut r) };
    r
}

fn estacionar(h: isize) {
    // minimizada fica minimizada: não aparece na tela de qualquer jeito
    if unsafe { IsIconic(hwnd(h)).as_bool() } {
        return;
    }
    let mut lista = ESTACIONADAS.lock().unwrap();
    if lista.iter().any(|(e, _)| *e == h) {
        return;
    }
    let r = retangulo(h);
    let (vx, vw) = unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN)) };
    mover_para(h, vx + vw + 2000 + (r.left - vx).max(0), r.top);
    lista.push((h, r));
    salvar(&lista);
}

fn desestacionar(h: isize) {
    let mut lista = ESTACIONADAS.lock().unwrap();
    let Some(i) = lista.iter().position(|(e, _)| *e == h) else { return };
    let (_, r) = lista.remove(i);
    if existe(h) {
        mover_para(h, r.left, r.top);
    }
    salvar(&lista);
}

/// Traz de volta tudo o que está estacionado (saída, desligamento, pânico).
pub fn trazer_todas_de_volta() {
    let Ok(mut lista) = ESTACIONADAS.try_lock() else { return };
    for (h, r) in lista.drain(..) {
        if existe(h) {
            mover_para(h, r.left, r.top);
        }
    }
    salvar(&lista);
}

/// Na abertura: janelas que ficaram estacionadas porque a barra morreu voltam ao lugar.
pub fn recuperar() {
    let Ok(txt) = std::fs::read_to_string(arquivo()) else { return };
    for linha in txt.lines() {
        let v: Vec<i64> = linha.split_whitespace().filter_map(|s| s.parse().ok()).collect();
        if let [h, l, t, _, _] = v[..] {
            if existe(h as isize) {
                mover_para(h as isize, l as i32, t as i32);
            }
        }
    }
    _ = std::fs::remove_file(arquivo());
}


fn primeiro_plano() -> isize {
    unsafe { GetAncestor(GetForegroundWindow(), GA_ROOTOWNER).0 as isize }
}

impl Grupos {
    pub fn new() -> Self {
        Self { lista: vec![Grupo::novo("1".into())], ativo: 0, contador: 1 }
    }

    pub fn grupo_de(&self, h: isize) -> Option<usize> {
        self.lista.iter().position(|g| g.janelas.contains(&h))
    }

    /// Acerta os grupos com as janelas que existem agora: some quem fechou, janela nova
    /// entra no grupo ativo. Devolve se algo mudou.
    pub fn sincronizar(&mut self, atuais: &[isize]) -> bool {
        let mut mudou = false;
        for g in &mut self.lista {
            let antes = g.janelas.len();
            g.janelas.retain(|h| {
                let fica = atuais.contains(h);
                if !fica && existe(*h) {
                    // escondeu (foi para a bandeja, por exemplo): volta ao lugar para não
                    // reaparecer fora da tela
                    desestacionar(*h);
                }
                fica
            });
            mudou |= g.janelas.len() != antes;
        }
        {
            let mut lista = ESTACIONADAS.lock().unwrap();
            let antes = lista.len();
            lista.retain(|(h, _)| existe(*h));
            if lista.len() != antes {
                salvar(&lista);
            }
        }
        for &h in atuais {
            if self.grupo_de(h).is_none() {
                self.lista[self.ativo].janelas.push(h);
                mudou = true;
            }
        }
        mudou
    }

    /// Lembra a janela em foco do grupo ativo.
    pub fn anotar_foco(&mut self, h: isize) {
        if let Some(g) = self.grupo_de(h) {
            self.lista[g].foco = h;
        }
    }

    /// Troca de grupo e devolve a janela que deve ganhar foco.
    pub fn trocar(&mut self, para: usize) -> Option<isize> {
        if para >= self.lista.len() {
            return None;
        }
        if para != self.ativo {
            let fg = primeiro_plano();
            if self.lista[self.ativo].janelas.contains(&fg) {
                self.lista[self.ativo].foco = fg;
            }
            for &h in &self.lista[self.ativo].janelas {
                estacionar(h);
            }
            self.ativo = para;
            for &h in &self.lista[para].janelas {
                desestacionar(h);
            }
        }
        let g = &self.lista[para];
        let visivel = |h: &isize| unsafe { !IsIconic(hwnd(*h)).as_bool() };
        if g.janelas.contains(&g.foco) && visivel(&g.foco) {
            Some(g.foco)
        } else {
            g.janelas.iter().copied().find(visivel)
        }
    }

    /// Passa a janela para outro grupo.
    pub fn mover(&mut self, h: isize, para: usize) {
        let Some(de) = self.grupo_de(h) else { return };
        if de == para || para >= self.lista.len() {
            return;
        }
        self.lista[de].janelas.retain(|&x| x != h);
        self.lista[para].janelas.push(h);
        if para == self.ativo {
            desestacionar(h);
        } else if de == self.ativo {
            estacionar(h);
        }
    }

    pub fn novo(&mut self) -> usize {
        self.contador += 1;
        self.lista.push(Grupo::novo(self.contador.to_string()));
        self.lista.len() - 1
    }

    /// Apaga o grupo; as janelas dele vão para o grupo anterior.
    pub fn apagar(&mut self, i: usize) {
        if self.lista.len() < 2 || i >= self.lista.len() {
            return;
        }
        let destino = if i > 0 { i - 1 } else { 1 };
        for h in self.lista[i].janelas.clone() {
            self.mover(h, destino);
        }
        if self.ativo == i {
            self.trocar(destino);
        }
        self.lista.remove(i);
        if self.ativo > i {
            self.ativo -= 1;
        }
    }

    /// Junta todas as janelas no grupo ativo e apaga os outros.
    pub fn juntar_tudo(&mut self) {
        trazer_todas_de_volta();
        let ativo = self.ativo;
        let mut todas = Vec::new();
        for g in &mut self.lista {
            todas.append(&mut g.janelas);
        }
        let mut g = self.lista.swap_remove(ativo);
        g.janelas = todas;
        self.lista = vec![g];
        self.ativo = 0;
    }
}
