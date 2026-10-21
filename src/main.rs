#![windows_subsystem = "windows"]

mod appbar;
mod desenho;
mod grupos;
mod janelas;
mod sistema;

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
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

use desenho::{Alvo, Alvos, Cena, ItemGrupo, ItemJanela};
use janelas::hwnd;

/// Altura da barra a 100% de escala.
const ALTURA: f32 = 40.0;
const CLASSE: PCWSTR = w!("BarraTarefas");
const TITULO: PCWSTR = w!("barra");
const WM_APPBAR: u32 = WM_APP + 1;
const WM_PRIMEIRO_PLANO: u32 = WM_APP + 2;
const TIMER: usize = 1;
const WM_MOUSELEAVE: u32 = 0x02A3;

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
    /// Janela oculta, dona dos menus.
    menus: HWND,
    gfx: desenho::Gfx,
    grupos: grupos::Grupos,
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

/// Acesso ao estado. Devolve None se ele já está em uso (mensagem que chegou no meio de
/// outra operação, enquanto o Windows espera outra janela responder).
fn com<R>(f: impl FnOnce(&mut Estado) -> R) -> Option<R> {
    ESTADO.with(|e| e.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn redesenhar(st: &Estado) {
    unsafe { _ = InvalidateRect(Some(st.barra), None, false) };
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

fn cena(st: &mut Estado, w: i32, h: i32) -> Cena {
    let fg = primeiro_plano();
    let tam_icone = (32.0 * st.escala) as i32;
    let ativo = st.grupos.ativo;
    let grupos = st
        .grupos
        .lista
        .iter()
        .enumerate()
        .map(|(i, g)| ItemGrupo { nome: g.nome.clone(), qtd: g.janelas.len(), ativo: i == ativo })
        .collect();
    let icones = &mut st.icones;
    let janelas = st.grupos.lista[ativo]
        .janelas
        .iter()
        .map(|&j| ItemJanela {
            hwnd: j,
            titulo: janelas::titulo(hwnd(j)),
            icone: *icones.entry(j).or_insert_with(|| janelas::icone(hwnd(j), tam_icone)),
            ativa: j == fg,
            minimizada: unsafe { IsIconic(hwnd(j)).as_bool() },
            piscando: st.piscando.contains(&j),
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
        hora: format!("{:02}:{:02}", t.wHour, t.wMinute),
        data: format!("{:02}/{:02}/{}", t.wDay, t.wMonth, t.wYear),
    }
}

fn alvo_em(alvos: &Alvos, x: i32, y: i32) -> Option<Alvo> {
    let (x, y) = (x as f32, y as f32);
    alvos.iter().find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(_, a)| *a)
}

fn focar(h: isize) {
    let j = hwnd(h);
    unsafe {
        if IsIconic(j).as_bool() {
            _ = ShowWindow(j, SW_RESTORE);
        }
        _ = SetForegroundWindow(j);
    }
}

fn trocar(i: usize) {
    if let Some(Some(foco)) = com(|st| {
        let f = st.grupos.trocar(i);
        redesenhar(st);
        f
    }) {
        focar(foco);
    }
}

/// Clique no botão da janela: traz para a frente, ou minimiza se já estava na frente.
fn clicar_janela(h: isize) {
    let j = hwnd(h);
    if primeiro_plano() == h && unsafe { !IsIconic(j).as_bool() } {
        unsafe { _ = ShowWindow(j, SW_MINIMIZE) };
    } else {
        focar(h);
    }
}

fn clicar(alvo: Alvo) {
    match alvo {
        Alvo::Iniciar => sistema::abrir_iniciar(),
        Alvo::Grupo(i) => trocar(i),
        Alvo::NovoGrupo => {
            if let Some(i) = com(|st| st.grupos.novo()) {
                trocar(i);
            }
        }
        Alvo::Janela(h) => clicar_janela(h),
        Alvo::Sistema => sistema::abrir_configuracoes_rapidas(),
        Alvo::Relogio => sistema::abrir_notificacoes(),
    }
}

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

/// Mostra o menu acima da barra e devolve o id escolhido (0 = nada).
fn mostrar_menu(dono: HWND, barra: HWND, m: HMENU) -> usize {
    unsafe {
        let mut pt = POINT::default();
        _ = GetCursorPos(&mut pt);
        let mut r = RECT::default();
        _ = GetWindowRect(barra, &mut r);
        _ = SetForegroundWindow(dono); // sem isso o menu não fecha ao clicar fora
        let id = TrackPopupMenu(m, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN, pt.x, r.top, None, dono, None);
        _ = PostMessageW(Some(dono), WM_NULL, WPARAM(0), LPARAM(0));
        _ = DestroyMenu(m);
        id.0 as usize
    }
}

const MENU_NOVO_GRUPO_COM: usize = 2;
const MENU_FECHAR: usize = 3;
const MENU_APAGAR_GRUPO: usize = 10;
const MENU_NOVO_GRUPO: usize = 11;
const MENU_ESCONDER: usize = 20;
const MENU_INICIAR: usize = 21;
const MENU_JUNTAR: usize = 22;
const MENU_SAIR: usize = 23;
const MENU_MOVER: usize = 100;

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

    let menu = match alvo {
        Some(Alvo::Janela(h)) => unsafe {
            let Ok(m) = CreatePopupMenu() else { return };
            let mut outros = vec![];
            for (i, n) in nomes.iter().enumerate() {
                if Some(i) != grupo_da_janela {
                    outros.push((MENU_MOVER + i, format!("Grupo {n}"), false, true));
                }
            }
            if let Some(sub) = montar_menu(&outros).filter(|_| !outros.is_empty()) {
                _ = AppendMenuW(m, MF_POPUP, sub.0 as usize, w!("Mover para"));
            }
            _ = AppendMenuW(m, MF_STRING, MENU_NOVO_GRUPO_COM, w!("Novo grupo com esta janela"));
            _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
            _ = AppendMenuW(m, MF_STRING, MENU_FECHAR, w!("Fechar janela"));
            let id = mostrar_menu(dono, barra, m);
            match id {
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
            return;
        },
        Some(Alvo::Grupo(i)) => {
            let itens = vec![
                (MENU_NOVO_GRUPO, "Novo grupo".to_string(), false, true),
                (0, String::new(), false, true),
                (MENU_APAGAR_GRUPO, format!("Apagar o grupo {} (as janelas vão para o anterior)", nomes[i]), false, nomes.len() > 1),
            ];
            let Some(m) = montar_menu(&itens) else { return };
            match mostrar_menu(dono, barra, m) {
                MENU_NOVO_GRUPO => _ = com(|st| st.grupos.novo()),
                MENU_APAGAR_GRUPO => _ = com(|st| st.grupos.apagar(i)),
                _ => {}
            }
            _ = com(|st| redesenhar(st));
            return;
        }
        _ => montar_menu(&[
            (MENU_ESCONDER, "Esconder a barra do Windows".into(), esconder, true),
            (MENU_INICIAR, "Iniciar com o Windows".into(), sistema::inicia_com_windows(), true),
            (MENU_JUNTAR, "Juntar todas as janelas num grupo só".into(), false, true),
            (0, String::new(), false, true),
            (MENU_SAIR, "Sair".into(), false, true),
        ]),
    };
    let Some(m) = menu else { return };
    match mostrar_menu(dono, barra, m) {
        MENU_ESCONDER => _ = com(|st| {
            st.esconder_windows = !st.esconder_windows;
            if st.esconder_windows {
                sistema::gravar(sistema::PREF_ESCONDER, Some(1));
                sistema::esconder_barra_windows();
            } else {
                sistema::gravar(sistema::PREF_ESCONDER, None);
                sistema::mostrar_barra_windows();
            }
            posicionar(st);
        }),
        MENU_INICIAR => sistema::definir_inicio(!sistema::inicia_com_windows()),
        MENU_JUNTAR => _ = com(|st| {
            st.grupos.juntar_tudo();
            redesenhar(st);
        }),
        MENU_SAIR => unsafe { _ = DestroyWindow(barra) },
        _ => {}
    }
}

/// Saída e desligamento: janelas de volta à tela, barra do Windows de volta.
fn limpar() {
    grupos::trazer_todas_de_volta();
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
    let barra = HWND(BARRA.load(Ordering::Relaxed) as *mut _);
    let (msg_shell, msg_barra_criada) = com(|st| (st.msg_shell, st.msg_barra_criada)).unwrap_or((0, 0));
    if h != barra || barra.is_invalid() {
        return DefWindowProcW(h, msg, wp, lp);
    }
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
                trocar(i);
            }
            LRESULT(0)
        }
        WM_PRIMEIRO_PLANO => {
            // janela de outro grupo veio para a frente (Alt+Tab, clique na notificação):
            // troca para o grupo dela
            let j = raiz(HWND(wp.0 as *mut _));
            _ = com(|st| {
                atualizar(st);
                st.piscando.remove(&j);
                if let Some(g) = st.grupos.grupo_de(j) {
                    if g != st.grupos.ativo {
                        st.grupos.trocar(g);
                    }
                    st.grupos.anotar_foco(j);
                }
                redesenhar(st);
            });
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
                st.gfx.atualizar_destaque();
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
            lpfnWndProc: Some(proc_janela),
            hInstance: instancia.into(),
            lpszClassName: CLASSE,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let barra = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            CLASSE,
            TITULO,
            WS_POPUP,
            0, 0, 0, 0,
            None, None, Some(instancia.into()), None,
        )?;
        let menus = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASSE,
            w!("barra-menus"),
            WS_POPUP,
            0, 0, 0, 0,
            None, None, Some(instancia.into()), None,
        )?;

        BARRA.store(barra.0 as isize, Ordering::Relaxed);
        let mut grupos = grupos::Grupos::new();
        grupos.sincronizar(&janelas::listar());
        let esconder = sistema::ler(sistema::PREF_ESCONDER) == Some(1);
        ESTADO.with_borrow_mut(|e| {
            *e = Some(Estado {
                barra,
                menus,
                gfx: desenho::Gfx::new().expect("Direct2D"),
                grupos,
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
    let mut gfx = desenho::Gfx::new()?;
    let icone = unsafe { LoadIconW(None, IDI_APPLICATION)? };
    let janela = |t: &str, ativa: bool, minimizada: bool, piscando: bool| ItemJanela {
        hwnd: t.len() as isize,
        titulo: t.into(),
        icone,
        ativa,
        minimizada,
        piscando,
    };
    let grupo = |n: &str, q: usize, ativo: bool| ItemGrupo { nome: n.into(), qtd: q, ativo };
    let cena = Cena {
        w,
        h,
        escala: 1.0,
        grupos: vec![grupo("1", 3, false), grupo("projeto-a", 4, true), grupo("claude", 6, false), grupo("geral", 2, false)],
        janelas: vec![
            janela("pwsh — projeto-a\\build", true, false, false),
            janela("pwsh — testes", false, false, false),
            janela("GitHub — snap-zones e mais um título comprido", false, false, true),
            janela("Docs", false, true, false),
        ],
        hover: Some(Alvo::Grupo(2)),
        hora: "14:32".into(),
        data: "21/10/2026".into(),
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
