//! Molduras das outras janelas no tema, como no Hyprland: barra de título na cor de fundo,
//! borda de destaque na janela em foco e apagada nas outras, cantos retos. Tudo pelo DWM
//! (DwmSetWindowAttribute vale para janela de outro processo), sem injetar nada.
//!
//! À parte, a barra de título sem os botões de minimizar, maximizar e fechar (no Hyprland não
//! há botões): sai o WS_SYSMENU, e o DWM deixa de desenhar os botões e o ícone. O título fica
//! para arrastar; duplo clique continua maximizando (WS_MAXIMIZEBOX fica) e Alt+F4 fechando.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Dwm::*;

use crate::janelas::{existe, hwnd};
use crate::sistema;
use crate::tema::Tema;

/// Preferência: molduras das janelas no tema (padrão: sim).
pub const PREF: PCWSTR = w!("Molduras");
/// Preferência: barras de título sem os botões (padrão: sim).
pub const PREF_BOTOES: PCWSTR = w!("OcultarBotoes");
/// Volta a cor ao padrão do Windows (DWMWA_COLOR_DEFAULT).
const COR_PADRAO: u32 = 0xFFFF_FFFF;

/// O que já foi aplicado numa janela, para não repetir a cada segundo.
struct Aplicada {
    tema: &'static str,
    frente: bool,
    /// Modo escuro que a janela tinha antes, para devolver na saída.
    escuro_antes: u32,
    /// Terminal: ícones (grande, pequeno) de antes de trocar pelo glifo do tema.
    icone_antes: Option<(isize, isize)>,
}

struct Molduras {
    ligadas: bool,
    janelas: HashMap<isize, Aplicada>,
    sem_botoes: bool,
    /// Janelas de onde a barra tirou o WS_SYSMENU, para devolver.
    sem_menu: HashSet<isize>,
}

thread_local! {
    static M: RefCell<Molduras> = RefCell::new(Molduras {
        ligadas: sistema::ler(PREF) != Some(0),
        janelas: HashMap::new(),
        sem_botoes: sistema::ler(PREF_BOTOES) != Some(0),
        sem_menu: HashSet::new(),
    });
}

/// 0xRRGGBB do tema para COLORREF (0x00BBGGRR).
fn colorref(c: u32) -> u32 {
    (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16) & 0xFF
}

fn definir(h: HWND, atributo: DWMWINDOWATTRIBUTE, v: u32) {
    unsafe { _ = DwmSetWindowAttribute(h, atributo, &v as *const _ as *const c_void, 4) };
}

fn ler_escuro(h: HWND) -> u32 {
    let mut v = 0u32;
    unsafe { _ = DwmGetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE, &mut v as *mut _ as *mut c_void, 4) };
    v
}

fn pintar(h: HWND, tema: &Tema, frente: bool) {
    definir(h, DWMWA_USE_IMMERSIVE_DARK_MODE, !tema.claro as u32);
    definir(h, DWMWA_CAPTION_COLOR, colorref(tema.fundo));
    definir(h, DWMWA_TEXT_COLOR, colorref(tema.texto));
    definir(h, DWMWA_BORDER_COLOR, colorref(if frente { tema.destaque } else { tema.mudo }));
    definir(h, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
}

/// Troca o ícone da barra de título (WM_SETICON vale para janela de outro processo) e faz a
/// janela redesenhar o título; sem o WM_THEMECHANGED o console não redesenha o ícone.
/// Devolve os ícones de antes.
fn trocar_icone(h: HWND, grande: isize, pequeno: isize) -> (isize, isize) {
    unsafe {
        let g = SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(grande))).0;
        let p = SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(pequeno))).0;
        SendMessageW(h, WM_THEMECHANGED, None, None);
        _ = SetWindowPos(h, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        (g, p)
    }
}

/// Barra de título desenhada pelo Windows: estilo de título e a área do programa começando
/// abaixo dela. Navegadores, Windows Terminal, Explorer e apps da loja estendem a área do
/// programa até o alto e desenham os próprios botões; ficam de fora.
fn titulo_do_windows(h: HWND, estilo: isize) -> bool {
    let titulo = WS_CAPTION.0 as isize;
    if estilo & titulo != titulo || unsafe { IsIconic(h) }.as_bool() {
        return false;
    }
    let mut r = RECT::default();
    let mut cliente = POINT::default();
    unsafe {
        if GetWindowRect(h, &mut r).is_err() || !ClientToScreen(h, &mut cliente).as_bool() {
            return false;
        }
        cliente.y - r.top >= GetSystemMetricsForDpi(SM_CYCAPTION, GetDpiForWindow(h))
    }
}

/// Liga ou desliga o WS_SYSMENU e faz o DWM redesenhar a moldura. Programa travado fica de
/// fora (a troca de estilo espera a janela); falha em programa que roda como administrador.
fn menu_de_sistema(h: HWND, sim: bool) -> bool {
    unsafe {
        if IsHungAppWindow(h).as_bool() {
            return false;
        }
        let estilo = GetWindowLongPtrW(h, GWL_STYLE);
        let menu = WS_SYSMENU.0 as isize;
        let novo = if sim { estilo | menu } else { estilo & !menu };
        if novo == estilo {
            return true;
        }
        SetWindowLongPtrW(h, GWL_STYLE, novo);
        if GetWindowLongPtrW(h, GWL_STYLE) != novo {
            return false;
        }
        _ = SetWindowPos(h, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_ASYNCWINDOWPOS);
        true
    }
}

fn devolver(h: HWND, a: &Aplicada) {
    definir(h, DWMWA_CAPTION_COLOR, COR_PADRAO);
    definir(h, DWMWA_TEXT_COLOR, COR_PADRAO);
    definir(h, DWMWA_BORDER_COLOR, COR_PADRAO);
    definir(h, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT.0 as u32);
    definir(h, DWMWA_USE_IMMERSIVE_DARK_MODE, a.escuro_antes);
    if let Some((g, p)) = a.icone_antes {
        trocar_icone(h, g, p);
    }
}

pub fn ligadas() -> bool {
    M.with_borrow(|m| m.ligadas)
}

/// Liga ou desliga; desligar devolve as molduras do Windows na hora.
pub fn ligar(sim: bool) {
    M.with_borrow_mut(|m| {
        m.ligadas = sim;
        if !sim {
            devolver_molduras(m);
        }
    });
}

pub fn botoes_ocultos() -> bool {
    M.with_borrow(|m| m.sem_botoes)
}

/// Oculta ou mostra os botões das barras de título; mostrar devolve na hora.
pub fn ocultar_botoes(sim: bool) {
    M.with_borrow_mut(|m| {
        m.sem_botoes = sim;
        if !sim {
            devolver_botoes(m);
        }
    });
}

/// Pinta as janelas novas, troca a borda de quem ganhou ou perdeu o foco e repinta tudo
/// quando o tema muda. Só chama o DWM no que mudou.
/// `icones`: o glifo de terminal do tema, (grande, pequeno), para a barra de título dos terminais.
pub fn sincronizar(tema: &'static Tema, atuais: &[isize], frente: isize, icones: Option<(HICON, HICON)>) {
    M.with_borrow_mut(|m| {
        m.sem_menu.retain(|&j| existe(j));
        if m.sem_botoes {
            // confere a cada passada: o console põe o menu de volta em algumas trocas de modo
            for &j in atuais {
                let estilo = unsafe { GetWindowLongPtrW(hwnd(j), GWL_STYLE) };
                if estilo & WS_SYSMENU.0 as isize != 0 && titulo_do_windows(hwnd(j), estilo) && menu_de_sistema(hwnd(j), false) {
                    m.sem_menu.insert(j);
                }
            }
        }
        if !m.ligadas {
            return;
        }
        // janela fechada sai; a que só sumiu da lista (outra área de trabalho) fica, para
        // voltar ao padrão na saída
        m.janelas.retain(|&j, _| existe(j));
        for &j in atuais {
            let f = j == frente;
            let a = m.janelas.entry(j).or_insert_with(|| Aplicada { tema: "", frente: !f, escuro_antes: ler_escuro(hwnd(j)), icone_antes: None });
            let tema_mudou = a.tema != tema.nome;
            if tema_mudou || a.frente != f {
                pintar(hwnd(j), tema, f);
                a.tema = tema.nome;
                a.frente = f;
            }
            // terminal: ícone da barra de título vira o glifo do tema (troca de novo quando o tema muda)
            if let Some((grande, pequeno)) = icones.filter(|_| crate::janelas::terminal(hwnd(j))) {
                if tema_mudou || a.icone_antes.is_none() {
                    let antes = trocar_icone(hwnd(j), grande.0 as isize, pequeno.0 as isize);
                    a.icone_antes.get_or_insert(antes);
                }
            }
        }
    });
}

fn devolver_molduras(m: &mut Molduras) {
    for (j, a) in m.janelas.drain() {
        if existe(j) {
            devolver(hwnd(j), &a);
        }
    }
}

fn devolver_botoes(m: &mut Molduras) {
    for j in m.sem_menu.drain() {
        if existe(j) {
            menu_de_sistema(hwnd(j), true);
        }
    }
}

/// Saída, desligamento e pânico: molduras e botões do Windows de volta em todas as janelas.
pub fn restaurar() {
    // no pânico o estado pode estar emprestado: aí não dá, e cor e botões ficam para trás
    _ = M.try_with(|m| {
        if let Ok(mut m) = m.try_borrow_mut() {
            devolver_molduras(&mut m);
            devolver_botoes(&mut m);
        }
    });
}
