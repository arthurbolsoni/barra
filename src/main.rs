#![windows_subsystem = "windows"]

mod appbar;
mod audio;
mod bluetooth;
mod central;
mod bandeja;
mod cores_do_windows;
mod desenho;
mod grupos;
mod janelas;
mod menus_externos;
mod molduras;
mod painel;
mod rede;
mod sistema;
mod tema;
mod teclado;
mod visao;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_CHECKED, ODS_DISABLED, ODS_GRAYED, ODS_SELECTED, ODT_MENU};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use desenho::{Alvo, AlvoPainel, Alvos, CartaoV, Cena, CenaPainel, CenaVisao, ItemBandeja, ItemGrupo, ItemJanela, JanelaV};
use janelas::hwnd;
use teclado::{forcar_primeiro_plano, MODO_ALT};

/// Altura da barra a 100% de escala.
const ALTURA: f32 = 26.0;
/// Preferência: índice do tema do Omarchy.
const PREF_TEMA: PCWSTR = w!("Tema");
const CLASSE: PCWSTR = w!("BarraTarefas");
const TITULO: PCWSTR = w!("barra");
const WM_APPBAR: u32 = WM_APP + 1;
const WM_PRIMEIRO_PLANO: u32 = WM_APP + 2;
const WM_RENOMEAR: u32 = WM_APP + 3;
/// Leitura do Bluetooth terminada (lParam: Box<bluetooth::Situacao>).
const WM_BT_LIDO: u32 = WM_APP + 4;
/// Pedido ao Bluetooth terminado (lParam: Box<bluetooth::Situacao> lida depois dele).
const WM_BT_FEITO: u32 = WM_APP + 5;
/// Clique fora do painel aberto (lParam: ponto na tela).
const WM_CLIQUE_FORA: u32 = WM_APP + 6;
const TIMER: usize = 1;
const DIAS: [&str; 7] = ["Domingo", "Segunda-feira", "Terça-feira", "Quarta-feira", "Quinta-feira", "Sexta-feira", "Sábado"];
const MESES: [&str; 12] = [
    "janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro",
];
const MENU_TEMA: usize = 200;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Preferência: Alt+Tab abre a visão geral (padrão: sim).
const PREF_ALT_TAB: PCWSTR = w!("AltTab");

// avisos do gancho de shell (RegisterShellHookWindow)
const HSHELL_CRIADA: usize = 1;
const HSHELL_DESTRUIDA: usize = 2;
const HSHELL_ATIVADA: usize = 4;
const HSHELL_REDESENHO: usize = 6;
const HSHELL_ATIVADA_RUDE: usize = 0x8004;
const HSHELL_PISCA: usize = 0x8006;

// avisos da AppBar
const ABN_ESTADO: usize = 0;
const ABN_POSICAO: usize = 1;
const ABN_TELA_CHEIA: usize = 2;

struct Estado {
    barra: HWND,
    /// Janela oculta, dona dos menus da barra.
    menus: HWND,
    /// Dica (tooltip) dos botões e dos ícones da bandeja.
    dica: HWND,
    dica_texto: String,
    visao_hwnd: HWND,
    gfx: desenho::Gfx,
    grupos: grupos::Grupos,
    visao: visao::Visao,
    /// Janelas da mais recente para a mais antiga em foco (ordem do Alt+Tab).
    mru: Vec<isize>,
    alvos: Alvos,
    hover: Option<Alvo>,
    piscando: HashSet<isize>,
    icones: HashMap<isize, HICON>,
    /// Glifo no lugar do ícone, por janela (calculado uma vez).
    glifos: HashMap<isize, Option<&'static str>>,
    /// Glifo de terminal do tema atual como ícone (nome do tema, grande, pequeno).
    icone_terminal: Option<(&'static str, HICON, HICON)>,
    msg_shell: u32,
    msg_barra_criada: u32,
    esconder_windows: bool,
    escala: f32,
    /// Painel de rede ou de volume aberto embaixo do ícone.
    painel: painel::Painel,
    audio: Option<audio::Audio>,
    /// Glifo da rede, se tem conexão, e a dica.
    rede: (&'static str, bool, String),
    central: central::Central,
}

thread_local! {
    static ESTADO: RefCell<Option<Estado>> = const { RefCell::new(None) };
}

/// Janela da barra, para o gancho de primeiro plano postar mesmo com o estado em uso.
static BARRA: AtomicIsize = AtomicIsize::new(0);
static VISAO: AtomicIsize = AtomicIsize::new(0);
static DICA: AtomicIsize = AtomicIsize::new(0);
static PAINEL: AtomicIsize = AtomicIsize::new(0);
static CENTRAL: AtomicIsize = AtomicIsize::new(0);

/// Acesso ao estado. Devolve None se ele já está em uso (mensagem que chegou no meio de
/// outra operação, enquanto o Windows espera outra janela responder).
fn com<R>(f: impl FnOnce(&mut Estado) -> R) -> Option<R> {
    ESTADO.with(|e| e.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn invalidar(h: HWND) {
    unsafe { _ = InvalidateRect(Some(h), None, false) };
}

fn redesenhar(st: &Estado) {
    invalidar(st.barra);
    if st.visao.aberta {
        invalidar(st.visao_hwnd);
    }
}

fn raiz(h: HWND) -> isize {
    unsafe { GetAncestor(h, GA_ROOTOWNER).0 as isize }
}

fn primeiro_plano() -> isize {
    raiz(unsafe { GetForegroundWindow() })
}

fn atualizar(st: &mut Estado) {
    let atuais = janelas::listar();
    st.grupos.sincronizar(&atuais);
    st.icones.retain(|h, _| atuais.contains(h));
    st.glifos.retain(|h, _| atuais.contains(h));
    st.piscando.retain(|h| atuais.contains(h));
    st.mru.retain(|h| atuais.contains(h));
    // glifo de terminal do tema para a barra de título dos terminais; refeito quando o tema muda
    // (os de temas anteriores não são destruídos: alguma janela pode ainda estar com eles)
    if st.icone_terminal.map(|(n, _, _)| n) != Some(st.gfx.tema.nome) {
        let cor = st.gfx.tema.texto;
        let tam = (16.0 * st.escala).round() as i32;
        st.icone_terminal = desenho::icone_de_glifo(desenho::G_TERMINAL, tam * 2, cor).zip(desenho::icone_de_glifo(desenho::G_TERMINAL, tam, cor)).map(|(g, p)| (st.gfx.tema.nome, g, p));
    }
    molduras::sincronizar(st.gfx.tema, &atuais, primeiro_plano(), st.icone_terminal.map(|(_, g, p)| (g, p)));
    // menus dos outros programas no modo claro ou escuro do tema, junto com as molduras
    if molduras::ligadas() {
        menus_externos::ligar(st.gfx.tema);
    } else {
        menus_externos::desligar();
    }
    cores_do_windows::aplicar(st.gfx.tema);
    for h in atuais {
        if !st.mru.contains(&h) {
            st.mru.push(h);
        }
    }
    if st.visao.aberta && st.visao.editando.is_none() && st.visao.arrasto.is_none() && visao::assinatura(&st.grupos) != st.visao.assinatura {
        remontar_visao(st);
    }
    redesenhar(st);
}

fn altura(st: &Estado) -> i32 {
    (ALTURA * st.escala).round() as i32
}

fn posicionar(st: &mut Estado) {
    st.escala = unsafe { GetDpiForWindow(st.barra) } as f32 / 96.0;
    appbar::posicionar(st.barra, altura(st));
    redesenhar(st);
}

fn icone_de(st: &mut Estado, j: isize) -> HICON {
    let tam = (32.0 * st.escala) as i32;
    *st.icones.entry(j).or_insert_with(|| janelas::icone(hwnd(j), tam))
}

fn cena(st: &mut Estado, w: i32, h: i32) -> Cena {
    let fg = primeiro_plano();
    let ativo = st.grupos.ativo;
    let grupos = st
        .grupos
        .lista
        .iter()
        .enumerate()
        .map(|(i, g)| ItemGrupo { nome: g.nome.clone(), qtd: g.janelas.len(), ativo: i == ativo, proprio: g.nome_proprio })
        .collect();
    let lista: Vec<(usize, isize)> = st.grupos.lista.iter().enumerate().flat_map(|(g, gr)| gr.janelas.iter().map(move |&j| (g, j))).collect();
    let janelas = lista
        .into_iter()
        .map(|(g, j)| {
            let p = bandeja::progresso(j);
            ItemJanela {
                hwnd: j,
                grupo: g,
                icone: icone_de(st, j),
                ativa: j == fg,
                minimizada: unsafe { IsIconic(hwnd(j)).as_bool() },
                piscando: st.piscando.contains(&j),
                selo: p.selo,
                progresso: (p.estado != 0).then_some((p.estado, p.fracao)),
                glifo: *st.glifos.entry(j).or_insert_with(|| janelas::glifo(hwnd(j))),
            }
        })
        .collect();
    let t = unsafe { GetLocalTime() };
    let agora = chrono::Local::now().naive_local();
    Cena {
        w,
        h,
        escala: st.escala,
        grupos,
        janelas,
        hover: st.hover,
        hora: format!("{} {:02}:{:02}", DIAS[t.wDayOfWeek as usize % 7], t.wHour, t.wMinute),
        bandeja: bandeja::visiveis().into_iter().map(|(indice, icone, _, dono)| ItemBandeja { indice, icone, glifo: janelas::glifo(hwnd(dono)) }).collect(),
        rede: (st.rede.0, st.rede.1),
        volume: st.audio.as_ref().and_then(|a| a.nivel()),
        aberto: if st.central.aberta {
            Some(Alvo::Relogio)
        } else {
            st.painel.tipo.map(|t| match t {
                painel::Tipo::Volume => Alvo::Volume,
                painel::Tipo::Rede => Alvo::Rede,
            })
        },
        aviso: st.central.proximo(agora).map(|(t, m)| (format!("{t} em {m} min"), m <= 5)),
        alerta: st.central.alerta(agora.date()),
    }
}

fn alvo_em(alvos: &Alvos, x: i32, y: i32) -> Option<Alvo> {
    let (x, y) = (x as f32, y as f32);
    alvos.iter().find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(_, a)| *a)
}

/// Troca para o grupo e põe em foco `janela` (ou a última em foco do grupo).
fn ir(grupo: usize, janela: Option<isize>) {
    let foco = com(|st| {
        let f = st.grupos.trocar(grupo);
        redesenhar(st);
        f
    })
    .flatten();
    if let Some(h) = janela.or(foco) {
        forcar_primeiro_plano(hwnd(h));
    } else {
        // grupo vazio: o foco vai para a área de trabalho; se ficasse na janela de antes (agora
        // estacionada), a barra veria ela em foco e voltaria para o grupo dela
        forcar_primeiro_plano(unsafe { GetShellWindow() });
    }
}

fn ir_janela(h: isize) {
    if let Some(Some(g)) = com(|st| st.grupos.grupo_de(h)) {
        ir(g, Some(h));
    } else {
        forcar_primeiro_plano(hwnd(h));
    }
}

/// Clique no botão da janela: traz para a frente, ou minimiza se já estava na frente. Janela de
/// outro grupo: troca para o grupo dela e põe ela em foco.
fn clicar_janela(h: isize) {
    if com(|st| st.grupos.grupo_de(h).is_some_and(|g| g != st.grupos.ativo)) == Some(true) {
        ir_janela(h);
        return;
    }
    let j = hwnd(h);
    if primeiro_plano() == h && unsafe { !IsIconic(j).as_bool() } {
        unsafe { _ = ShowWindow(j, SW_MINIMIZE) };
    } else {
        forcar_primeiro_plano(j);
    }
}

fn cursor() -> POINT {
    let mut pt = POINT::default();
    unsafe { _ = GetCursorPos(&mut pt) };
    pt
}

fn clicar(alvo: Alvo) {
    match alvo {
        Alvo::Grupo(i) => ir(i, None),
        Alvo::NovoGrupo => {
            if let Some(i) = com(|st| st.grupos.novo()) {
                ir(i, None);
            }
        }
        Alvo::Janela(h) => clicar_janela(h),
        Alvo::Bandeja(i) => bandeja::clicar(i, bandeja::Botao::Esquerdo, cursor()),
        Alvo::Rede => abrir_painel(painel::Tipo::Rede),
        Alvo::Volume => abrir_painel(painel::Tipo::Volume),
        Alvo::Relogio => alternar_central(),
    }
}

// ---------- dica ----------

fn mostrar_dica(st: &mut Estado) {
    // com o painel aberto a dica cobriria ele
    let hover = st.hover.filter(|_| st.painel.tipo.is_none() && !st.central.aberta);
    let texto = match hover {
        Some(Alvo::Bandeja(i)) => bandeja::visiveis().into_iter().find(|(n, _, _, _)| *n == i).map(|(_, _, d, _)| d).unwrap_or_default(),
        Some(Alvo::Janela(h)) => janelas::titulo(hwnd(h)),
        Some(Alvo::Relogio) => {
            let t = unsafe { GetLocalTime() };
            let semana = central::semana_iso(chrono::Local::now().date_naive());
            format!("{}, {} de {} de {} · semana {semana}", DIAS[t.wDayOfWeek as usize % 7], t.wDay, MESES[(t.wMonth as usize + 11) % 12], t.wYear)
        }
        Some(Alvo::Rede) => st.rede.2.clone(),
        Some(Alvo::Volume) => match st.audio.as_ref().and_then(|a| a.nivel()) {
            Some((_, true)) => "Volume: mudo".into(),
            Some((n, false)) => format!("Volume: {}%", (n * 100.0).round() as i32),
            None => "Sem saída de som".into(),
        },
        Some(Alvo::Grupo(i)) => st.grupos.lista.get(i).map(|g| match g.janelas.len() {
            0 => format!("Grupo {}: nenhuma janela", g.nome),
            1 => format!("Grupo {}: 1 janela", g.nome),
            n => format!("Grupo {}: {n} janelas", g.nome),
        }).unwrap_or_default(),
        _ => String::new(),
    };
    let r = hover.and_then(|a| st.alvos.iter().find(|(_, b)| *b == a).map(|(r, _)| *r));
    let (Some(r), false) = (r, texto.is_empty()) else {
        st.dica_texto.clear();
        unsafe { _ = ShowWindow(st.dica, SW_HIDE) };
        return;
    };
    let (w, h) = st.gfx.medir_dica(&texto, st.escala);
    let mut b = RECT::default();
    unsafe { _ = GetWindowRect(st.barra, &mut b) };
    let m = appbar::monitor();
    let x = (b.left + ((r.left + r.right) / 2.0) as i32 - w / 2).clamp(m.left + 4, m.right - w - 4);
    let y = b.bottom + (6.0 * st.escala) as i32;
    st.dica_texto = texto;
    unsafe {
        _ = SetWindowPos(st.dica, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    invalidar(st.dica);
}

// ---------- painéis de rede e volume ----------

fn renovar_rede(st: &mut Estado) {
    let placas = rede::placas();
    let principal = placas.iter().find(|p| p.internet).or_else(|| placas.iter().find(|p| p.conectada));
    st.rede = match principal {
        Some(p) if p.ip.is_empty() => (p.glifo, true, p.nome.clone()),
        Some(p) => (p.glifo, true, format!("{}: {}", p.nome, p.ip)),
        None => (rede::G_SEM_REDE, false, "Sem conexão de rede".into()),
    };
    if st.painel.tipo == Some(painel::Tipo::Rede) {
        st.painel.placas = placas;
    }
}

/// Ícone de cada programa do painel de volume: o da janela do mesmo executável.
fn icones_programas(st: &mut Estado) -> Vec<HICON> {
    let mut por_exe: HashMap<String, isize> = HashMap::new();
    for g in &st.grupos.lista {
        for &j in &g.janelas {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd(j), Some(&mut pid)) };
            if let Some(e) = audio::caminho_do_processo(pid) {
                por_exe.entry(e.to_lowercase()).or_insert(j);
            }
        }
    }
    let exes: Vec<String> = st.painel.programas.iter().map(|p| p.exe.to_lowercase()).collect();
    exes.iter().map(|e| por_exe.get(e).map(|&j| icone_de(st, j)).unwrap_or_default()).collect()
}

/// Manda a leitura do Bluetooth para a janela do painel (WM_BT_LIDO ou WM_BT_FEITO).
fn devolver_bluetooth(h: isize, msg: u32, s: bluetooth::Situacao) {
    let s = Box::into_raw(Box::new(s));
    unsafe {
        if PostMessageW(Some(hwnd(h)), msg, WPARAM(0), LPARAM(s as isize)).is_err() {
            drop(Box::from_raw(s));
        }
    }
}

/// Lê o Bluetooth numa thread; o resultado volta em WM_BT_LIDO.
fn ler_bluetooth(st: &mut Estado) {
    if st.painel.lendo || st.painel.pedido.is_some() {
        return;
    }
    st.painel.lendo = true;
    let h = st.painel.hwnd.0 as isize;
    std::thread::spawn(move || devolver_bluetooth(h, WM_BT_LIDO, bluetooth::ler()));
}

/// Liga/desliga o rádio ou conecta/desconecta um fone, numa thread. Espera o aparelho mudar
/// de estado (até 10 s) e devolve a leitura de depois em WM_BT_FEITO.
fn pedir_bluetooth(st: &mut Estado, p: painel::Pedido) {
    if st.painel.pedido.is_some() {
        return;
    }
    let ligado = st.painel.bt.as_ref().and_then(|b| b.radio).unwrap_or(false);
    st.painel.pedido = Some(p);
    let h = st.painel.hwnd.0 as isize;
    std::thread::spawn(move || {
        let pronto = |s: &bluetooth::Situacao| match p {
            painel::Pedido::Radio => s.radio == Some(!ligado),
            painel::Pedido::Conectar(e) => s.dispositivos.iter().any(|d| d.endereco == e && d.conectado),
            painel::Pedido::Desconectar(e) => !s.dispositivos.iter().any(|d| d.endereco == e && d.conectado),
        };
        match p {
            painel::Pedido::Radio => bluetooth::ligar_radio(!ligado),
            painel::Pedido::Conectar(e) => bluetooth::conectar(e, true),
            painel::Pedido::Desconectar(e) => bluetooth::conectar(e, false),
        }
        let mut s = bluetooth::ler();
        for _ in 0..10 {
            if pronto(&s) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            s = bluetooth::ler();
        }
        devolver_bluetooth(h, WM_BT_FEITO, s);
    });
}

/// Altura do painel conforme as linhas de agora.
fn ajustar_painel(st: &mut Estado) {
    if st.painel.tipo.is_none() {
        return;
    }
    let h = desenho::altura_painel(&st.painel.linhas(st.audio.as_ref()), st.escala);
    let w = (painel::LARGURA * st.escala).round() as i32;
    unsafe { _ = SetWindowPos(st.painel.hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE) };
    invalidar(st.painel.hwnd);
}

/// Relê o que o painel aberto mostra (a cada segundo e depois de cada ação).
fn renovar_painel(st: &mut Estado) {
    match st.painel.tipo {
        Some(painel::Tipo::Volume) if st.painel.arrasto.is_none() => {
            if let Some(a) = st.audio.as_mut() {
                a.renovar();
                st.painel.saidas = a.saidas();
                st.painel.programas = a.programas();
            }
            st.painel.icones = icones_programas(st);
        }
        Some(painel::Tipo::Rede) => {
            renovar_rede(st);
            if st.painel.ticks % 3 == 0 {
                ler_bluetooth(st);
            }
        }
        _ => {}
    }
    st.painel.ticks += 1;
    ajustar_painel(st);
}

/// Gancho de mouse, ligado só com o painel aberto: clique fora dele fecha.
static GANCHO_MOUSE: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn gancho_mouse(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && matches!(wp.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
        let m = &*(lp.0 as *const MSLLHOOKSTRUCT);
        let p = HWND(PAINEL.load(Ordering::Relaxed) as *mut _);
        let mut r = RECT::default();
        _ = GetWindowRect(p, &mut r);
        if !PtInRect(&r, m.pt).as_bool() {
            let ponto = (m.pt.x as u16 as isize) | ((m.pt.y as u16 as isize) << 16);
            _ = PostMessageW(Some(p), WM_CLIQUE_FORA, WPARAM(0), LPARAM(ponto));
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

fn fechar_painel(st: &mut Estado) {
    if st.painel.tipo.take().is_none() {
        return;
    }
    st.painel.arrasto = None;
    st.painel.hover = None;
    teclado::PAINEL_ABERTO.store(false, Ordering::Relaxed);
    let g = GANCHO_MOUSE.swap(0, Ordering::Relaxed);
    unsafe {
        if g != 0 {
            _ = UnhookWindowsHookEx(HHOOK(g as *mut _));
        }
        _ = ShowWindow(st.painel.hwnd, SW_HIDE);
    }
    redesenhar(st);
}

/// Abre o painel embaixo do ícone dele; clicar no mesmo ícone de novo fecha. O painel não
/// pega o foco (a janela em uso continua com ele): clique fora e Esc chegam pelos ganchos.
fn abrir_painel(tipo: painel::Tipo) {
    _ = com(|st| {
        if st.painel.tipo == Some(tipo) {
            fechar_painel(st);
            return None;
        }
        let alvo = match tipo {
            painel::Tipo::Volume => Alvo::Volume,
            painel::Tipo::Rede => Alvo::Rede,
        };
        let r = st.alvos.iter().find(|(_, a)| *a == alvo).map(|(r, _)| *r)?;
        st.painel.tipo = Some(tipo);
        st.painel.hover = None;
        st.painel.arrasto = None;
        st.painel.ticks = 0;
        renovar_painel(st);
        let mut b = RECT::default();
        unsafe { _ = GetWindowRect(st.barra, &mut b) };
        let m = appbar::monitor();
        let w = (painel::LARGURA * st.escala).round() as i32;
        let h = desenho::altura_painel(&st.painel.linhas(st.audio.as_ref()), st.escala);
        let x = (b.left + r.left as i32 - (12.0 * st.escala) as i32).clamp(m.left, m.right - w);
        unsafe {
            _ = SetWindowPos(st.painel.hwnd, Some(HWND_TOPMOST), x, b.bottom, w, h, SWP_SHOWWINDOW | SWP_NOACTIVATE);
            _ = ShowWindow(st.dica, SW_HIDE);
            if GANCHO_MOUSE.load(Ordering::Relaxed) == 0 {
                let inst = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
                if let Ok(g) = SetWindowsHookExW(WH_MOUSE_LL, Some(gancho_mouse), Some(inst.into()), 0) {
                    GANCHO_MOUSE.store(g.0 as isize, Ordering::Relaxed);
                }
            }
        }
        teclado::PAINEL_ABERTO.store(true, Ordering::Relaxed);
        st.dica_texto.clear();
        invalidar(st.painel.hwnd);
        redesenhar(st);
        Some(())
    });
}

fn alvo_painel_em(alvos: &desenho::AlvosPainel, x: i32, y: i32) -> Option<AlvoPainel> {
    let (x, y) = (x as f32, y as f32);
    alvos.iter().find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(_, a)| *a)
}

fn abrir_link(i: usize) {
    let u: Vec<u16> = painel::LINKS[i].1.encode_utf16().chain([0]).collect();
    sistema::abrir_uri(PCWSTR(u.as_ptr()));
}

/// Clique no painel.
fn clicar_painel(st: &mut Estado, alvo: AlvoPainel, x: i32) {
    match alvo {
        AlvoPainel::Nivel | AlvoPainel::Programa(_) => {
            unsafe { SetCapture(st.painel.hwnd) };
            st.painel.arrasto = Some(alvo);
            if let Some(n) = st.painel.nivel_em(alvo, x as f32) {
                st.painel.definir_nivel(st.audio.as_ref(), alvo, n);
            }
        }
        AlvoPainel::Mudo => {
            if let Some(a) = &st.audio {
                a.alternar_mudo();
            }
        }
        AlvoPainel::Saida(i) => {
            let Some(id) = st.painel.saidas.get(i).map(|s| s.id.clone()) else { return };
            if let Some(a) = st.audio.as_mut() {
                a.definir_saida(&id);
            }
            renovar_painel(st);
        }
        AlvoPainel::Radio => pedir_bluetooth(st, painel::Pedido::Radio),
        AlvoPainel::Bluetooth(i) => {
            let Some(d) = st.painel.bt.as_ref().and_then(|b| b.dispositivos.get(i)).cloned() else { return };
            if d.audio {
                pedir_bluetooth(st, if d.conectado { painel::Pedido::Desconectar(d.endereco) } else { painel::Pedido::Conectar(d.endereco) });
            } else {
                // mouse, teclado, controle: o Windows conecta sozinho; aqui só abre as configurações
                fechar_painel(st);
                abrir_link(1);
                return;
            }
        }
        AlvoPainel::Link(i) => {
            fechar_painel(st);
            abrir_link(i);
            return;
        }
    }
    ajustar_painel(st);
    redesenhar(st);
}

unsafe fn proc_painel(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut ps);
            let mut r = RECT::default();
            _ = GetClientRect(h, &mut r);
            _ = com(|st| {
                let c = CenaPainel { w: r.right, escala: st.escala, linhas: st.painel.linhas(st.audio.as_ref()), hover: st.painel.hover };
                if let Ok(a) = st.gfx.desenhar_painel(dc, r.bottom, &c) {
                    st.painel.alvos = a;
                }
            });
            _ = EndPaint(h, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            _ = com(|st| {
                if let Some(a) = st.painel.arrasto {
                    if let Some(n) = st.painel.nivel_em(a, x as f32) {
                        st.painel.definir_nivel(st.audio.as_ref(), a, n);
                        invalidar(h);
                        redesenhar(st);
                    }
                    return;
                }
                let a = alvo_painel_em(&st.painel.alvos, x, y);
                if a != st.painel.hover {
                    st.painel.hover = a;
                    invalidar(h);
                }
            });
            let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
            _ = TrackMouseEvent(&mut t);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            _ = com(|st| {
                if st.painel.arrasto.is_none() {
                    st.painel.hover = None;
                    invalidar(h);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = xy(lp);
            _ = com(|st| {
                if let Some(a) = alvo_painel_em(&st.painel.alvos, x, y) {
                    clicar_painel(st, a, x);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            _ = ReleaseCapture();
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            _ = com(|st| st.painel.arrasto = None);
            invalidar(h);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let mut pt = POINT { x: xy(lp).0, y: xy(lp).1 };
            _ = ScreenToClient(h, &mut pt);
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16;
            _ = com(|st| {
                let alvo = match alvo_painel_em(&st.painel.alvos, pt.x, pt.y) {
                    Some(a @ AlvoPainel::Programa(_)) => a,
                    _ => AlvoPainel::Nivel,
                };
                st.painel.somar(st.audio.as_ref(), alvo, if delta > 0 { 0.02 } else { -0.02 });
                invalidar(h);
                redesenhar(st);
            });
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            _ = com(fechar_painel);
            LRESULT(0)
        }
        WM_CLIQUE_FORA => {
            // no ícone de rede/volume quem decide é o clique na barra (fecha ou troca de painel)
            let (x, y) = xy(lp);
            _ = com(|st| {
                let mut b = RECT::default();
                _ = GetWindowRect(st.barra, &mut b);
                let no_icone = matches!(alvo_em(&st.alvos, x - b.left, y - b.top), Some(Alvo::Rede | Alvo::Volume));
                if !(no_icone && PtInRect(&b, POINT { x, y }).as_bool()) {
                    fechar_painel(st);
                }
            });
            LRESULT(0)
        }
        WM_BT_LIDO | WM_BT_FEITO => {
            let s = *Box::from_raw(lp.0 as *mut bluetooth::Situacao);
            _ = com(|st| {
                if msg == WM_BT_LIDO {
                    st.painel.lendo = false;
                } else {
                    st.painel.pedido = None;
                }
                st.painel.bt = Some(s);
                ajustar_painel(st);
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

// ---------- central ----------

/// Pede ao serviço uma sincronização da agenda, se a última tem mais de um minuto.
fn pedir_sincronizacao(st: &Estado) {
    let agora = chrono::Local::now().naive_local();
    let velha = st.central.dados.fontes.iter().find(|f| f.nome == "agenda").and_then(|f| f.atualizado).is_none_or(|a| agora - a > chrono::Duration::minutes(1));
    if !velha {
        return;
    }
    use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
    unsafe {
        if let Ok(e) = OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\barra-central-sincronizar")) {
            _ = SetEvent(e);
            _ = CloseHandle(e);
        }
    }
}

fn abrir_central() {
    let Some(Some(h)) = com(|st| {
        if st.central.aberta {
            return None;
        }
        fechar_painel(st);
        st.central.dados.recarregar();
        pedir_sincronizacao(st);
        let hoje = chrono::Local::now().date_naive();
        st.central.aberta = true;
        st.central.vista = central::Vista::Dia;
        st.central.ir_dia(hoje);
        st.central.filtro.clear();
        st.central.rolagem = 0.0;
        st.central.hover = None;
        st.central.anterior = primeiro_plano();
        let s = st.escala;
        let m = appbar::monitor();
        let mut b = RECT::default();
        unsafe { _ = GetWindowRect(st.barra, &mut b) };
        let w = ((1560.0 * s) as i32).min(m.right - m.left - (24.0 * s) as i32);
        let h = ((800.0 * s) as i32).min(m.bottom - b.bottom - (16.0 * s) as i32);
        // centrada embaixo do relógio
        let centro = st.alvos.iter().find(|(_, a)| *a == Alvo::Relogio).map(|(r, _)| b.left + ((r.left + r.right) / 2.0) as i32).unwrap_or((m.left + m.right) / 2);
        let x = (centro - w / 2).clamp(m.left, m.right - w);
        unsafe {
            _ = SetWindowPos(st.central.hwnd, Some(HWND_TOPMOST), x, b.bottom, w, h, SWP_SHOWWINDOW);
            _ = ShowWindow(st.dica, SW_HIDE);
        }
        st.dica_texto.clear();
        redesenhar(st);
        Some(st.central.hwnd)
    }) else {
        return;
    };
    teclado::CENTRAL_ABERTA.store(true, Ordering::Relaxed);
    unsafe { _ = UpdateWindow(h) };
    forcar_primeiro_plano(h);
}

/// Fecha a central; `voltar` devolve o foco para a janela de antes (Esc).
fn fechar_central(voltar: bool) {
    teclado::CENTRAL_ABERTA.store(false, Ordering::Relaxed);
    let Some(Some(anterior)) = com(|st| {
        if !st.central.aberta {
            return None;
        }
        st.central.aberta = false;
        st.central.hover = None;
        unsafe { _ = ShowWindow(st.central.hwnd, SW_HIDE) };
        redesenhar(st);
        Some(st.central.anterior)
    }) else {
        return;
    };
    if voltar && anterior != 0 {
        forcar_primeiro_plano(hwnd(anterior));
    }
}

fn alternar_central() {
    if com(|st| st.central.aberta) == Some(true) {
        fechar_central(true);
    } else {
        abrir_central();
    }
}

fn abrir_endereco(u: &str) {
    let w: Vec<u16> = u.encode_utf16().chain([0]).collect();
    sistema::abrir_uri(PCWSTR(w.as_ptr()));
}

/// Clique na central. Link e comando fecham a central antes (o foco vai para outro programa).
fn clicar_central(a: central::AlvoCentral) {
    use central::{AlvoCentral as A, Sel};
    enum Depois {
        Nada,
        Abrir(String),
        Executar(central::Executar),
    }
    let depois = com(|st| {
        let c = &mut st.central;
        let d = &c.dados;
        let r = match a {
            A::Anterior => {
                c.passo(-1);
                Depois::Nada
            }
            A::Seguinte => {
                c.passo(1);
                Depois::Nada
            }
            A::Vista(v) => {
                c.vista = v;
                c.sel = None;
                Depois::Nada
            }
            A::Fonte(i) => {
                c.alternar_fonte(i);
                Depois::Nada
            }
            A::Evento(i) => {
                c.sel = if c.sel == Some(Sel::Evento(i)) { None } else { Some(Sel::Evento(i)) };
                Depois::Nada
            }
            A::Execucao(i) => {
                c.sel = if c.sel == Some(Sel::Execucao(i)) { None } else { Some(Sel::Execucao(i)) };
                Depois::Nada
            }
            A::Acao(id) => match c.sel {
                Some(Sel::Evento(i)) => d.eventos.get(i).map(|e| Depois::Abrir(if id == 1 { e.reuniao.clone() } else { e.link.clone() })).unwrap_or(Depois::Nada),
                Some(Sel::Execucao(i)) => match d.execucoes.get(i) {
                    Some(e) if id == 0 => Depois::Abrir(e.registro.clone()),
                    Some(e) => e.executar.clone().or_else(|| d.fontes.get(e.fonte).and_then(|f| f.executar.clone())).map(Depois::Executar).unwrap_or(Depois::Nada),
                    None => Depois::Nada,
                },
                None => Depois::Nada,
            },
            A::FecharDetalhe => Depois::Nada,
            A::Pendencia(i) => {
                c.dados.alternar(i);
                Depois::Nada
            }
            A::LinkPendencia(i) => d.pendencias.get(i).map(|p| Depois::Abrir(p.link.clone())).unwrap_or(Depois::Nada),
            A::ExecutarPendencia(i) => d.pendencias.get(i).and_then(|p| p.executar.clone()).map(Depois::Executar).unwrap_or(Depois::Nada),
            A::ExecutarFonte(i) => d.fontes.get(i).and_then(|f| f.executar.clone()).map(Depois::Executar).unwrap_or(Depois::Nada),
            A::Prazo(i) => d.prazos.get(i).map(|p| Depois::Abrir(p.link.clone())).unwrap_or(Depois::Nada),
            A::Rotina(i) => d.rotinas.get(i).and_then(|r| r.executar.clone()).map(Depois::Executar).unwrap_or(Depois::Nada),
            A::MesAnterior | A::MesSeguinte => {
                let m = c.mes;
                c.mes = if a == A::MesAnterior { m - chrono::Months::new(1) } else { m + chrono::Months::new(1) };
                Depois::Nada
            }
            A::Dia(dd) => {
                c.vista = central::Vista::Dia;
                c.ir_dia(dd);
                Depois::Nada
            }
        };
        invalidar(c.hwnd);
        redesenhar(st);
        r
    })
    .unwrap_or(Depois::Nada);
    match depois {
        Depois::Abrir(u) if !u.is_empty() => {
            fechar_central(false);
            abrir_endereco(&u);
        }
        Depois::Executar(e) => {
            fechar_central(false);
            central::executar(&e);
        }
        _ => {}
    }
}

unsafe fn proc_central(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut ps);
            let mut r = RECT::default();
            _ = GetClientRect(h, &mut r);
            _ = com(|st| {
                let cena = desenho::CenaCentral { w: r.right, h: r.bottom, escala: st.escala, c: &st.central, agora: chrono::Local::now().naive_local() };
                if let Ok(a) = st.gfx.desenhar_central(dc, &cena) {
                    st.central.alvos = a;
                }
            });
            _ = EndPaint(h, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            _ = com(|st| {
                let a = st.central.alvo_em(x as f32, y as f32);
                if a != st.central.hover {
                    st.central.hover = a;
                    invalidar(h);
                }
            });
            let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
            _ = TrackMouseEvent(&mut t);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            _ = com(|st| {
                st.central.hover = None;
                invalidar(h);
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = xy(lp);
            if let Some(Some(a)) = com(|st| st.central.alvo_em(x as f32, y as f32)) {
                clicar_central(a);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16;
            // sobre as pendências rola a lista; no resto troca o dia
            let mut pt = POINT { x: xy(lp).0, y: xy(lp).1 };
            _ = ScreenToClient(h, &mut pt);
            _ = com(|st| {
                if !st.central.rolar(pt.x as f32, pt.y as f32, delta as f32 * 0.5) {
                    st.central.passo(if delta > 0 { -1 } else { 1 });
                }
                st.central.hover = st.central.alvo_em(pt.x as f32, pt.y as f32);
                invalidar(h);
            });
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = VIRTUAL_KEY(wp.0 as u16);
            if vk == VK_ESCAPE {
                let limpou = com(|st| {
                    let tinha = !st.central.filtro.is_empty();
                    st.central.filtro.clear();
                    tinha
                });
                if limpou == Some(true) {
                    invalidar(h);
                } else {
                    fechar_central(true);
                }
                return LRESULT(0);
            }
            _ = com(|st| {
                let c = &mut st.central;
                match vk {
                    VK_LEFT => c.passo(-1),
                    VK_RIGHT => c.passo(1),
                    VK_TAB => {
                        c.vista = if c.vista == central::Vista::Dia { central::Vista::Semana } else { central::Vista::Dia };
                        c.sel = None;
                    }
                    VK_HOME => {
                        c.vista = central::Vista::Dia;
                        c.ir_dia(chrono::Local::now().date_naive());
                    }
                    VK_BACK => _ = c.filtro.pop(),
                    _ => return,
                }
                invalidar(h);
            });
            LRESULT(0)
        }
        WM_CHAR => {
            let Some(ch) = char::from_u32(wp.0 as u32).filter(|ch| !ch.is_control() && (*ch as u32) < 0xE000) else {
                return LRESULT(0);
            };
            _ = com(|st| {
                let c = &mut st.central;
                if c.filtro.is_empty() {
                    if ch.is_whitespace() {
                        return;
                    }
                    // sem filtro, 1–9 liga e desliga a fonte
                    if let Some(n) = ch.to_digit(10).filter(|n| *n >= 1) {
                        c.alternar_fonte(n as usize - 1);
                        invalidar(h);
                        return;
                    }
                }
                c.filtro.push(ch);
                invalidar(h);
            });
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                fechar_central(false);
            }
            DefWindowProcW(h, msg, wp, lp)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

/// Abre o serviço da central (agenda e hooks do Claude). Roda de uma cópia em
/// `%LOCALAPPDATA%\barra\bin`: assim o `cargo build` troca o executável sem esbarrar no
/// serviço em execução, e os hooks do Claude apontam sempre para o mesmo caminho.
fn iniciar_servico() {
    std::thread::spawn(|| {
        use std::os::windows::process::CommandExt;
        const SEM_JANELA: u32 = 0x0800_0000;
        let Some(base) = central::pasta().parent().map(|p| p.to_path_buf()) else { return };
        let instalado = base.join("bin").join("barra-central.exe");
        let origem = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("barra-central.exe"))).filter(|o| o.exists());
        if let Some(o) = origem {
            let marca = |p: &std::path::Path| std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok()));
            if marca(&o) != marca(&instalado) {
                if instalado.exists() {
                    _ = std::process::Command::new(&instalado).arg("sair").creation_flags(SEM_JANELA).status();
                }
                _ = std::fs::create_dir_all(base.join("bin"));
                for _ in 0..30 {
                    if std::fs::copy(&o, &instalado).is_ok() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        }
        if instalado.exists() {
            _ = std::process::Command::new(&instalado).creation_flags(SEM_JANELA).spawn();
        }
    });
}

// ---------- visão geral ----------

fn remontar_visao(st: &mut Estado) {
    let m = appbar::monitor();
    let mut c = RECT::default();
    unsafe { _ = GetClientRect(st.visao_hwnd, &mut c) };
    if st.visao.lista {
        let mru = st.mru.clone();
        st.visao.montar_lista(&st.grupos, &mru, m, c.right as f32, c.bottom as f32, st.escala);
    } else {
        st.visao.montar(&st.grupos, m, c.right as f32, c.bottom as f32, st.escala);
    }
    invalidar(st.visao_hwnd);
}

fn abrir_visao(modo_alt: bool, selecao: Option<isize>) {
    let Some(Some(v)) = com(|st| {
        if st.visao.aberta {
            return None;
        }
        st.visao.anterior = primeiro_plano();
        st.visao.aberta = true;
        st.visao.filtro.clear();
        st.visao.lista = modo_alt;
        st.visao.cartao_sel = None;
        st.visao.arrasto = None;
        st.visao.selecao = selecao.or_else(|| st.mru.first().copied());
        let m = appbar::monitor();
        unsafe {
            _ = SetWindowPos(st.visao_hwnd, Some(HWND_TOPMOST), m.left, m.top, m.right - m.left, m.bottom - m.top, SWP_SHOWWINDOW);
        }
        remontar_visao(st);
        unsafe { _ = ShowWindow(st.dica, SW_HIDE) };
        Some(st.visao_hwnd)
    }) else {
        return;
    };
    MODO_ALT.store(modo_alt, Ordering::Relaxed);
    teclado::VISAO_ABERTA.store(true, Ordering::Relaxed);
    // primeiro quadro já, sem esperar a mensagem de pintura passar pela fila
    unsafe { _ = UpdateWindow(v) };
    forcar_primeiro_plano(v);
}

/// Fecha a visão geral; `voltar` devolve o foco para a janela de antes.
fn fechar_visao(voltar: bool) {
    teclado::VISAO_ABERTA.store(false, Ordering::Relaxed);
    MODO_ALT.store(false, Ordering::Relaxed);
    let Some(Some(anterior)) = com(|st| {
        if !st.visao.aberta {
            return None;
        }
        st.visao.aberta = false;
        st.visao.tirar_miniaturas();
        st.visao.arrasto = None;
        if let Some((_, e)) = st.visao.editando.take() {
            unsafe { _ = DestroyWindow(e) };
        }
        unsafe { _ = ShowWindow(st.visao_hwnd, SW_HIDE) };
        Some(st.visao.anterior)
    }) else {
        return;
    };
    if voltar && anterior != 0 {
        forcar_primeiro_plano(hwnd(anterior));
    }
}

fn escolher_selecao() {
    let sel = com(|st| (st.visao.selecao, st.visao.cartao_sel.and_then(|c| st.visao.cartoes.get(c)).map(|c| c.grupo)));
    match sel {
        // quadro de grupo vazio: vai para ele
        Some((_, Some(Some(g)))) => {
            fechar_visao(false);
            ir(g, None);
        }
        // "novo grupo": cria e continua na visão geral
        Some((_, Some(None))) => _ = com(novo_grupo_na_visao),
        Some((Some(h), None)) => {
            fechar_visao(false);
            ir_janela(h);
        }
        _ => {}
    }
}

/// Cria um grupo vazio, refaz a visão geral e deixa o quadro dele escolhido.
fn novo_grupo_na_visao(st: &mut Estado) {
    let g = st.grupos.novo();
    remontar_visao(st);
    st.visao.selecao = None;
    st.visao.cartao_sel = st.visao.cartoes.iter().position(|c| c.grupo == Some(g));
    redesenhar(st);
}

/// Apaga o grupo da seleção (janela ou quadro); as janelas dele vão para o grupo anterior.
fn apagar_grupo_escolhido(st: &mut Estado) {
    let porjanela = st.visao.selecao.and_then(|j| st.grupos.grupo_de(j));
    let g = porjanela.or_else(|| st.visao.cartao_sel.and_then(|c| st.visao.cartoes.get(c)).and_then(|c| c.grupo));
    if let Some(g) = g {
        st.grupos.apagar(g);
        st.visao.cartao_sel = None;
        remontar_visao(st);
        redesenhar(st);
    }
}

/// Quadro (índice em `cartoes`) da seleção atual: o próprio quadro escolhido, ou o do grupo
/// da janela escolhida.
fn cartao_da_selecao(st: &Estado) -> Option<usize> {
    let v = &st.visao;
    v.cartao_sel.or_else(|| {
        let g = v.selecao.and_then(|j| st.grupos.grupo_de(j))?;
        v.cartoes.iter().position(|c| c.grupo == Some(g))
    })
}

/// Setas no Win+Tab: pulam de grupo em grupo (um toque por grupo), para o quadro mais perto
/// naquela direção, incluindo o "novo grupo".
fn navegar(st: &mut Estado, dx: f32, dy: f32) {
    let v = &st.visao;
    let centro = |r: &windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F| ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
    let atual = cartao_da_selecao(st);
    let escolha = match atual {
        None => v.cartoes.iter().position(|c| c.grupo == Some(st.grupos.ativo)),
        Some(a) => {
            let (ax, ay) = centro(&v.cartoes[a].r);
            v.cartoes
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != a)
                .filter_map(|(i, c)| {
                    let (px, py) = centro(&c.r);
                    let principal = (px - ax) * dx + (py - ay) * dy;
                    if principal <= 1.0 {
                        return None;
                    }
                    let lado = if dx != 0.0 { (py - ay).abs() } else { (px - ax).abs() };
                    Some((principal + 2.0 * lado, i))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|p| p.1)
        }
    };
    if let Some(c) = escolha {
        st.visao.cartao_sel = Some(c);
        st.visao.selecao = None;
        invalidar(st.visao_hwnd);
    }
}

/// Tab no Win+Tab: escolhe a janela seguinte dentro do grupo selecionado.
fn andar_no_grupo(st: &mut Estado, passo: i32) {
    let Some(c) = cartao_da_selecao(st) else { return };
    let Some(g) = st.visao.cartoes[c].grupo else { return };
    let js: Vec<isize> = st.visao.ordem_espacial().into_iter().filter(|&j| st.grupos.grupo_de(j) == Some(g)).collect();
    if js.is_empty() {
        return;
    }
    let n = js.len() as i32;
    let i = st.visao.selecao.and_then(|s| js.iter().position(|&j| j == s)).map(|i| i as i32);
    let novo = match i {
        Some(i) => (i + passo).rem_euclid(n),
        None if passo > 0 => 0,
        None => n - 1,
    };
    st.visao.selecao = Some(js[novo as usize]);
    st.visao.cartao_sel = None;
    invalidar(st.visao_hwnd);
}

/// Shift+setas: leva a janela escolhida para o grupo ao lado (depois do último, um novo).
fn mover_para_vizinho(st: &mut Estado, passo: i32) {
    let Some(h) = st.visao.selecao else { return };
    let Some(g) = st.grupos.grupo_de(h) else { return };
    let destino = g as i32 + passo;
    if destino < 0 {
        return;
    }
    let destino = if destino as usize >= st.grupos.lista.len() { st.grupos.novo() } else { destino as usize };
    st.grupos.mover(h, destino);
    remontar_visao(st);
    redesenhar(st);
}


/// Anda `passo` na lista (MRU ou espacial) a partir da seleção.
fn andar(st: &mut Estado, espacial: bool, passo: i32) {
    let lista: Vec<isize> = if espacial {
        st.visao.ordem_espacial()
    } else {
        st.mru.iter().copied().filter(|&h| st.visao.casa(h)).collect()
    };
    if lista.is_empty() {
        return;
    }
    let n = lista.len() as i32;
    let i = st.visao.selecao.and_then(|s| lista.iter().position(|&h| h == s)).map(|i| i as i32).unwrap_or(-1);
    let novo = if i < 0 { if passo > 0 { 0 } else { n - 1 } } else { (i + passo).rem_euclid(n) };
    st.visao.selecao = Some(lista[novo as usize]);
    st.visao.cartao_sel = None;
    invalidar(st.visao_hwnd);
}

fn cena_visao(st: &mut Estado, w: i32, h: i32) -> CenaVisao {
    let ativo = st.grupos.ativo;
    let destino = st.visao.arrasto.as_ref().filter(|a| a.ativo).and_then(|_| st.visao.hover_cartao);
    let cartoes = st
        .visao
        .cartoes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (nome, qtd) = match c.grupo {
                Some(g) => (st.grupos.lista[g].nome.clone(), st.grupos.lista[g].janelas.len()),
                None => (String::new(), 0),
            };
            CartaoV {
                r: c.r,
                cab: c.cab,
                area: c.area,
                nome,
                qtd,
                ativo: c.grupo == Some(ativo),
                novo: c.grupo.is_none(),
                hover: st.visao.hover_cartao == Some(i),
                destino: destino == Some(i),
                selecionado: st.visao.cartao_sel == Some(i),
            }
        })
        .collect();
    let itens: Vec<_> = st.visao.itens.iter().enumerate().map(|(i, it)| (it.hwnd, it.r, it.minimizada, st.visao.hover == Some(i), it.rotulo, it.grupo)).collect();
    let mut janelas_v = Vec::new();
    for (j, r, minimizada, hover, rotulo, g) in itens {
        janelas_v.push(JanelaV {
            r,
            titulo: janelas::titulo(hwnd(j)),
            icone: icone_de(st, j),
            minimizada,
            selecionada: st.visao.selecao == Some(j),
            hover,
            apagada: !st.visao.casa(j),
            rotulo,
            grupo: st.grupos.lista.get(g).map(|x| x.nome.clone()).unwrap_or_default(),
        });
    }
    let titulo = st.visao.selecao.map(|s| janelas::titulo(hwnd(s))).unwrap_or_default();
    CenaVisao { w, h, escala: st.escala, cartoes, janelas: janelas_v, filtro: st.visao.filtro.clone(), titulo, lista: st.visao.lista }
}

fn comecar_renomear(g: usize) {
    _ = com(|st| unsafe {
        if g >= st.grupos.lista.len() {
            return;
        }
        let Some(c) = st.visao.cartoes.iter().find(|c| c.grupo == Some(g)) else { return };
        let s = st.escala;
        let (x, y) = ((c.cab.left + 10.0 * s) as i32, (c.cab.top + 7.0 * s) as i32);
        let (w, h) = (((c.cab.right - c.cab.left) - 140.0 * s).max(120.0 * s) as i32, (c.cab.bottom - c.cab.top - 14.0 * s) as i32);
        let nome: Vec<u16> = st.grupos.lista[g].nome.encode_utf16().chain([0]).collect();
        let Ok(e) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            PCWSTR(nome.as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            x, y, w, h,
            Some(st.visao_hwnd), None, None, None,
        ) else {
            return;
        };
        // mesma letra do nome do grupo no cabeçalho do cartão
        let fonte = CreateFontW(
            -(12.0 * s) as i32, 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0,
            DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0,
            w!("CaskaydiaMono NF"),
        );
        SendMessageW(e, WM_SETFONT, Some(WPARAM(fonte.0 as usize)), Some(LPARAM(1)));
        _ = SetWindowSubclass(e, Some(proc_edicao), 1, 0);
        SendMessageW(e, 0x00B1 /* EM_SETSEL */, Some(WPARAM(0)), Some(LPARAM(-1)));
        _ = SetFocus(Some(e));
        st.visao.editando = Some((g, e));
    });
}

unsafe extern "system" fn proc_edicao(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    let visao = HWND(VISAO.load(Ordering::Relaxed) as *mut _);
    match msg {
        WM_KEYDOWN if wp.0 == VK_RETURN.0 as usize => {
            _ = PostMessageW(Some(visao), WM_RENOMEAR, WPARAM(1), LPARAM(0));
            LRESULT(0)
        }
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            _ = PostMessageW(Some(visao), WM_RENOMEAR, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        // sem o "bip" do Enter/Esc numa caixa de uma linha
        WM_CHAR if wp.0 == 13 || wp.0 == 27 => LRESULT(0),
        WM_KILLFOCUS => {
            _ = PostMessageW(Some(visao), WM_RENOMEAR, WPARAM(1), LPARAM(0));
            DefSubclassProc(h, msg, wp, lp)
        }
        _ => DefSubclassProc(h, msg, wp, lp),
    }
}

// ---------- menus ----------

/// Item de menu: (id, texto, marcado, habilitado). id 0 = separador.
type Item = (usize, String, bool, bool);

/// Texto de um item do menu aberto. Os itens são desenhados pela barra (no tema) e só
/// guardam o índice nesta lista.
struct TextoMenu {
    texto: String,
    atalho: String,
    separador: bool,
    submenu: bool,
}

thread_local! {
    static TEXTOS_MENU: RefCell<Vec<TextoMenu>> = const { RefCell::new(Vec::new()) };
}

/// Borda do menu aberto (COLORREF), para o gancho que pega a janela do menu.
static BORDA_MENU: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Fundo do menu aberto (COLORREF), para a moldura.
static FUNDO_MENU: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Põe um item desenhado pela barra na posição `pos` (None = no fim). id 0 = separador;
/// `sub` faz do item a entrada de um submenu. O texto pode ter o atalho depois de um Tab.
fn anexar(m: HMENU, pos: Option<u32>, id: usize, texto: &str, marcado: bool, habilitado: bool, sub: Option<HMENU>) {
    let (texto, atalho) = texto.split_once('\t').unwrap_or((texto, ""));
    let indice = TEXTOS_MENU.with_borrow_mut(|v| {
        v.push(TextoMenu { texto: texto.into(), atalho: atalho.into(), separador: id == 0, submenu: sub.is_some() });
        v.len() - 1
    });
    let mut tipo = MFT_OWNERDRAW;
    if id == 0 {
        tipo |= MFT_SEPARATOR;
    }
    let mut estado = MENU_ITEM_STATE(0);
    if marcado {
        estado |= MFS_CHECKED;
    }
    if !habilitado {
        estado |= MFS_DISABLED;
    }
    let mii = MENUITEMINFOW {
        cbSize: size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_DATA | MIIM_SUBMENU,
        fType: tipo,
        fState: estado,
        wID: id as u32,
        hSubMenu: sub.unwrap_or_default(),
        dwItemData: indice,
        ..Default::default()
    };
    unsafe {
        let pos = pos.unwrap_or(GetMenuItemCount(Some(m)).max(0) as u32);
        _ = InsertMenuItemW(m, pos, true, &mii);
    }
}

fn montar_menu(itens: &[Item]) -> Option<HMENU> {
    let m = unsafe { CreatePopupMenu().ok()? };
    for (id, texto, marcado, habilitado) in itens {
        anexar(m, None, *id, texto, *marcado, *habilitado, None);
    }
    Some(m)
}

/// A janela do menu acabou de aparecer: cantos retos e borda de destaque, como a dica.
unsafe extern "system" fn menu_aberto(_: HWINEVENTHOOK, _: u32, h: HWND, _: i32, _: i32, _: u32, _: u32) {
    vestir_menu(h);
}

/// Cantos retos, sem sombra e moldura do tema na janela do menu. Chamado pelo evento de
/// menu aberto e pelo WM_ENTERIDLE (o evento não chega para o primeiro menu); age uma vez
/// por janela de menu e tema, já que o Windows reaproveita a janela.
unsafe fn vestir_menu(h: HWND) {
    use windows::Win32::Graphics::Dwm::*;
    if h.is_invalid() || crate::janelas::classe(h) != "#32768" {
        return;
    }
    // marca na janela a borda já aplicada: o tema pode ter mudado desde o último menu
    let borda = BORDA_MENU.load(Ordering::Relaxed);
    let marca = borda as usize + 1;
    if GetPropW(h, w!("barra-menu")).0 as usize == marca {
        return;
    }
    let primeira = GetPropW(h, w!("barra-menu")).0.is_null();
    _ = SetPropW(h, w!("barra-menu"), Some(HANDLE(marca as *mut _)));
    let canto = DWMWCP_DONOTROUND.0 as u32;
    _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &canto as *const _ as *const _, 4);
    _ = DwmSetWindowAttribute(h, DWMWA_BORDER_COLOR, &borda as *const _ as *const _, 4);
    // sem sombra: vale para os próximos menus da barra
    let estilo = GetClassLongPtrW(h, GCL_STYLE);
    if estilo & CS_DROPSHADOW.0 as usize != 0 {
        SetClassLongPtrW(h, GCL_STYLE, (estilo & !(CS_DROPSHADOW.0 as usize)) as isize);
    }
    // a moldura cinza do Windows vira a borda de 2 px do tema
    if primeira {
        _ = SetWindowSubclass(h, Some(proc_moldura_menu), 2, 0);
    }
    // a janela do primeiro menu já está na tela: refaz a moldura e a borda do DWM
    _ = SetWindowPos(h, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    _ = RedrawWindow(Some(h), None, None, RDW_FRAME | RDW_INVALIDATE | RDW_UPDATENOW);
}

/// Moldura (área não cliente) da janela do menu: `fundo` com borda de 2 px em `destaque`.
unsafe extern "system" fn proc_moldura_menu(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    match msg {
        WM_NCPAINT => {
            let dc = GetWindowDC(Some(h));
            let mut j = RECT::default();
            _ = GetWindowRect(h, &mut j);
            let mut c = RECT::default();
            _ = GetClientRect(h, &mut c);
            let mut origem = POINT::default();
            _ = ClientToScreen(h, &mut origem);
            // cliente em coordenadas da janela, fora do recorte
            let (cx, cy) = (origem.x - j.left, origem.y - j.top);
            ExcludeClipRect(dc, cx, cy, cx + c.right, cy + c.bottom);
            let todo = RECT { left: 0, top: 0, right: j.right - j.left, bottom: j.bottom - j.top };
            let fundo = CreateSolidBrush(COLORREF(FUNDO_MENU.load(Ordering::Relaxed)));
            let borda = CreateSolidBrush(COLORREF(BORDA_MENU.load(Ordering::Relaxed)));
            FillRect(dc, &todo, borda);
            let e = 2;
            FillRect(dc, &RECT { left: e, top: e, right: todo.right - e, bottom: todo.bottom - e }, fundo);
            _ = DeleteObject(fundo.into());
            _ = DeleteObject(borda.into());
            ReleaseDC(Some(h), dc);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            _ = RemovePropW(h, w!("barra-menu"));
            DefSubclassProc(h, msg, wp, lp)
        }
        _ => DefSubclassProc(h, msg, wp, lp),
    }
}

/// Mostra o menu no ponto (embaixo: acima da barra) e devolve o id escolhido (0 = nada).
fn mostrar_menu(dono: HWND, m: HMENU, pt: POINT, acima: bool) -> usize {
    let (fundo, destaque) = com(|st| (st.gfx.tema.fundo, st.gfx.tema.destaque)).unwrap_or((0x1a1b26, 0x7aa2f7));
    let colorref = |c: u32| (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16) & 0xFF;
    BORDA_MENU.store(colorref(destaque), Ordering::Relaxed);
    FUNDO_MENU.store(colorref(fundo), Ordering::Relaxed);
    unsafe {
        // fundo do tema também nas margens do menu e dos submenus
        let pincel = CreateSolidBrush(COLORREF(colorref(fundo)));
        let info = MENUINFO { cbSize: size_of::<MENUINFO>() as u32, fMask: MIM_BACKGROUND | MIM_APPLYTOSUBMENUS, hbrBack: pincel, ..Default::default() };
        _ = SetMenuInfo(m, &info);
        let gancho = SetWinEventHook(EVENT_SYSTEM_MENUPOPUPSTART, EVENT_SYSTEM_MENUPOPUPSTART, None, Some(menu_aberto), windows::Win32::System::Threading::GetCurrentProcessId(), 0, WINEVENT_OUTOFCONTEXT);
        _ = SetForegroundWindow(dono); // sem isso o menu não fecha ao clicar fora
        // sem a animação de abertura: a janela do menu animada é em camadas e ignora a moldura
        let mut f = TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NOANIMATION;
        if acima {
            f |= TPM_BOTTOMALIGN;
        }
        let id = TrackPopupMenu(m, f, pt.x, pt.y, None, dono, None);
        _ = PostMessageW(Some(dono), WM_NULL, WPARAM(0), LPARAM(0));
        _ = windows::Win32::UI::Accessibility::UnhookWinEvent(gancho);
        _ = DestroyMenu(m);
        _ = DeleteObject(pincel.into());
        TEXTOS_MENU.with_borrow_mut(|v| v.clear());
        id.0 as usize
    }
}

/// WM_MEASUREITEM de um item de menu da barra.
fn medir_item_menu(mi: &mut MEASUREITEMSTRUCT) {
    let (w, h) = TEXTOS_MENU.with_borrow(|v| {
        let Some(t) = v.get(mi.itemData) else { return (0, 0) };
        let it = desenho::ItemMenu { texto: &t.texto, atalho: &t.atalho, separador: t.separador, submenu: t.submenu, marcado: false, habilitado: true, selecionado: false };
        com(|st| st.gfx.medir_menu(&it, st.escala)).unwrap_or((200, 26))
    });
    mi.itemWidth = w;
    mi.itemHeight = h;
}

/// WM_DRAWITEM de um item de menu da barra.
fn desenhar_item_menu(di: &DRAWITEMSTRUCT) {
    TEXTOS_MENU.with_borrow(|v| {
        let Some(t) = v.get(di.itemData) else { return };
        let e = di.itemState.0;
        let it = desenho::ItemMenu {
            texto: &t.texto,
            atalho: &t.atalho,
            separador: t.separador,
            submenu: t.submenu,
            marcado: e & ODS_CHECKED.0 != 0,
            habilitado: e & (ODS_DISABLED.0 | ODS_GRAYED.0) == 0,
            selecionado: e & ODS_SELECTED.0 != 0,
        };
        _ = com(|st| st.gfx.desenhar_menu(di.hDC, di.rcItem, &it, st.escala));
    });
    // a seta de submenu do Windows é desenhada depois deste item: fica de fora
    let r = di.rcItem;
    unsafe { ExcludeClipRect(di.hDC, r.left, r.top, r.right, r.bottom) };
}

fn ponto_da_barra(barra: HWND) -> POINT {
    let mut r = RECT::default();
    unsafe { _ = GetWindowRect(barra, &mut r) };
    POINT { x: cursor().x, y: r.bottom }
}

const MENU_NOVO_GRUPO_COM: usize = 2;
const MENU_FECHAR: usize = 3;
const MENU_APAGAR_GRUPO: usize = 10;
const MENU_NOVO_GRUPO: usize = 11;
const MENU_RENOMEAR: usize = 12;
const MENU_ESCONDER: usize = 20;
const MENU_INICIAR: usize = 21;
const MENU_JUNTAR: usize = 22;
const MENU_SAIR: usize = 23;
const MENU_ALT_TAB: usize = 24;
const MENU_VISAO: usize = 25;
const MENU_MOLDURAS: usize = 26;
const MENU_CORES: usize = 27;
const MENU_MOVER: usize = 100;
/// Entrada de submenu (não é escolhida; só abre o submenu).
const MENU_SUBMENU: usize = 1;

fn menu_grupo(dono: HWND, i: usize, pt: POINT, acima: bool) {
    let Some(nomes) = com(|st| st.grupos.lista.iter().map(|g| g.nome.clone()).collect::<Vec<_>>()) else { return };
    if i >= nomes.len() {
        return;
    }
    let itens = vec![
        (MENU_RENOMEAR, "Renomear grupo".to_string(), false, true),
        (MENU_NOVO_GRUPO, "Criar grupo".to_string(), false, true),
        (0, String::new(), false, true),
        (MENU_APAGAR_GRUPO, format!("Excluir o grupo {} (as janelas passam para o grupo anterior)", nomes[i]), false, nomes.len() > 1),
    ];
    let Some(m) = montar_menu(&itens) else { return };
    match mostrar_menu(dono, m, pt, acima) {
        MENU_RENOMEAR => {
            abrir_visao(false, None);
            comecar_renomear(i);
        }
        MENU_NOVO_GRUPO => _ = com(|st| st.grupos.novo()),
        MENU_APAGAR_GRUPO => _ = com(|st| st.grupos.apagar(i)),
        _ => {}
    }
    _ = com(|st| {
        if st.visao.aberta && st.visao.editando.is_none() {
            remontar_visao(st);
        }
        redesenhar(st);
    });
}

fn clicar_direito(alvo: Option<Alvo>) {
    let Some((dono, barra, nomes, grupo_da_janela, esconder)) = com(|st| {
        let nomes: Vec<String> = st.grupos.lista.iter().map(|g| g.nome.clone()).collect();
        let gj = match alvo {
            Some(Alvo::Janela(h)) => st.grupos.grupo_de(h),
            _ => None,
        };
        (st.menus, st.barra, nomes, gj, st.esconder_windows)
    }) else {
        return;
    };
    let pt = ponto_da_barra(barra);

    match alvo {
        Some(Alvo::Bandeja(i)) => bandeja::clicar(i, bandeja::Botao::Direito, cursor()),
        Some(Alvo::Grupo(i)) => menu_grupo(dono, i, pt, false),
        Some(Alvo::Janela(h)) => unsafe {
            let Ok(m) = CreatePopupMenu() else { return };
            let outros: Vec<Item> = nomes
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != grupo_da_janela)
                .map(|(i, n)| (MENU_MOVER + i, format!("Grupo {n}"), false, true))
                .collect();
            if let Some(sub) = montar_menu(&outros).filter(|_| !outros.is_empty()) {
                anexar(m, None, MENU_SUBMENU, "Mover para o grupo", false, true, Some(sub));
            }
            anexar(m, None, MENU_NOVO_GRUPO_COM, "Criar grupo com esta janela", false, true, None);
            anexar(m, None, 0, "", false, true, None);
            anexar(m, None, MENU_FECHAR, "Fechar janela", false, true, None);
            match mostrar_menu(dono, m, pt, false) {
                MENU_FECHAR => _ = PostMessageW(Some(hwnd(h)), WM_CLOSE, WPARAM(0), LPARAM(0)),
                MENU_NOVO_GRUPO_COM => _ = com(|st| {
                    let i = st.grupos.novo();
                    st.grupos.mover(h, i);
                    redesenhar(st);
                }),
                i if i >= MENU_MOVER => _ = com(|st| {
                    st.grupos.mover(h, i - MENU_MOVER);
                    redesenhar(st);
                }),
                _ => {}
            }
        },
        _ => {
            let alt_tab = teclado::ALT_TAB.load(Ordering::Relaxed);
            let Some(m) = montar_menu(&[
                (MENU_VISAO, "Abrir a visão geral	Win+Tab".into(), false, true),
                (MENU_MOLDURAS, "Aplicar o tema às janelas e aos menus dos outros programas".into(), molduras::ligadas(), true),
                (MENU_CORES, "Aplicar o tema às cores do Windows".into(), cores_do_windows::ligadas(), true),
                (0, String::new(), false, true),
                (MENU_ESCONDER, "Ocultar a barra de tarefas do Windows".into(), esconder, true),
                (MENU_ALT_TAB, "Usar Alt+Tab para a visão geral".into(), alt_tab, true),
                (MENU_INICIAR, "Iniciar com o Windows".into(), sistema::inicia_com_windows(), true),
                (MENU_JUNTAR, "Reunir todas as janelas em um único grupo".into(), false, true),
                (0, String::new(), false, true),
                (MENU_SAIR, "Sair".into(), false, true),
            ]) else {
                return;
            };
            // submenu com os temas do Omarchy, logo depois da visão geral
            let atual = com(|st| st.gfx.tema.nome).unwrap_or("");
            let temas: Vec<Item> = tema::TEMAS.iter().enumerate().map(|(i, t)| (MENU_TEMA + i, tema::nome_de_exibicao(t.nome), t.nome == atual, true)).collect();
            if let Some(sub) = montar_menu(&temas) {
                anexar(m, Some(1), MENU_SUBMENU, "Tema", false, true, Some(sub));
            }
            match mostrar_menu(dono, m, pt, false) {
                MENU_VISAO => abrir_visao(false, None),
                i if (MENU_TEMA..MENU_TEMA + tema::TEMAS.len()).contains(&i) => {
                    sistema::gravar(PREF_TEMA, Some((i - MENU_TEMA) as u32));
                    _ = com(|st| {
                        st.gfx.tema = tema::por_indice((i - MENU_TEMA) as u32);
                        atualizar(st); // repinta as molduras no tema novo e redesenha
                    });
                }
                MENU_MOLDURAS => {
                    let ligar = !molduras::ligadas();
                    sistema::gravar(molduras::PREF, Some(ligar as u32));
                    molduras::ligar(ligar);
                    _ = com(atualizar);
                }
                MENU_CORES => {
                    let ligar = !cores_do_windows::ligadas();
                    sistema::gravar(cores_do_windows::PREF, Some(ligar as u32));
                    cores_do_windows::ligar(ligar);
                    _ = com(atualizar);
                }
                MENU_ESCONDER => {
                    let ligar = !esconder;
                    sistema::gravar(sistema::PREF_ESCONDER, Some(ligar as u32));
                    if ligar {
                        sistema::esconder_barra_windows();
                        bandeja::ligar(barra);
                    } else {
                        bandeja::desligar();
                        sistema::mostrar_barra_windows();
                    }
                    _ = com(|st| {
                        st.esconder_windows = ligar;
                        posicionar(st);
                    });
                }
                MENU_ALT_TAB => {
                    teclado::ALT_TAB.store(!alt_tab, Ordering::Relaxed);
                    sistema::gravar(PREF_ALT_TAB, Some(!alt_tab as u32));
                }
                MENU_INICIAR => sistema::definir_inicio(!sistema::inicia_com_windows()),
                MENU_JUNTAR => _ = com(|st| {
                    st.grupos.juntar_tudo();
                    redesenhar(st);
                }),
                MENU_SAIR => unsafe { _ = DestroyWindow(barra) },
                _ => {}
            }
        }
    }
}

/// Saída e desligamento: janelas de volta à tela, ícones e barra do Windows de volta.
fn limpar() {
    fechar_visao(false);
    grupos::trazer_todas_de_volta();
    bandeja::desligar();
    menus_externos::desligar();
    molduras::restaurar();
    cores_do_windows::restaurar();
    _ = com(|st| appbar::remover(st.barra));
    if sistema::barra_windows_escondida() {
        sistema::mostrar_barra_windows();
    }
}

fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn ao_primeiro_plano(_: HWINEVENTHOOK, _: u32, h: HWND, _: i32, _: i32, _: u32, _: u32) {
    // só repassa: o estado é tratado no laço de mensagens
    let barra = HWND(BARRA.load(Ordering::Relaxed) as *mut _);
    _ = PostMessageW(Some(barra), WM_PRIMEIRO_PLANO, WPARAM(h.0 as usize), LPARAM(0));
}

unsafe extern "system" fn proc_janela(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // itens dos menus de contexto, desenhados no tema (o dono do menu recebe as mensagens)
    if msg == WM_MEASUREITEM && wp.0 == 0 {
        let mi = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
        if mi.CtlType == ODT_MENU {
            medir_item_menu(mi);
            return LRESULT(1);
        }
    }
    if msg == WM_ENTERIDLE && wp.0 == MSGF_MENU as usize {
        vestir_menu(HWND(lp.0 as *mut _));
    }
    if msg == WM_DRAWITEM && wp.0 == 0 {
        let di = &*(lp.0 as *const DRAWITEMSTRUCT);
        if di.CtlType == ODT_MENU {
            desenhar_item_menu(di);
            return LRESULT(1);
        }
    }
    if h.0 as isize == BARRA.load(Ordering::Relaxed) {
        proc_barra(h, msg, wp, lp)
    } else if h.0 as isize == VISAO.load(Ordering::Relaxed) {
        proc_visao(h, msg, wp, lp)
    } else if h.0 as isize == PAINEL.load(Ordering::Relaxed) {
        proc_painel(h, msg, wp, lp)
    } else if h.0 as isize == CENTRAL.load(Ordering::Relaxed) {
        proc_central(h, msg, wp, lp)
    } else if h.0 as isize == DICA.load(Ordering::Relaxed) && msg == WM_PAINT {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(h, &mut ps);
        let mut r = RECT::default();
        _ = GetClientRect(h, &mut r);
        _ = com(|st| {
            let t = st.dica_texto.clone();
            _ = st.gfx.desenhar_dica(dc, r.right, r.bottom, &t, st.escala);
        });
        _ = EndPaint(h, &ps);
        LRESULT(0)
    } else if msg == WM_MOUSEACTIVATE && h.0 as isize == DICA.load(Ordering::Relaxed) {
        LRESULT(MA_NOACTIVATE as isize)
    } else {
        DefWindowProcW(h, msg, wp, lp)
    }
}

unsafe fn proc_barra(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let (msg_shell, msg_barra_criada) = com(|st| (st.msg_shell, st.msg_barra_criada)).unwrap_or((0, 0));
    if msg != 0 && msg == msg_shell {
        appbar::conferir_tela_cheia(h);
        _ = com(|st| {
            let j = lp.0 as isize;
            match wp.0 {
                HSHELL_PISCA => _ = st.piscando.insert(j),
                HSHELL_ATIVADA | HSHELL_ATIVADA_RUDE => _ = st.piscando.remove(&j),
                HSHELL_REDESENHO => _ = st.icones.remove(&j),
                HSHELL_CRIADA | HSHELL_DESTRUIDA => {}
                _ => return,
            }
            atualizar(st);
        });
        return LRESULT(0);
    }
    if msg != 0 && msg == msg_barra_criada {
        // Explorer reiniciou: ele esquece as AppBars e mostra a barra dele de novo
        _ = com(|st| {
            appbar::registrar(st.barra, WM_APPBAR);
            if st.esconder_windows {
                sistema::esconder_barra_windows();
            }
            posicionar(st);
        });
        return LRESULT(0);
    }
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut ps);
            let mut r = RECT::default();
            _ = GetClientRect(h, &mut r);
            _ = com(|st| {
                let c = cena(st, r.right, r.bottom);
                if let Ok(a) = st.gfx.desenhar(dc, &c) {
                    st.alvos = a;
                }
                // a área da bandeja vai para a Shell_TrayWnd própria (programas leem dali)
                let mut b = RECT::default();
                _ = GetWindowRect(st.barra, &mut b);
                let icones: Vec<_> = st.alvos.iter().filter(|(_, a)| matches!(a, Alvo::Bandeja(_))).map(|(r, _)| *r).collect();
                let area = match (icones.first(), icones.last()) {
                    (Some(p), Some(u)) => RECT {
                        left: b.left + p.left.min(u.left) as i32,
                        top: b.top,
                        right: b.left + p.right.max(u.right) as i32,
                        bottom: b.bottom,
                    },
                    _ => RECT { left: b.right - 200, top: b.top, right: b.right - 150, bottom: b.bottom },
                };
                bandeja::posicionar(b, area);
            });
            _ = EndPaint(h, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            _ = com(|st| {
                let a = alvo_em(&st.alvos, x, y);
                if a != st.hover {
                    st.hover = a;
                    mostrar_dica(st);
                    redesenhar(st);
                }
            });
            let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
            _ = TrackMouseEvent(&mut t);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            _ = com(|st| {
                st.hover = None;
                mostrar_dica(st);
                redesenhar(st);
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = xy(lp);
            if let Some(Some(a)) = com(|st| alvo_em(&st.alvos, x, y)) {
                clicar(a);
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = xy(lp);
            if let Some(Some(Alvo::Bandeja(i))) = com(|st| alvo_em(&st.alvos, x, y)) {
                bandeja::clicar(i, bandeja::Botao::Duplo, cursor());
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let (x, y) = xy(lp);
            if let Some(a) = com(|st| alvo_em(&st.alvos, x, y)) {
                clicar_direito(a);
            }
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            // clique do meio no volume: mudo
            let (x, y) = xy(lp);
            _ = com(|st| {
                if alvo_em(&st.alvos, x, y) == Some(Alvo::Volume) {
                    if let Some(a) = &st.audio {
                        a.alternar_mudo();
                    }
                    mostrar_dica(st);
                    invalidar(st.painel.hwnd);
                    redesenhar(st);
                }
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // roda sobre os grupos: grupo seguinte/anterior; sobre o volume: 2% por passo
            let mut pt = POINT { x: xy(lp).0, y: xy(lp).1 };
            _ = ScreenToClient(h, &mut pt);
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16;
            let no_volume = com(|st| {
                let sim = alvo_em(&st.alvos, pt.x, pt.y) == Some(Alvo::Volume);
                if sim {
                    if let Some(a) = &st.audio {
                        a.somar(if delta > 0 { 0.02 } else { -0.02 });
                    }
                    mostrar_dica(st);
                    invalidar(st.painel.hwnd);
                    redesenhar(st);
                }
                sim
            });
            if no_volume == Some(true) {
                return LRESULT(0);
            }
            let destino = com(|st| {
                let sobre_grupos = matches!(alvo_em(&st.alvos, pt.x, pt.y), Some(Alvo::Grupo(_) | Alvo::NovoGrupo));
                let n = st.grupos.lista.len();
                sobre_grupos.then(|| if delta < 0 { (st.grupos.ativo + 1) % n } else { (st.grupos.ativo + n - 1) % n })
            });
            if let Some(Some(i)) = destino {
                ir(i, None);
            }
            LRESULT(0)
        }
        WM_PRIMEIRO_PLANO => {
            // janela de outro grupo veio para a frente (barra do Windows, notificação):
            // troca para o grupo dela
            let j = raiz(HWND(wp.0 as *mut _));
            appbar::conferir_tela_cheia(h);
            _ = com(|st| {
                atualizar(st);
                st.piscando.remove(&j);
                if st.mru.contains(&j) {
                    st.mru.retain(|&x| x != j);
                    st.mru.insert(0, j);
                }
                if let Some(g) = st.grupos.grupo_de(j) {
                    if g != st.grupos.ativo && !st.visao.aberta {
                        st.grupos.trocar(g);
                    }
                    st.grupos.anotar_foco(j);
                }
                redesenhar(st);
            });
            LRESULT(0)
        }
        teclado::WM_ALT_TAB => {
            teclado::mascarar();
            let volta = wp.0 != 0;
            if com(|st| st.visao.aberta) == Some(true) {
                _ = com(|st| andar(st, false, if volta { -1 } else { 1 }));
                return LRESULT(0);
            }
            // a visão geral abre no primeiro Tab, com a janela de antes (ou a última, com Shift)
            let sel = com(|st| {
                let n = st.mru.len();
                (n > 0).then(|| st.mru[if volta { n - 1 } else { 1.min(n - 1) }])
            });
            match sel {
                Some(Some(sel)) => abrir_visao(true, Some(sel)),
                _ => MODO_ALT.store(false, Ordering::Relaxed),
            }
            LRESULT(0)
        }
        teclado::WM_ALT_SOLTO => {
            if MODO_ALT.load(Ordering::Relaxed) && com(|st| st.visao.aberta) == Some(true) {
                escolher_selecao();
            }
            MODO_ALT.store(false, Ordering::Relaxed);
            LRESULT(0)
        }
        teclado::WM_ESC_ALT => {
            fechar_visao(true);
            LRESULT(0)
        }
teclado::WM_GRUPO_VIZINHO => {            teclado::mascarar();            let destino = com(|st| {                let a = st.grupos.ativo;                if wp.0 == 1 { (a + 1 < st.grupos.lista.len()).then_some(a + 1) } else { a.checked_sub(1) }            });            if let Some(Some(g)) = destino {                fechar_visao(false);                ir(g, None);            }            LRESULT(0)        }
        teclado::WM_WIN_TAB => {
            teclado::mascarar();
            if com(|st| st.visao.aberta) == Some(true) {
                fechar_visao(true);
            } else {
                abrir_visao(false, Some(primeiro_plano()));
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER => {
            appbar::conferir_tela_cheia(h);
            _ = com(|st| {
                if st.esconder_windows {
                    sistema::manter_escondida();
                }
                if let Some(a) = st.audio.as_mut() {
                    a.renovar();
                }
                renovar_rede(st);
                renovar_painel(st);
                if st.central.dados.recarregar() && st.central.aberta {
                    invalidar(st.central.hwnd);
                }
                atualizar(st);
            });
            LRESULT(0)
        }
        teclado::WM_CENTRAL => {
            teclado::mascarar();
            alternar_central();
            LRESULT(0)
        }
        WM_APPBAR => {
            match wp.0 {
                ABN_POSICAO | ABN_ESTADO => _ = com(posicionar),
                ABN_TELA_CHEIA => appbar::conferir_tela_cheia(h),
                _ => {}
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            appbar::ativada(h);
            DefWindowProcW(h, msg, wp, lp)
        }
        WM_WINDOWPOSCHANGED => {
            appbar::movida(h);
            DefWindowProcW(h, msg, wp, lp)
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            _ = com(posicionar);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            _ = com(|st| {
                redesenhar(st);
            });
            DefWindowProcW(h, msg, wp, lp)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            // desligando: os programas gravam onde estavam ao fechar; nenhum pode ficar fora da tela
            if wp.0 != 0 {
                limpar();
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            _ = DestroyWindow(h);
            LRESULT(0)
        }
        WM_DESTROY => {
            limpar();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

/// O que fazer depois de soltar o botão na visão geral (fora do acesso ao estado).
enum Acao {
    Nada,
    Ir(usize, Option<isize>),
    NovoEIr,
}

unsafe fn proc_visao(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(h, &mut ps);
            let mut r = RECT::default();
            _ = GetClientRect(h, &mut r);
            _ = com(|st| {
                let c = cena_visao(st, r.right, r.bottom);
                _ = st.gfx.desenhar_visao(h, &c);
            });
            _ = EndPaint(h, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            _ = com(|st| {
                let (fx, fy) = (x as f32, y as f32);
                let v = &mut st.visao;
                if let Some(a) = v.arrasto.as_mut() {
                    a.atual = (x, y);
                    if !a.ativo && ((x - a.inicio.0).abs() > 6 || (y - a.inicio.1).abs() > 6) {
                        a.ativo = true;
                    }
                    if a.ativo {
                        v.aplicar_miniaturas();
                    }
                }
                let hv = if v.arrasto.as_ref().is_some_and(|a| a.ativo) { None } else { v.item_em(fx, fy) };
                let hc = v.cartao_em(fx, fy);
                if hv != v.hover || hc != v.hover_cartao {
                    v.hover = hv;
                    v.hover_cartao = hc;
                    invalidar(h);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = xy(lp);
            _ = SetCapture(h);
            _ = com(|st| {
                // no Alt+Tab não tem arrastar entre grupos: clique só escolhe
                st.visao.arrasto = st.visao.item_em(x as f32, y as f32).filter(|_| !st.visao.lista).map(|item| visao::Arrasto { item, inicio: (x, y), atual: (x, y), ativo: false });
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = xy(lp);
            _ = ReleaseCapture();
            let acao = com(|st| {
                let (fx, fy) = (x as f32, y as f32);
                let arrasto = st.visao.arrasto.take();
                let cartao = st.visao.cartao_em(fx, fy).map(|i| st.visao.cartoes[i].grupo);
                match arrasto {
                    Some(a) if a.ativo => {
                        // soltou a janela num cartão: muda de grupo
                        let j = st.visao.itens[a.item].hwnd;
                        match cartao {
                            Some(Some(g)) => st.grupos.mover(j, g),
                            Some(None) => {
                                let g = st.grupos.novo();
                                st.grupos.mover(j, g);
                            }
                            None => {}
                        }
                        remontar_visao(st);
                        redesenhar(st);
                        Acao::Nada
                    }
                    Some(a) => {
                        let it = &st.visao.itens[a.item];
                        Acao::Ir(it.grupo, Some(it.hwnd))
                    }
                    // Alt+Tab: clique numa janela vai para ela
                    None if st.visao.lista => match st.visao.item_em(fx, fy) {
                        Some(i) => Acao::Ir(st.visao.itens[i].grupo, Some(st.visao.itens[i].hwnd)),
                        None => Acao::Nada,
                    },
                    None => match cartao {
                        Some(Some(g)) => Acao::Ir(g, None),
                        Some(None) => Acao::NovoEIr,
                        None => Acao::Nada,
                    },
                }
            })
            .unwrap_or(Acao::Nada);
            match acao {
                Acao::Ir(g, j) => {
                    fechar_visao(false);
                    ir(g, j);
                }
                // cria o grupo vazio e fica na visão geral, para arrastar janelas para ele
                Acao::NovoEIr => _ = com(|st| {
                    st.grupos.novo();
                    remontar_visao(st);
                    redesenhar(st);
                }),
                Acao::Nada => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = xy(lp);
            let g = com(|st| {
                let (fx, fy) = (x as f32, y as f32);
                st.visao.cartoes.iter().find(|c| fx >= c.cab.left && fx < c.cab.right && fy >= c.cab.top && fy < c.cab.bottom).and_then(|c| c.grupo)
            });
            if let Some(Some(g)) = g {
                comecar_renomear(g);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let (x, y) = xy(lp);
            let g = com(|st| st.visao.cartao_em(x as f32, y as f32).and_then(|i| st.visao.cartoes[i].grupo));
            if let Some(Some(g)) = g {
                menu_grupo(h, g, cursor(), false);
            }
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = VIRTUAL_KEY(wp.0 as u16);
            // tecla pode ter vindo do gancho (sem foco): o estado do Shift vem do teclado físico
            let shift = GetAsyncKeyState(VK_SHIFT.0 as i32) < 0;
            let lista = com(|st| st.visao.lista).unwrap_or(false);
            match vk {
                VK_ESCAPE => fechar_visao(true),
                VK_RETURN => escolher_selecao(),
                // Alt+Tab: as setas andam na fila
                VK_LEFT | VK_UP if lista => _ = com(|st| andar(st, true, -1)),
                VK_RIGHT | VK_DOWN if lista => _ = com(|st| andar(st, true, 1)),
                // Win+Tab: Shift+←/→ leva a janela para o grupo ao lado; setas andam para qualquer lado
                VK_LEFT if shift => _ = com(|st| mover_para_vizinho(st, -1)),
                VK_RIGHT if shift => _ = com(|st| mover_para_vizinho(st, 1)),
                VK_LEFT => _ = com(|st| navegar(st, -1.0, 0.0)),
                VK_RIGHT => _ = com(|st| navegar(st, 1.0, 0.0)),
                VK_UP => _ = com(|st| navegar(st, 0.0, -1.0)),
                VK_DOWN => _ = com(|st| navegar(st, 0.0, 1.0)),
                VK_TAB if lista => _ = com(|st| andar(st, false, if shift { -1 } else { 1 })),
                // Win+Tab: Tab escolhe a janela dentro do grupo selecionado
                VK_TAB => _ = com(|st| andar_no_grupo(st, if shift { -1 } else { 1 })),
                // Delete: apaga o grupo escolhido (as janelas dele vão para o grupo anterior)
                VK_DELETE if !lista => _ = com(apagar_grupo_escolhido),
                VK_F2 => {
                    let g = com(|st| {
                        let porjanela = st.visao.selecao.and_then(|j| st.grupos.grupo_de(j));
                        porjanela.or_else(|| st.visao.cartao_sel.and_then(|c| st.visao.cartoes.get(c)).and_then(|c| c.grupo))
                    });
                    if let Some(Some(g)) = g {
                        comecar_renomear(g);
                    }
                }
                VK_BACK => _ = com(|st| {
                    st.visao.filtro.pop();
                    st.visao.aplicar_miniaturas();
                    invalidar(h);
                }),
                _ => return DefWindowProcW(h, msg, wp, lp),
            }
            LRESULT(0)
        }
        WM_CHAR => {
            // a tecla de máscara (0xE8) do Alt/Win+Tab e espaços no começo não entram no filtro
            let (vazio, lista) = com(|st| (st.visao.filtro.is_empty(), st.visao.lista)).unwrap_or((true, false));
            let Some(c) = char::from_u32(wp.0 as u32).filter(|c| !c.is_control() && !(vazio && c.is_whitespace()) && (*c as u32) < 0xE000) else {
                return LRESULT(0);
            };
            // Win+Tab, sem filtro: 1–9 vai para o grupo, + cria um grupo
            if vazio && !lista {
                if let Some(d) = c.to_digit(10).filter(|d| *d >= 1) {
                    if com(|st| (d as usize) <= st.grupos.lista.len()) == Some(true) {
                        fechar_visao(false);
                        ir(d as usize - 1, None);
                    }
                    return LRESULT(0);
                }
                // - (e Shift+-, que dá _) apaga o grupo escolhido, como o Delete
                if c == '-' || c == '_' {
                    _ = com(apagar_grupo_escolhido);
                    return LRESULT(0);
                }
                if c == '+' {
                    _ = com(novo_grupo_na_visao);
                    return LRESULT(0);
                }
            }
            _ = com(|st| {
                st.visao.filtro.push(c);
                st.visao.aplicar_miniaturas();
                // seleção vai para a primeira que casa (na ordem do Alt+Tab)
                if !st.visao.selecao.is_some_and(|s| st.visao.casa(s)) {
                    st.visao.selecao = st.mru.iter().copied().find(|&j| st.visao.casa(j) && st.visao.itens.iter().any(|i| i.hwnd == j));
                    st.visao.cartao_sel = None;
                }
                invalidar(h);
            });
            LRESULT(0)
        }
        WM_RENOMEAR => {
            _ = com(|st| {
                let Some((g, e)) = st.visao.editando.take() else { return };
                if wp.0 == 1 {
                    let nome = janelas::titulo(e).trim().to_string();
                    if !nome.is_empty() && g < st.grupos.lista.len() {
                        st.grupos.lista[g].nome = nome;
                        st.grupos.lista[g].nome_proprio = true;
                    }
                }
                _ = DestroyWindow(e);
                _ = SetFocus(Some(h));
                redesenhar(st);
            });
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            // caixa de renomear no tema: `texto_forte` sobre `fundo_claro`
            let dc = HDC(wp.0 as *mut _);
            let colorref = |c: u32| COLORREF((c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16) & 0xFF);
            let (fundo, texto) = com(|st| (st.gfx.tema.fundo_claro, st.gfx.tema.texto_forte)).unwrap_or((0x24283b, 0xc0caf5));
            SetTextColor(dc, colorref(texto));
            SetBkColor(dc, colorref(fundo));
            thread_local! {
                static PINCEL: std::cell::Cell<(u32, HBRUSH)> = const { std::cell::Cell::new((u32::MAX, HBRUSH(std::ptr::null_mut()))) };
            }
            let pincel = PINCEL.with(|p| {
                let (c, b) = p.get();
                if c == fundo {
                    return b;
                }
                if !b.is_invalid() {
                    _ = DeleteObject(b.into());
                }
                let novo = CreateSolidBrush(colorref(fundo));
                p.set((fundo, novo));
                novo
            });
            LRESULT(pincel.0 as isize)
        }
        WM_ACTIVATE => {
            // outra janela tomou o foco (clique em outro lugar, Win): fecha sem roubar de volta
            if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                let lp_alvo = HWND(lp.0 as *mut _);
                let nossa = !lp_alvo.is_invalid() && GetAncestor(lp_alvo, GA_ROOT) == h;
                if !nossa {
                    fechar_visao(false);
                }
            }
            DefWindowProcW(h, msg, wp, lp)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    unsafe {
        _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if args.len() > 2 && args[1] == "--preview" {
            return preview(&args[2]);
        }
        if args.get(1).is_some_and(|a| a == "--sair") {
            if let Ok(b) = FindWindowW(CLASSE, TITULO) {
                _ = PostMessageW(Some(b), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            return Ok(());
        }
        let _mutex = CreateMutexW(None, true, w!("Local\\barra"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }

        // a barra anterior morreu com janelas estacionadas: traz de volta antes de tudo
        grupos::recuperar();
        sistema::migrar_inicio();
        if !cores_do_windows::ligadas() {
            cores_do_windows::restaurar();
        }
        std::panic::set_hook(Box::new(|_| {
            grupos::trazer_todas_de_volta();
            sistema::mostrar_barra_windows();
            menus_externos::desligar();
            molduras::restaurar();
            cores_do_windows::restaurar();
        }));

        let instancia = GetModuleHandleW(PCWSTR::null())?;
        let wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(proc_janela),
            hInstance: instancia.into(),
            lpszClassName: CLASSE,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let criar = |ex: WINDOW_EX_STYLE, titulo: PCWSTR| {
            CreateWindowExW(ex, CLASSE, titulo, WS_POPUP, 0, 0, 0, 0, None, None, Some(instancia.into()), None)
        };
        let barra = criar(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, TITULO)?;
        let menus = criar(WS_EX_TOOLWINDOW, w!("barra-menus"))?;
        let dica = criar(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, w!("barra-dica"))?;
        let visao_hwnd = criar(WS_EX_TOOLWINDOW | WS_EX_TOPMOST, w!("barra-visao"))?;
        let painel_hwnd = criar(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, w!("barra-painel"))?;
        let central_hwnd = criar(WS_EX_TOOLWINDOW | WS_EX_TOPMOST, w!("barra-central"))?;
        {
            use windows::Win32::Graphics::Dwm::*;
            let canto = DWMWCP_DONOTROUND.0 as u32;
            for j in [painel_hwnd, central_hwnd] {
                _ = DwmSetWindowAttribute(j, DWMWA_WINDOW_CORNER_PREFERENCE, &canto as *const _ as *const _, 4);
            }
        }
        BARRA.store(barra.0 as isize, Ordering::Relaxed);
        VISAO.store(visao_hwnd.0 as isize, Ordering::Relaxed);
        teclado::VISAO.store(visao_hwnd.0 as isize, Ordering::Relaxed);
        DICA.store(dica.0 as isize, Ordering::Relaxed);
        PAINEL.store(painel_hwnd.0 as isize, Ordering::Relaxed);
        teclado::PAINEL.store(painel_hwnd.0 as isize, Ordering::Relaxed);
        CENTRAL.store(central_hwnd.0 as isize, Ordering::Relaxed);
        teclado::CENTRAL.store(central_hwnd.0 as isize, Ordering::Relaxed);

        let atuais = janelas::listar();
        let mut grupos = grupos::Grupos::new();
        grupos.sincronizar(&atuais);
        let esconder = sistema::ler(sistema::PREF_ESCONDER) != Some(0);
        teclado::ALT_TAB.store(sistema::ler(PREF_ALT_TAB) != Some(0), Ordering::Relaxed);
        ESTADO.with_borrow_mut(|e| {
            *e = Some(Estado {
                barra,
                menus,
                dica,
                dica_texto: String::new(),
                visao_hwnd,
                gfx: desenho::Gfx::new(tema::por_indice(sistema::ler(PREF_TEMA).unwrap_or(0))).expect("Direct2D"),
                grupos,
                visao: visao::Visao { janela: visao_hwnd, ..Default::default() },
                mru: atuais,
                alvos: Vec::new(),
                hover: None,
                piscando: HashSet::new(),
                icones: HashMap::new(),
                glifos: HashMap::new(),
                icone_terminal: None,
                msg_shell: RegisterWindowMessageW(w!("SHELLHOOK")),
                msg_barra_criada: RegisterWindowMessageW(w!("TaskbarCreated")),
                esconder_windows: esconder,
                escala: 1.0,
                painel: painel::Painel { hwnd: painel_hwnd, ..Default::default() },
                audio: audio::Audio::new(),
                rede: (rede::G_SEM_REDE, false, String::new()),
                central: central::Central { hwnd: central_hwnd, ..Default::default() },
            })
        });
        _ = com(|st| st.central.dados.recarregar());
        iniciar_servico();
        _ = com(renovar_rede);

        if esconder {
            sistema::esconder_barra_windows();
        }
        appbar::registrar(barra, WM_APPBAR);
        _ = com(posicionar);
        _ = com(atualizar); // molduras das janelas abertas já no tema
        _ = ShowWindow(barra, SW_SHOWNOACTIVATE);
        if esconder {
            bandeja::ligar(barra);
        }
        _ = RegisterShellHookWindow(barra);
        let gancho = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(ao_primeiro_plano),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        if gancho.is_invalid() {
            return Err(windows::core::Error::from_win32());
        }
        teclado::instalar(barra);
        SetTimer(Some(barra), TIMER, 1000, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

/// `barra --preview <arquivo.bmp>`: desenha uma barra de exemplo num BMP, sem abrir nada na tela.
fn preview(saida: &str) -> Result<()> {
    let (w, h) = (2560, ALTURA as i32);
    let tema_i = std::env::args().nth(3).and_then(|n| tema::TEMAS.iter().position(|t| t.nome == n)).unwrap_or(0);
    let mut gfx = desenho::Gfx::new(&tema::TEMAS[tema_i])?;
    let icone = unsafe { LoadIconW(None, IDI_APPLICATION)? };
    let janela = |grupo: usize, t: &str, ativa: bool, minimizada: bool, piscando: bool, progresso: Option<(u32, f32)>| ItemJanela {
        grupo,
        glifo: t.starts_with("pwsh").then_some(desenho::G_TERMINAL),
        hwnd: t.len() as isize,
        icone,
        ativa,
        minimizada,
        piscando,
        selo: HICON::default(),
        progresso,
    };
    let grupo = |n: &str, q: usize, ativo: bool| ItemGrupo { nome: n.into(), qtd: q, ativo, proprio: false };
    let cena = Cena {
        w,
        h,
        escala: 1.0,
        grupos: vec![grupo("1", 3, false), grupo("2", 4, true), grupo("3", 6, false), grupo("4", 0, false), grupo("geral", 2, false)],
        janelas: vec![
            janela(0, "Excel", false, false, false, None),
            janela(1, "pwsh — projeto-a\\build", true, false, false, None),
            janela(1, "pwsh — testes", false, false, false, Some((2, 0.6))),
            janela(1, "GitHub — snap-zones e mais um título comprido", false, false, true, None),
            janela(1, "Docs", false, true, false, None),
            janela(2, "Outlook", false, false, false, None),
            janela(2, "Teams", false, false, false, None),
        ],
        hover: Some(Alvo::Grupo(4)),
        hora: "Terça-feira 20:25".into(),
        bandeja: (0..4).map(|i| ItemBandeja { indice: i, icone, glifo: (i == 1).then_some(desenho::G_TECLADO) }).collect(),
        rede: (rede::G_CABO, true),
        volume: Some((0.45, false)),
        aberto: None,
        aviso: None,
        alerta: false,
    };
    unsafe {
        let dc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let antigo = SelectObject(dc, bmp.into());
        gfx.desenhar(dc, &cena)?;
        let px = std::slice::from_raw_parts(bits as *const u32, (w * h) as usize);
        let mut arq = Vec::with_capacity(54 + px.len() * 4);
        let tam = 54 + px.len() as u32 * 4;
        arq.extend_from_slice(b"BM");
        arq.extend_from_slice(&tam.to_le_bytes());
        arq.extend_from_slice(&[0, 0, 0, 0]);
        arq.extend_from_slice(&54u32.to_le_bytes());
        arq.extend_from_slice(&40u32.to_le_bytes());
        arq.extend_from_slice(&w.to_le_bytes());
        arq.extend_from_slice(&(-h).to_le_bytes());
        arq.extend_from_slice(&1u16.to_le_bytes());
        arq.extend_from_slice(&32u16.to_le_bytes());
        arq.extend_from_slice(&[0u8; 24]);
        for p in px {
            arq.extend_from_slice(&(p | 0xFF00_0000).to_le_bytes());
        }
        _ = std::fs::write(saida, arq);
        SelectObject(dc, antigo);
        _ = DeleteObject(bmp.into());
        _ = DeleteDC(dc);
    }
    Ok(())
}
