//! Ícones da bandeja dos outros programas e progresso/selo dos botões, pelo mesmo truque do
//! ManagedShell (base do RetroBar):
//!
//! - `Shell_NotifyIcon` manda WM_COPYDATA para a primeira janela da classe `Shell_TrayWnd`
//!   que o Windows encontrar. A barra cria uma janela invisível com essa classe e a mantém na
//!   frente da do Explorer; tudo o que chega é anotado e repassado ao Explorer, que continua
//!   recebendo os mesmos avisos (notificações em balão, AppBars).
//! - `ITaskbarList3` (progresso e selo) procura a propriedade `TaskbandHWND` nessa mesma
//!   janela e manda as mensagens para a janela que estiver ali.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::core::{w, GUID, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASSE_BANDEJA: PCWSTR = w!("Shell_TrayWnd");
const CLASSE_NOTIFY: PCWSTR = w!("TrayNotifyWnd");
const CLASSE_TASKBAND: PCWSTR = w!("BarraTaskband");
const TIMER_FRENTE: usize = 1;

// Shell_NotifyIcon
const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const NIF_STATE: u32 = 0x8;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x1;
const NIN_SELECT: u32 = WM_USER;

// ITaskbarList3
const TB_PROGRESSO: u32 = WM_USER + 64;
const TB_ESTADO: u32 = WM_USER + 65;
const TB_SELO: u32 = WM_USER + 79;

/// NOTIFYICONDATAW como chega no WM_COPYDATA: handles com 32 bits, para servir a processos
/// de 32 e 64 bits.
#[repr(C)]
#[derive(Clone, Copy)]
struct Nid32 {
    cb_size: u32,
    hwnd: u32,
    id: u32,
    flags: u32,
    callback: u32,
    icone: u32,
    dica: [u16; 128],
    estado: u32,
    mascara: u32,
    info: [u16; 256],
    versao: u32,
    titulo_info: [u16; 64],
    info_flags: u32,
    guid: GUID,
    icone_balao: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DadosBandeja {
    assinatura: u32,
    mensagem: u32,
    nid: Nid32,
}

/// Tamanho da NOTIFYICONDATAW antiga (dica de 64 caracteres, sem estado).
const NID_V1: u32 = 24 + 64 * 2;
/// Tamanho até o guid (Windows XP em diante).
const NID_COM_GUID: u32 = std::mem::offset_of!(Nid32, guid) as u32 + 16;

pub struct Icone {
    pub dono: isize,
    pub id: u32,
    pub guid: Option<GUID>,
    pub callback: u32,
    /// Cópia nossa do ícone (o programa pode destruir o dele).
    pub icone: HICON,
    pub dica: String,
    pub versao: u32,
    pub oculto: bool,
}

impl Drop for Icone {
    fn drop(&mut self) {
        if !self.icone.is_invalid() {
            unsafe { _ = DestroyIcon(self.icone) };
        }
    }
}

#[derive(Default, Clone, Copy)]
pub struct Progresso {
    /// TBPF_*: 0 nada, 1 indeterminado, 2 normal, 4 erro, 8 pausado
    pub estado: u32,
    /// 0..1
    pub fracao: f32,
    pub selo: HICON,
}

#[derive(Default)]
struct Bandeja {
    janela: HWND,
    notify: HWND,
    taskband: HWND,
    /// Janela da barra, avisada para redesenhar.
    barra: HWND,
    icones: Vec<Icone>,
    progresso: HashMap<isize, Progresso>,
}

thread_local! {
    // separado do estado da barra: o WM_COPYDATA chega por SendMessage de outros processos,
    // inclusive no meio de operações da barra
    static BANDEJA: RefCell<Bandeja> = RefCell::new(Bandeja::default());
}

fn texto(v: &[u16]) -> String {
    let n = v.iter().position(|&c| c == 0).unwrap_or(v.len());
    String::from_utf16_lossy(&v[..n])
}

fn proprio(h: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid == unsafe { GetCurrentProcessId() }
}

/// `Shell_TrayWnd` do Explorer (a primeira que não é a nossa).
pub fn barra_do_explorer() -> Option<HWND> {
    let mut depois: Option<HWND> = None;
    unsafe {
        while let Ok(h) = FindWindowExW(None, depois, CLASSE_BANDEJA, PCWSTR::null()) {
            if h.is_invalid() {
                return None;
            }
            if !proprio(h) {
                return Some(h);
            }
            depois = Some(h);
        }
    }
    None
}

fn redesenhar_barra() {
    BANDEJA.with_borrow(|b| unsafe { _ = InvalidateRect(Some(b.barra), None, false) });
}

pub fn ligada() -> bool {
    BANDEJA.with_borrow(|b| !b.janela.is_invalid())
}

/// Cria a `Shell_TrayWnd` própria e pede a todos os programas que mandem os ícones de novo.
pub fn ligar(barra: HWND) {
    if ligada() {
        return;
    }
    unsafe {
        let inst = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
        let classes: [(PCWSTR, WNDPROC); 3] = [
            (CLASSE_BANDEJA, Some(proc_bandeja)),
            (CLASSE_NOTIFY, Some(proc_padrao)),
            (CLASSE_TASKBAND, Some(proc_taskband)),
        ];
        for (classe, proc_) in classes {
            let wc = WNDCLASSW { lpfnWndProc: proc_, hInstance: inst.into(), lpszClassName: classe, ..Default::default() };
            RegisterClassW(&wc);
        }
        let Ok(janela) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASSE_BANDEJA,
            PCWSTR::null(),
            WS_POPUP | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0, 0, 0, 0,
            None, None, Some(inst.into()), None,
        ) else {
            return;
        };
        let notify = CreateWindowExW(WINDOW_EX_STYLE(0), CLASSE_NOTIFY, PCWSTR::null(), WS_CHILD, 0, 0, 0, 0, Some(janela), None, Some(inst.into()), None)
            .unwrap_or_default();
        let taskband = CreateWindowExW(WINDOW_EX_STYLE(0), CLASSE_TASKBAND, PCWSTR::null(), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None)
            .unwrap_or_default();
        _ = SetPropW(janela, w!("TaskbandHWND"), Some(HANDLE(taskband.0)));
        // a barra roda elevada: sem isso o UIPI barra o que vem de programas sem elevação
        // (Steam, Discord...) e o ícone deles não chega nem aqui nem no Explorer
        _ = ChangeWindowMessageFilterEx(janela, WM_COPYDATA, MSGFLT_ALLOW, None);
        for m in [TB_PROGRESSO, TB_ESTADO, TB_SELO] {
            _ = ChangeWindowMessageFilterEx(taskband, m, MSGFLT_ALLOW, None);
        }
        BANDEJA.with_borrow_mut(|b| {
            b.janela = janela;
            b.notify = notify;
            b.taskband = taskband;
            b.barra = barra;
        });
        ficar_na_frente(janela);
        SetTimer(Some(janela), TIMER_FRENTE, 100, None);
        avisar_barra_criada();
    }
}

/// Devolve os ícones ao Explorer.
pub fn desligar() {
    let (janela, taskband) = BANDEJA.with_borrow_mut(|b| {
        b.icones.clear();
        b.progresso.clear();
        let r = (b.janela, b.taskband);
        b.janela = HWND::default();
        b.taskband = HWND::default();
        r
    });
    if janela.is_invalid() {
        return;
    }
    unsafe {
        _ = RemovePropW(janela, w!("TaskbandHWND"));
        _ = DestroyWindow(janela);
        _ = DestroyWindow(taskband);
    }
    avisar_barra_criada();
}

fn avisar_barra_criada() {
    unsafe {
        let msg = RegisterWindowMessageW(w!("TaskbarCreated"));
        _ = SendNotifyMessageW(HWND_BROADCAST, msg, WPARAM(0), LPARAM(0));
    }
}

/// A nossa `Shell_TrayWnd` precisa ser a primeira que o Windows acha.
fn ficar_na_frente(janela: HWND) {
    unsafe {
        if let Some(exp) = barra_do_explorer() {
            _ = SetWindowPos(exp, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        _ = SetWindowPos(janela, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Retângulos que outros programas leem para posicionar janelinhas perto da bandeja.
pub fn posicionar(barra: RECT, area_icones: RECT) {
    BANDEJA.with_borrow(|b| unsafe {
        if b.janela.is_invalid() {
            return;
        }
        let f = SWP_NOACTIVATE | SWP_NOZORDER;
        _ = SetWindowPos(b.janela, None, barra.left, barra.top, barra.right - barra.left, barra.bottom - barra.top, f);
        _ = SetWindowPos(
            b.notify,
            None,
            area_icones.left - barra.left,
            area_icones.top - barra.top,
            area_icones.right - area_icones.left,
            area_icones.bottom - area_icones.top,
            f,
        );
    });
}

/// Repassa ao Explorer o que chegou na nossa `Shell_TrayWnd`.
unsafe fn repassar(msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(exp) = barra_do_explorer() else { return LRESULT(0) };
    if msg == WM_USER + 372 {
        _ = PostMessageW(Some(exp), msg, wp, lp);
        return LRESULT(0);
    }
    let mut r = 0usize;
    _ = SendMessageTimeoutW(exp, msg, wp, lp, SMTO_ABORTIFHUNG, 2000, Some(&mut r));
    LRESULT(r as isize)
}

fn mesmo(i: &Icone, nid: &Nid32, guid: Option<GUID>) -> bool {
    match (guid, i.guid) {
        (Some(g), Some(ig)) => g == ig,
        (Some(_), None) | (None, Some(_)) => false,
        (None, None) => i.dono == nid.hwnd as isize && i.id == nid.id,
    }
}

fn aplicar(i: &mut Icone, nid: &Nid32, tam: u32) {
    if nid.flags & NIF_MESSAGE != 0 {
        i.callback = nid.callback;
    }
    if nid.flags & NIF_ICON != 0 {
        let novo = unsafe { CopyIcon(HICON(nid.icone as isize as *mut _)) }.unwrap_or_default();
        if !i.icone.is_invalid() {
            unsafe { _ = DestroyIcon(i.icone) };
        }
        i.icone = novo;
    }
    if nid.flags & NIF_TIP != 0 {
        let n = if tam <= NID_V1 { 64 } else { 128 };
        i.dica = texto(&nid.dica[..n]);
    }
    if nid.flags & NIF_STATE != 0 && tam > NID_V1 && nid.mascara & NIS_HIDDEN != 0 {
        i.oculto = nid.estado & NIS_HIDDEN != 0;
    }
}

/// Trata um Shell_NotifyIcon. Devolve se deu certo.
fn notificacao(d: &DadosBandeja, tam_nid: u32) -> bool {
    let nid = &d.nid;
    let guid = (nid.flags & NIF_GUID != 0 && tam_nid >= NID_COM_GUID).then_some(nid.guid);
    if std::env::var_os("BARRA_LOG").is_some() {
        // diagnóstico: cada Shell_NotifyIcon que chega, com o programa dono
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(HWND(nid.hwnd as isize as *mut _), Some(&mut pid)) };
        let linha = format!("msg={} pid={} id={} flags={:#x} guid={:?} tam={} dica={}
", d.mensagem, pid, nid.id, nid.flags, guid.is_some(), tam_nid, texto(&nid.dica));
        let caminho = std::env::temp_dir().join("barra-bandeja.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(caminho) {
            use std::io::Write;
            _ = f.write_all(linha.as_bytes());
        }
    }
    BANDEJA.with_borrow_mut(|b| {
        let pos = b.icones.iter().position(|i| mesmo(i, nid, guid));
        match (d.mensagem, pos) {
            (NIM_ADD, None) => {
                let mut i = Icone {
                    dono: nid.hwnd as isize,
                    id: nid.id,
                    guid,
                    callback: 0,
                    icone: HICON::default(),
                    dica: String::new(),
                    versao: 0,
                    oculto: false,
                };
                aplicar(&mut i, nid, tam_nid);
                b.icones.push(i);
                true
            }
            (NIM_ADD | NIM_MODIFY, Some(p)) => {
                if nid.hwnd != 0 {
                    b.icones[p].dono = nid.hwnd as isize;
                }
                aplicar(&mut b.icones[p], nid, tam_nid);
                true
            }
            (NIM_DELETE, Some(p)) => {
                b.icones.remove(p);
                true
            }
            (NIM_SETVERSION, Some(p)) => {
                b.icones[p].versao = nid.versao;
                true
            }
            _ => false,
        }
    })
}

unsafe extern "system" fn proc_bandeja(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COPYDATA => {
            let cds = &*(lp.0 as *const COPYDATASTRUCT);
            if cds.dwData == 1 && cds.cbData as usize >= 8 + NID_V1 as usize && !cds.lpData.is_null() {
                // cópia com zeros no que o programa não mandou (estruturas antigas são menores)
                let mut d: DadosBandeja = std::mem::zeroed();
                let n = (cds.cbData as usize).min(size_of::<DadosBandeja>());
                std::ptr::copy_nonoverlapping(cds.lpData as *const u8, &mut d as *mut _ as *mut u8, n);
                let tam_nid = d.nid.cb_size.min(n as u32 - 8);
                let ok = notificacao(&d, tam_nid);
                repassar(msg, wp, lp);
                redesenhar_barra();
                return LRESULT(ok as isize);
            }
            repassar(msg, wp, lp)
        }
        WM_TIMER if wp.0 == TIMER_FRENTE => {
            // Explorer reiniciado ou outra barra: volta para a frente
            if FindWindowW(CLASSE_BANDEJA, PCWSTR::null()).ok() != Some(h) {
                ficar_na_frente(h);
            }
            // ícones de programas que fecharam sem avisar
            let mudou = BANDEJA.with_borrow_mut(|b| {
                let antes = b.icones.len();
                b.icones.retain(|i| IsWindow(Some(HWND(i.dono as *mut _))).as_bool());
                b.progresso.retain(|&j, _| IsWindow(Some(HWND(j as *mut _))).as_bool());
                antes != b.icones.len()
            });
            if mudou {
                redesenhar_barra();
            }
            LRESULT(0)
        }
        WM_WINDOWPOSCHANGED => {
            // fica sempre invisível
            let estilo = GetWindowLongW(h, GWL_STYLE) as u32;
            if estilo & WS_VISIBLE.0 != 0 {
                SetWindowLongW(h, GWL_STYLE, (estilo & !WS_VISIBLE.0) as i32);
            }
            DefWindowProcW(h, msg, wp, lp)
        }
        WM_COMMAND | WM_ACTIVATEAPP => repassar(msg, wp, lp),
        m if m >= WM_USER => repassar(msg, wp, lp),
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

unsafe extern "system" fn proc_taskband(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let j = wp.0 as isize;
    let mudou = match msg {
        TB_PROGRESSO => BANDEJA.with_borrow_mut(|b| {
            b.progresso.entry(j).or_default().fracao = (lp.0 as f32 / 65534.0).clamp(0.0, 1.0);
            true
        }),
        TB_ESTADO => BANDEJA.with_borrow_mut(|b| {
            let p = b.progresso.entry(j).or_default();
            p.estado = lp.0 as u32;
            if p.estado == 0 {
                p.fracao = 0.0;
            }
            true
        }),
        TB_SELO => BANDEJA.with_borrow_mut(|b| {
            let p = b.progresso.entry(j).or_default();
            if !p.selo.is_invalid() {
                _ = DestroyIcon(p.selo);
            }
            p.selo = if lp.0 == 0 { HICON::default() } else { CopyIcon(HICON(lp.0 as *mut _)).unwrap_or_default() };
            true
        }),
        m if m >= WM_USER => {
            return LRESULT(0);
        }
        _ => return DefWindowProcW(h, msg, wp, lp),
    };
    if mudou {
        redesenhar_barra();
    }
    LRESULT(0)
}

/// Ícones visíveis, na ordem em que chegaram: (índice, ícone, dica).
/// (índice, ícone, dica, janela dona).
pub fn visiveis() -> Vec<(usize, HICON, String, isize)> {
    BANDEJA.with_borrow(|b| {
        b.icones
            .iter()
            .enumerate()
            .filter(|(_, i)| !i.oculto && !i.icone.is_invalid())
            .map(|(n, i)| (n, i.icone, i.dica.clone(), i.dono))
            .collect()
    })
}

pub fn progresso(j: isize) -> Progresso {
    BANDEJA.with_borrow(|b| b.progresso.get(&j).copied().unwrap_or_default())
}

#[derive(Clone, Copy)]
pub enum Botao {
    Esquerdo,
    Direito,
    Duplo,
}

fn mk(baixo: u32, alto: u32) -> usize {
    ((alto & 0xFFFF) << 16 | (baixo & 0xFFFF)) as usize
}

/// Repassa o clique ao programa dono do ícone, do jeito que a barra do Windows faz.
pub fn clicar(indice: usize, botao: Botao, pt: POINT) {
    let Some((dono, id, callback, versao)) = BANDEJA.with_borrow(|b| b.icones.get(indice).map(|i| (i.dono, i.id, i.callback, i.versao)))
    else {
        return;
    };
    if callback == 0 {
        return;
    }
    let dono = HWND(dono as *mut _);
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(dono, Some(&mut pid));
        // o programa precisa poder vir para a frente para abrir o menu dele
        _ = AllowSetForegroundWindow(pid);
        let eventos: &[u32] = match botao {
            Botao::Esquerdo => &[WM_LBUTTONDOWN, WM_LBUTTONUP],
            Botao::Direito => &[WM_RBUTTONDOWN, WM_RBUTTONUP],
            Botao::Duplo => &[WM_LBUTTONDBLCLK],
        };
        for &e in eventos {
            let (wp, lp) = if versao >= 4 { (mk(pt.x as u32, pt.y as u32), mk(e, id)) } else { (id as usize, e as usize) };
            _ = PostMessageW(Some(dono), callback, WPARAM(wp), LPARAM(lp as isize));
        }
        if versao >= 4 {
            let fim = match botao {
                Botao::Esquerdo => Some(NIN_SELECT),
                Botao::Direito => Some(WM_CONTEXTMENU),
                Botao::Duplo => None,
            };
            if let Some(e) = fim {
                _ = PostMessageW(Some(dono), callback, WPARAM(mk(pt.x as u32, pt.y as u32)), LPARAM(mk(e, id) as isize));
            }
        }
    }
}

unsafe extern "system" fn proc_padrao(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(h, msg, wp, lp)
}
