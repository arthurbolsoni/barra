#![windows_subsystem = "windows"]

mod appbar;
mod bandeja;
mod desenho;
mod grupos;
mod janelas;
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
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use desenho::{Alvo, Alvos, CartaoV, Cena, CenaVisao, ItemBandeja, ItemGrupo, ItemJanela, JanelaV};
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
const TIMER: usize = 1;
const DIAS: [&str; 7] = ["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"];
const MESES: [&str; 12] = [
    "janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro",
];
const MENU_TEMA: usize = 200;
const TIMER_ALT: usize = 2;
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
    /// Alt+Tab apertado e ainda sem mostrar a visão geral (toque rápido só troca de janela).
    alt_pendente: bool,
    alt_indice: usize,
    alvos: Alvos,
    hover: Option<Alvo>,
    piscando: HashSet<isize>,
    icones: HashMap<isize, HICON>,
    msg_shell: u32,
    msg_barra_criada: u32,
    esconder_windows: bool,
    escala: f32,
}

thread_local! {
    static ESTADO: RefCell<Option<Estado>> = const { RefCell::new(None) };
}

/// Janela da barra, para o gancho de primeiro plano postar mesmo com o estado em uso.
static BARRA: AtomicIsize = AtomicIsize::new(0);
static VISAO: AtomicIsize = AtomicIsize::new(0);
static DICA: AtomicIsize = AtomicIsize::new(0);

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
    st.piscando.retain(|h| atuais.contains(h));
    st.mru.retain(|h| atuais.contains(h));
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
        .map(|(i, g)| ItemGrupo { nome: g.nome.clone(), qtd: g.janelas.len(), ativo: i == ativo })
        .collect();
    let lista = st.grupos.lista[ativo].janelas.clone();
    let janelas = lista
        .into_iter()
        .map(|j| {
            let p = bandeja::progresso(j);
            ItemJanela {
                hwnd: j,
                icone: icone_de(st, j),
                ativa: j == fg,
                minimizada: unsafe { IsIconic(hwnd(j)).as_bool() },
                piscando: st.piscando.contains(&j),
                selo: p.selo,
                progresso: (p.estado != 0).then_some((p.estado, p.fracao)),
            }
        })
        .collect();
    let t = unsafe { GetLocalTime() };
    Cena {
        w,
        h,
        escala: st.escala,
        grupos,
        janelas,
        hover: st.hover,
        hora: format!("{} {:02}:{:02}", DIAS[t.wDayOfWeek as usize % 7], t.wHour, t.wMinute),
        bandeja: bandeja::visiveis().into_iter().map(|(indice, icone, _)| ItemBandeja { indice, icone }).collect(),
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

/// Clique no botão da janela: traz para a frente, ou minimiza se já estava na frente.
fn clicar_janela(h: isize) {
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
        Alvo::Sistema => sistema::abrir_configuracoes_rapidas(),
        Alvo::Relogio => sistema::abrir_notificacoes(),
    }
}

// ---------- dica ----------

fn mostrar_dica(st: &mut Estado) {
    let texto = match st.hover {
        Some(Alvo::Bandeja(i)) => bandeja::visiveis().into_iter().find(|(n, _, _)| *n == i).map(|(_, _, d)| d).unwrap_or_default(),
        Some(Alvo::Janela(h)) => janelas::titulo(hwnd(h)),
        Some(Alvo::Relogio) => {
            let t = unsafe { GetLocalTime() };
            format!("{}, {} de {} de {}", DIAS[t.wDayOfWeek as usize % 7], t.wDay, MESES[(t.wMonth as usize + 11) % 12], t.wYear)
        }
        Some(Alvo::Grupo(i)) => st.grupos.lista.get(i).map(|g| match g.janelas.len() {
            0 => format!("{}: vazio", g.nome),
            1 => format!("{}: 1 janela", g.nome),
            n => format!("{}: {n} janelas", g.nome),
        }).unwrap_or_default(),
        _ => String::new(),
    };
    let r = st.hover.and_then(|a| st.alvos.iter().find(|(_, b)| *b == a).map(|(r, _)| *r));
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
    forcar_primeiro_plano(v);
}

/// Fecha a visão geral; `voltar` devolve o foco para a janela de antes.
fn fechar_visao(voltar: bool) {
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

/// Setas no Win+Tab: vai para o vizinho mais perto naquela direção, entre as janelas que
/// casam com o filtro, os grupos vazios e o "novo grupo".
fn navegar(st: &mut Estado, dx: f32, dy: f32) {
    let v = &st.visao;
    let centro = |r: &windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F| ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
    let mut pontos: Vec<((f32, f32), Option<isize>, Option<usize>)> =
        v.itens.iter().filter(|i| v.casa(i.hwnd)).map(|i| (centro(&i.r), Some(i.hwnd), None)).collect();
    for (ci, c) in v.cartoes.iter().enumerate() {
        if c.grupo.is_none_or(|g| st.grupos.lista.get(g).is_some_and(|x| x.janelas.is_empty())) {
            pontos.push((centro(&c.r), None, Some(ci)));
        }
    }
    let atual = v
        .selecao
        .and_then(|s| v.itens.iter().find(|i| i.hwnd == s).map(|i| centro(&i.r)))
        .or_else(|| v.cartao_sel.and_then(|c| v.cartoes.get(c)).map(|c| centro(&c.r)));
    let escolha = match atual {
        None => pontos.first().map(|p| (p.1, p.2)),
        Some((ax, ay)) => pontos
            .iter()
            .filter_map(|&((px, py), j, c)| {
                let principal = (px - ax) * dx + (py - ay) * dy;
                if principal <= 1.0 {
                    return None;
                }
                let lado = if dx != 0.0 { (py - ay).abs() } else { (px - ax).abs() };
                Some((principal + 2.0 * lado, j, c))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|p| (p.1, p.2)),
    };
    if let Some((j, c)) = escolha {
        st.visao.selecao = j;
        st.visao.cartao_sel = c;
        invalidar(st.visao_hwnd);
    }
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
        let fonte = CreateFontW(
            -(16.0 * s) as i32, 0, 0, 0, FW_SEMIBOLD.0 as i32, 0, 0, 0,
            DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0,
            w!("Segoe UI Variable Text"),
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

fn montar_menu(itens: &[Item]) -> Option<HMENU> {
    unsafe {
        let m = CreatePopupMenu().ok()?;
        for (id, texto, marcado, habilitado) in itens {
            if *id == 0 {
                _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
                continue;
            }
            let mut f = MF_STRING;
            if *marcado {
                f |= MF_CHECKED;
            }
            if !*habilitado {
                f |= MF_GRAYED;
            }
            let t: Vec<u16> = texto.encode_utf16().chain([0]).collect();
            _ = AppendMenuW(m, f, *id, PCWSTR(t.as_ptr()));
        }
        Some(m)
    }
}

/// Mostra o menu no ponto (embaixo: acima da barra) e devolve o id escolhido (0 = nada).
fn mostrar_menu(dono: HWND, m: HMENU, pt: POINT, acima: bool) -> usize {
    unsafe {
        _ = SetForegroundWindow(dono); // sem isso o menu não fecha ao clicar fora
        let mut f = TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY;
        if acima {
            f |= TPM_BOTTOMALIGN;
        }
        let id = TrackPopupMenu(m, f, pt.x, pt.y, None, dono, None);
        _ = PostMessageW(Some(dono), WM_NULL, WPARAM(0), LPARAM(0));
        _ = DestroyMenu(m);
        id.0 as usize
    }
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
const MENU_MOVER: usize = 100;

fn menu_grupo(dono: HWND, i: usize, pt: POINT, acima: bool) {
    let Some(nomes) = com(|st| st.grupos.lista.iter().map(|g| g.nome.clone()).collect::<Vec<_>>()) else { return };
    if i >= nomes.len() {
        return;
    }
    let itens = vec![
        (MENU_RENOMEAR, "Renomear".to_string(), false, true),
        (MENU_NOVO_GRUPO, "Novo grupo".to_string(), false, true),
        (0, String::new(), false, true),
        (MENU_APAGAR_GRUPO, format!("Apagar o grupo {} (as janelas vão para o anterior)", nomes[i]), false, nomes.len() > 1),
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
                _ = AppendMenuW(m, MF_POPUP, sub.0 as usize, w!("Mover para"));
            }
            _ = AppendMenuW(m, MF_STRING, MENU_NOVO_GRUPO_COM, w!("Novo grupo com esta janela"));
            _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
            _ = AppendMenuW(m, MF_STRING, MENU_FECHAR, w!("Fechar janela"));
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
                (MENU_VISAO, "Visão geral (Win+Tab)".into(), false, true),
                (0, String::new(), false, true),
                (MENU_ESCONDER, "Esconder a barra do Windows".into(), esconder, true),
                (MENU_ALT_TAB, "Alt+Tab abre a visão geral".into(), alt_tab, true),
                (MENU_INICIAR, "Iniciar com o Windows".into(), sistema::inicia_com_windows(), true),
                (MENU_JUNTAR, "Juntar todas as janelas num grupo só".into(), false, true),
                (0, String::new(), false, true),
                (MENU_SAIR, "Sair".into(), false, true),
            ]) else {
                return;
            };
            // submenu com os temas do Omarchy, logo depois da visão geral
            let atual = com(|st| st.gfx.tema.nome).unwrap_or("");
            let temas: Vec<Item> = tema::TEMAS.iter().enumerate().map(|(i, t)| (MENU_TEMA + i, t.nome.to_string(), t.nome == atual, true)).collect();
            if let Some(sub) = montar_menu(&temas) {
                unsafe { _ = InsertMenuW(m, 1, MF_BYPOSITION | MF_POPUP, sub.0 as usize, w!("Tema")) };
            }
            match mostrar_menu(dono, m, pt, false) {
                MENU_VISAO => abrir_visao(false, None),
                i if (MENU_TEMA..MENU_TEMA + tema::TEMAS.len()).contains(&i) => {
                    sistema::gravar(PREF_TEMA, Some((i - MENU_TEMA) as u32));
                    _ = com(|st| {
                        st.gfx.tema = tema::por_indice((i - MENU_TEMA) as u32);
                        redesenhar(st);
                    });
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
    if h.0 as isize == BARRA.load(Ordering::Relaxed) {
        proc_barra(h, msg, wp, lp)
    } else if h.0 as isize == VISAO.load(Ordering::Relaxed) {
        proc_visao(h, msg, wp, lp)
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
        WM_MOUSEWHEEL => {
            // roda sobre os grupos: grupo seguinte/anterior
            let mut pt = POINT { x: xy(lp).0, y: xy(lp).1 };
            _ = ScreenToClient(h, &mut pt);
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16;
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
            _ = com(|st| {
                if st.visao.aberta {
                    andar(st, false, if volta { -1 } else { 1 });
                } else if st.alt_pendente {
                    let n = st.mru.len().max(1);
                    st.alt_indice = if volta { (st.alt_indice + n - 1) % n } else { (st.alt_indice + 1) % n };
                } else if !st.mru.is_empty() {
                    st.alt_pendente = true;
                    let n = st.mru.len();
                    st.alt_indice = if volta { n - 1 } else { 1.min(n - 1) };
                    MODO_ALT.store(true, Ordering::Relaxed);
                    SetTimer(Some(h), TIMER_ALT, 160, None);
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_ALT => {
            _ = KillTimer(Some(h), TIMER_ALT);
            // Alt ainda apertado: mostra a visão geral com a janela escolhida até aqui
            if let Some(Some(sel)) = com(|st| {
                let p = std::mem::take(&mut st.alt_pendente);
                p.then(|| st.mru.get(st.alt_indice).copied()).flatten()
            }) {
                abrir_visao(true, Some(sel));
            }
            LRESULT(0)
        }
        teclado::WM_ALT_SOLTO => {
            _ = KillTimer(Some(h), TIMER_ALT);
            let rapido = com(|st| std::mem::take(&mut st.alt_pendente).then(|| st.mru.get(st.alt_indice).copied()).flatten());
            match rapido {
                // toque rápido: vai direto, sem mostrar nada
                Some(Some(j)) => {
                    MODO_ALT.store(false, Ordering::Relaxed);
                    ir_janela(j);
                }
                _ => {
                    if MODO_ALT.load(Ordering::Relaxed) && com(|st| st.visao.aberta) == Some(true) {
                        escolher_selecao();
                    }
                    MODO_ALT.store(false, Ordering::Relaxed);
                }
            }
            LRESULT(0)
        }
        teclado::WM_ESC_ALT => {
            _ = KillTimer(Some(h), TIMER_ALT);
            _ = com(|st| st.alt_pendente = false);
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
            _ = com(|st| {
                if st.esconder_windows {
                    sistema::manter_escondida();
                }
                atualizar(st);
            });
            LRESULT(0)
        }
        WM_APPBAR => {
            match wp.0 {
                ABN_POSICAO | ABN_ESTADO => _ = com(posicionar),
                ABN_TELA_CHEIA => appbar::tela_cheia(h, lp.0 != 0),
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
            let dc = BeginPaint(h, &mut ps);
            let mut r = RECT::default();
            _ = GetClientRect(h, &mut r);
            _ = com(|st| {
                let c = cena_visao(st, r.right, r.bottom);
                _ = st.gfx.desenhar_visao(dc, &c);
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
            let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
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
                VK_TAB => _ = com(|st| andar(st, false, if shift { -1 } else { 1 })),
                // Delete: apaga o grupo escolhido (as janelas dele vão para o grupo anterior)
                VK_DELETE if !lista => _ = com(|st| {
                    let porjanela = st.visao.selecao.and_then(|j| st.grupos.grupo_de(j));
                    let g = porjanela.or_else(|| st.visao.cartao_sel.and_then(|c| st.visao.cartoes.get(c)).and_then(|c| c.grupo));
                    if let Some(g) = g {
                        st.grupos.apagar(g);
                        st.visao.cartao_sel = None;
                        remontar_visao(st);
                        redesenhar(st);
                    }
                }),
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
                    }
                }
                _ = DestroyWindow(e);
                _ = SetFocus(Some(h));
                redesenhar(st);
            });
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let dc = HDC(wp.0 as *mut _);
            SetTextColor(dc, COLORREF(0x00FFFFFF));
            SetBkColor(dc, COLORREF(0x00302C2B));
            thread_local! {
                static FUNDO: HBRUSH = unsafe { CreateSolidBrush(COLORREF(0x00302C2B)) };
            }
            LRESULT(FUNDO.with(|b| b.0 as isize))
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
        std::panic::set_hook(Box::new(|_| {
            grupos::trazer_todas_de_volta();
            sistema::mostrar_barra_windows();
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
        BARRA.store(barra.0 as isize, Ordering::Relaxed);
        VISAO.store(visao_hwnd.0 as isize, Ordering::Relaxed);
        DICA.store(dica.0 as isize, Ordering::Relaxed);

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
                alt_pendente: false,
                alt_indice: 0,
                alvos: Vec::new(),
                hover: None,
                piscando: HashSet::new(),
                icones: HashMap::new(),
                msg_shell: RegisterWindowMessageW(w!("SHELLHOOK")),
                msg_barra_criada: RegisterWindowMessageW(w!("TaskbarCreated")),
                esconder_windows: esconder,
                escala: 1.0,
            })
        });

        if esconder {
            sistema::esconder_barra_windows();
        }
        appbar::registrar(barra, WM_APPBAR);
        _ = com(posicionar);
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
    let janela = |t: &str, ativa: bool, minimizada: bool, piscando: bool, progresso: Option<(u32, f32)>| ItemJanela {
        hwnd: t.len() as isize,
        icone,
        ativa,
        minimizada,
        piscando,
        selo: HICON::default(),
        progresso,
    };
    let grupo = |n: &str, q: usize, ativo: bool| ItemGrupo { nome: n.into(), qtd: q, ativo };
    let cena = Cena {
        w,
        h,
        escala: 1.0,
        grupos: vec![grupo("1", 3, false), grupo("2", 4, true), grupo("3", 6, false), grupo("4", 0, false), grupo("geral", 2, false)],
        janelas: vec![
            janela("pwsh — projeto-a\\build", true, false, false, None),
            janela("pwsh — testes", false, false, false, Some((2, 0.6))),
            janela("GitHub — snap-zones e mais um título comprido", false, false, true, None),
            janela("Docs", false, true, false, None),
        ],
        hover: Some(Alvo::Grupo(4)),
        hora: "terça-feira 20:25".into(),
        bandeja: (0..4).map(|i| ItemBandeja { indice: i, icone }).collect(),
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
