//! Menus Win32 dos outros programas (bandeja, clique direito, menu de sistema, barra de menus
//! clássica), botões da barra de título e controles no tema. A barra instala ganchos globais
//! com a barra_gancho.dll; o Windows a carrega em cada programa com janela, e ela carrega lá a
//! barra_menus.dll, que faz o resto (ver gancho/src/lib.rs e menus/src/lib.rs).
//!
//! As DLLs ficam presas nos programas até eles fecharem, então a barra usa cópias em
//! %LOCALAPPDATA%\barra, com o hash do conteúdo no nome: o arquivo do build nunca fica travado,
//! e a mesma versão tem sempre o mesmo caminho. O da barra_gancho.dll ocupa uma posição na
//! tabela de DLLs de gancho do Windows, que tem 32 por sessão; como ela quase nunca muda, é a
//! mesma posição a cada versão da barra.

use std::cell::Cell;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::core::{s, w, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{FreeLibrary, LocalFree, HLOCAL, HMODULE, LPARAM, LRESULT, WPARAM};
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::{ACL, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SUB_CONTAINERS_AND_OBJECTS_INHERIT};
use windows::Win32::Storage::FileSystem::{FILE_GENERIC_EXECUTE, FILE_GENERIC_READ};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::tema::Tema;

const PADRAO: u32 = 0;
const ESCURO: u32 = 2;
const CLARO: u32 = 3;

type DefinirTema = unsafe extern "system" fn(u32, *const u32);
/// O `definir_tema` das primeiras versões da barra_menus.dll.
type DefinirModo = unsafe extern "system" fn(u32);
type DefinirImplementacao = unsafe extern "system" fn(*const u16);
type Gancho = unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT;

#[derive(Clone, Copy)]
struct Dlls {
    /// A barra_gancho.dll, dos ganchos.
    gancho: HMODULE,
    /// `definir_tema` da barra_menus.dll.
    definir: DefinirTema,
}

thread_local! {
    /// None: ainda não carregadas; Some(None): não deu.
    static DLLS: Cell<Option<Option<Dlls>>> = const { Cell::new(None) };
    static GANCHOS: Cell<Option<[HHOOK; 3]>> = const { Cell::new(None) };
    /// Tema já passado para a DLL ("": nenhum, ou o padrão do Windows).
    static ATUAL: Cell<&'static str> = const { Cell::new("") };
    /// Última tentativa de instalar os ganchos, se falhou, e o erro do Windows.
    static FALHA: Cell<Option<(Instant, u32)>> = const { Cell::new(None) };
}

/// Espera entre as tentativas de instalar os ganchos depois de uma falha.
const ESPERA: Duration = Duration::from_secs(30);

/// `nome` dentro de %LOCALAPPDATA%\barra.
fn local(nome: &str) -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("barra").join(nome))
}

fn largo(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain([0]).collect()
}

/// FNV-1a de 64 bits.
fn hash(dados: &[u8]) -> u64 {
    dados.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// Copia `arquivo`, que fica ao lado do barra.exe, para a pasta `destino`, com o hash do
/// conteúdo no nome.
fn copiar(arquivo: &str, destino: &Path) -> Option<PathBuf> {
    let dados = std::fs::read(std::env::current_exe().ok()?.with_file_name(arquivo)).ok()?;
    let nome = Path::new(arquivo).file_stem()?.to_str()?;
    std::fs::create_dir_all(destino).ok()?;
    let copia = destino.join(format!("{nome}-{:016x}.dll", hash(&dados)));
    if !copia.exists() {
        // outro nome até terminar: nenhum programa carrega a cópia pela metade
        let parcial = copia.with_extension("parcial");
        std::fs::write(&parcial, &dados).ok()?;
        std::fs::rename(&parcial, &copia).ok()?;
    }
    Some(copia)
}

/// Apaga as cópias antigas que nenhum programa segura mais. `desligar`: as que ainda estão em
/// algum programa recebem o padrão do Windows. Se a barra que as usava caiu sem desligá-las,
/// elas continuariam pintando os programas com o tema daquela hora.
fn limpar(pasta: &Path, atual: &Path, desligar: bool) {
    let Ok(lista) = std::fs::read_dir(pasta) else { return };
    for e in lista.flatten() {
        let p = e.path();
        if p == atual || std::fs::remove_file(&p).is_ok() {
            continue;
        }
        if desligar && p.extension().is_some_and(|x| x == "dll") {
            desligar_copia(&p);
        }
    }
}

/// O modo e as cores ficam na memória comum da cópia, que a barra alcança carregando o mesmo
/// arquivo.
fn desligar_copia(caminho: &Path) {
    unsafe {
        let Ok(m) = LoadLibraryW(PCWSTR(largo(caminho).as_ptr())) else { return };
        if let Some(f) = GetProcAddress(m, s!("definir_tema")) {
            std::mem::transmute::<_, DefinirTema>(f)(PADRAO, std::ptr::null());
        } else if let Some(f) = GetProcAddress(m, s!("definir_modo")) {
            std::mem::transmute::<_, DefinirModo>(f)(PADRAO);
        }
        _ = FreeLibrary(m);
    }
}

/// Apps da Loja rodam em AppContainer e só carregam DLL de pasta com leitura para "ALL
/// APPLICATION PACKAGES" (S-1-15-2-1) e "ALL RESTRICTED APPLICATION PACKAGES" (S-1-15-2-2).
/// A permissão vai na pasta, herdada pelas cópias.
fn liberar_para_apps(pasta: &Path) {
    let nome = largo(pasta);
    let caminho = PCWSTR(nome.as_ptr());
    unsafe {
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        if GetNamedSecurityInfoW(caminho, SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, None, None, Some(&mut dacl), None, &mut sd).is_err() {
            return;
        }
        let mut sids = Vec::new();
        for texto in [w!("S-1-15-2-1"), w!("S-1-15-2-2")] {
            let mut sid = PSID::default();
            if ConvertStringSidToSidW(texto, &mut sid).is_ok() {
                sids.push(sid);
            }
        }
        let entradas: Vec<EXPLICIT_ACCESS_W> = sids
            .iter()
            .map(|sid| EXPLICIT_ACCESS_W {
                grfAccessPermissions: (FILE_GENERIC_READ | FILE_GENERIC_EXECUTE).0,
                grfAccessMode: GRANT_ACCESS,
                grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
                Trustee: TRUSTEE_W { TrusteeForm: TRUSTEE_IS_SID, TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP, ptstrName: PWSTR(sid.0 as *mut u16), ..Default::default() },
            })
            .collect();
        let mut nova: *mut ACL = std::ptr::null_mut();
        if SetEntriesInAclW(Some(&entradas), Some(dacl), &mut nova).is_ok() {
            _ = SetNamedSecurityInfoW(caminho, SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, None, None, Some(nova), None);
            _ = LocalFree(Some(HLOCAL(nova as *mut _)));
        }
        for sid in sids {
            _ = LocalFree(Some(HLOCAL(sid.0)));
        }
        _ = LocalFree(Some(HLOCAL(sd.0)));
    }
}

fn carregar() -> Option<Dlls> {
    let (pasta_menus, pasta_gancho) = (local("menus")?, local("gancho")?);
    let menus = copiar("barra_menus.dll", &pasta_menus)?;
    let gancho = copiar("barra_gancho.dll", &pasta_gancho)?;
    liberar_para_apps(&pasta_menus);
    liberar_para_apps(&pasta_gancho);
    limpar(&pasta_menus, &menus, true);
    limpar(&pasta_gancho, &gancho, false);
    unsafe {
        let m = LoadLibraryW(PCWSTR(largo(&menus).as_ptr())).ok()?;
        let g = LoadLibraryW(PCWSTR(largo(&gancho).as_ptr())).ok()?;
        let definir: DefinirTema = std::mem::transmute(GetProcAddress(m, s!("definir_tema"))?);
        let apontar: DefinirImplementacao = std::mem::transmute(GetProcAddress(g, s!("definir_implementacao"))?);
        apontar(largo(&menus).as_ptr());
        Some(Dlls { gancho: g, definir })
    }
}

fn dlls() -> Option<Dlls> {
    if let Some(d) = DLLS.get() {
        return d;
    }
    let d = carregar();
    if d.is_none() {
        registrar("ganchos globais: barra_menus.dll ou barra_gancho.dll não carregou");
    }
    DLLS.set(Some(d));
    d
}

/// Uma linha com data e hora em %LOCALAPPDATA%\barra\barra.log.
fn registrar(texto: &str) {
    let Some(caminho) = local("barra.log") else { return };
    let t = unsafe { GetLocalTime() };
    let linha = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} {texto}\r\n", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(caminho) {
        use std::io::Write;
        _ = f.write_all(linha.as_bytes());
    }
}

/// Os três ganchos da barra_gancho.dll; se um falhar, nenhum fica. Erro: o código do Windows.
fn instalar(m: HMODULE) -> Result<[HHOOK; 3], u32> {
    let um = |tipo: WINDOWS_HOOK_ID, nome: PCSTR| unsafe {
        let f: Gancho = std::mem::transmute(GetProcAddress(m, nome).ok_or(127u32)?);
        SetWindowsHookExW(tipo, Some(f), Some(m.into()), 0).map_err(|e| (e.code().0 & 0xFFFF) as u32)
    };
    match (um(WH_GETMESSAGE, s!("gancho")), um(WH_CALLWNDPROCRET, s!("gancho_retorno")), um(WH_CBT, s!("gancho_cbt"))) {
        (Ok(a), Ok(b), Ok(c)) => Ok([a, b, c]),
        (a, b, c) => {
            let erro = [&a, &b, &c].into_iter().find_map(|r| r.as_ref().err().copied()).unwrap_or(0);
            for h in [a, b, c].into_iter().flatten() {
                unsafe { _ = UnhookWindowsHookEx(h) };
            }
            Err(erro)
        }
    }
}

/// Cada programa aplica o tema novo na próxima mensagem que tirar da fila; uma mensagem
/// vazia para todas as janelas faz isso acontecer agora.
fn avisar_todos() {
    unsafe { _ = PostMessageW(Some(HWND_BROADCAST), WM_NULL, WPARAM(0), LPARAM(0)) };
}

/// Cores na ordem que a DLL espera: fundo, fundo_claro, selecao, mudo, texto, texto_forte,
/// destaque, apagado (texto misturado 30% com o fundo, como na barra), vermelho.
fn cores(t: &Tema) -> [u32; 9] {
    let mistura = |a: u32, b: u32, f: f32| {
        let canal = |s: u32| {
            let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
            ((x + (y - x) * f).round() as u32) << s
        };
        canal(16) | canal(8) | canal(0)
    };
    [t.fundo, t.fundo_claro, t.selecao, t.mudo, t.texto, t.texto_forte, t.destaque, mistura(t.texto, t.fundo, 0.3), t.vermelho]
}

/// Liga ou troca o tema. Chamado a cada atualização da barra; só age quando algo mudou. Se o
/// Windows recusar os ganchos, tenta de novo a cada `ESPERA` e registra o erro no barra.log.
pub fn ligar(tema: &'static Tema) {
    let Some(d) = dlls() else { return };
    let mut avisar = false;
    if ATUAL.get() != tema.nome {
        let c = cores(tema);
        unsafe { (d.definir)(if tema.claro { CLARO } else { ESCURO }, c.as_ptr()) };
        ATUAL.set(tema.nome);
        avisar = true;
    }
    if GANCHOS.get().is_none() && FALHA.get().is_none_or(|(t, _)| t.elapsed() >= ESPERA) {
        match instalar(d.gancho) {
            Ok(g) => {
                if FALHA.take().is_some() {
                    registrar("ganchos globais: instalados");
                }
                GANCHOS.set(Some(g));
                avisar = true;
            }
            Err(erro) => {
                if FALHA.get().is_none_or(|(_, e)| e != erro) {
                    registrar(&format!("ganchos globais: SetWindowsHookEx falhou com o erro {erro}"));
                }
                FALHA.set(Some((Instant::now(), erro)));
            }
        }
    }
    if avisar && GANCHOS.get().is_some() {
        avisar_todos();
    }
}

/// Devolve o padrão do Windows a todos os programas e tira os ganchos. As DLLs continuam nos
/// programas que já as usaram, só passando as mensagens adiante.
pub fn desligar() {
    let ganchos = GANCHOS.take();
    if ATUAL.get().is_empty() && ganchos.is_none() {
        return;
    }
    ATUAL.set("");
    FALHA.set(None);
    if let Some(Some(d)) = DLLS.get() {
        unsafe { (d.definir)(PADRAO, std::ptr::null()) };
        if ganchos.is_some() {
            avisar_todos();
            // tempo para os programas tirarem a mensagem da fila antes de os ganchos saírem
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    for h in ganchos.into_iter().flatten() {
        unsafe { _ = UnhookWindowsHookEx(h) };
    }
}
