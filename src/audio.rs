//! Volume pela Core Audio: nível e mudo da saída padrão, lista de saídas (troca da padrão pela
//! IPolicyConfig, interface não documentada que o EarTrumpet e o SoundSwitch também usam) e
//! volume por programa (sessões de áudio da saída padrão).

// os métodos da IPolicyConfig têm os nomes da interface COM
#![allow(non_snake_case)]

use std::ffi::c_void;

use windows::core::{Interface, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, PROPERTYKEY};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::{PropVariantToStringAlloc, PropVariantToUInt32};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

pub const G_VOLUME_ALTO: &str = "\u{f057e}";
pub const G_VOLUME_MEDIO: &str = "\u{f0580}";
pub const G_VOLUME_BAIXO: &str = "\u{f057f}";
pub const G_VOLUME_MUDO: &str = "\u{f0581}";
const G_FONE: &str = "\u{f02cb}";
const G_CAIXA: &str = "\u{f04c3}";
const G_MONITOR: &str = "\u{f0379}";

/// Glifo do volume para o nível (0..1).
pub fn glifo(nivel: f32, mudo: bool) -> &'static str {
    if mudo {
        G_VOLUME_MUDO
    } else if nivel < 0.34 {
        G_VOLUME_BAIXO
    } else if nivel < 0.67 {
        G_VOLUME_MEDIO
    } else {
        G_VOLUME_ALTO
    }
}

#[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    fn GetMixFormat(&self, id: PCWSTR, formato: *mut *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, id: PCWSTR, padrao: i32, formato: *mut *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, id: PCWSTR, a: *mut c_void, b: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, id: PCWSTR, padrao: i32, a: *mut i64, b: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, id: PCWSTR, a: *mut i64) -> HRESULT;
    fn GetShareMode(&self, id: PCWSTR, modo: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, id: PCWSTR, modo: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, id: PCWSTR, chave: *const c_void, valor: *mut c_void) -> HRESULT;
    fn SetPropertyValue(&self, id: PCWSTR, chave: *const c_void, valor: *mut c_void) -> HRESULT;
    fn SetDefaultEndpoint(&self, id: PCWSTR, papel: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, id: PCWSTR, visivel: i32) -> HRESULT;
}

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

pub struct Saida {
    pub id: String,
    pub nome: String,
    pub glifo: &'static str,
    pub padrao: bool,
}

/// Programa tocando som; as sessões do mesmo executável ficam juntas.
pub struct Programa {
    /// Caminho do executável.
    pub exe: String,
    pub nome: String,
    pub nivel: f32,
    pub mudo: bool,
    volumes: Vec<ISimpleAudioVolume>,
}

impl Programa {
    pub fn definir(&self, nivel: f32) {
        for v in &self.volumes {
            unsafe {
                _ = v.SetMasterVolume(nivel.clamp(0.0, 1.0), std::ptr::null());
                _ = v.SetMute(false, std::ptr::null());
            }
        }
    }
}

pub struct Audio {
    en: IMMDeviceEnumerator,
    /// Saída padrão (id, volume), refeita quando o Windows troca a padrão.
    padrao: Option<(String, IAudioEndpointVolume)>,
}

pub(crate) fn texto_com(p: PWSTR) -> String {
    let s = unsafe { p.to_string().unwrap_or_default() };
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s
}

fn propriedade(loja: &IPropertyStore, chave: &PROPERTYKEY) -> Option<String> {
    unsafe {
        let v = loja.GetValue(chave).ok()?;
        PropVariantToStringAlloc(&v).ok().map(texto_com)
    }
}

impl Audio {
    pub fn new() -> Option<Self> {
        let en: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()? };
        let mut a = Self { en, padrao: None };
        a.renovar();
        Some(a)
    }

    /// Confere a saída padrão (o Windows ou outro programa pode ter trocado).
    pub fn renovar(&mut self) {
        let atual = unsafe { self.en.GetDefaultAudioEndpoint(eRender, eMultimedia).ok() };
        let Some(d) = atual else {
            self.padrao = None;
            return;
        };
        let id = unsafe { d.GetId().map(texto_com).unwrap_or_default() };
        if self.padrao.as_ref().is_some_and(|(i, _)| *i == id) {
            return;
        }
        self.padrao = unsafe { d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok() }.map(|v| (id, v));
    }

    /// (nível 0..1, mudo) da saída padrão.
    pub fn nivel(&self) -> Option<(f32, bool)> {
        let (_, v) = self.padrao.as_ref()?;
        unsafe { Some((v.GetMasterVolumeLevelScalar().ok()?, v.GetMute().ok()?.as_bool())) }
    }

    pub fn definir(&self, nivel: f32) {
        if let Some((_, v)) = &self.padrao {
            unsafe {
                _ = v.SetMasterVolumeLevelScalar(nivel.clamp(0.0, 1.0), std::ptr::null());
                _ = v.SetMute(false, std::ptr::null());
            }
        }
    }

    /// Soma `passo` ao nível (roda do mouse).
    pub fn somar(&self, passo: f32) {
        if let Some((n, _)) = self.nivel() {
            self.definir(((n + passo) * 50.0).round() / 50.0);
        }
    }

    pub fn alternar_mudo(&self) {
        if let (Some((_, v)), Some((_, m))) = (&self.padrao, self.nivel()) {
            unsafe { _ = v.SetMute(!m, std::ptr::null()) };
        }
    }

    pub fn saidas(&self) -> Vec<Saida> {
        let padrao = self.padrao.as_ref().map(|(i, _)| i.clone()).unwrap_or_default();
        let mut v = Vec::new();
        unsafe {
            let Ok(lista) = self.en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return v };
            for i in 0..lista.GetCount().unwrap_or(0) {
                let Ok(d) = lista.Item(i) else { continue };
                let id = d.GetId().map(texto_com).unwrap_or_default();
                let Ok(loja) = d.OpenPropertyStore(STGM_READ) else { continue };
                let nome = propriedade(&loja, &PKEY_Device_FriendlyName).unwrap_or_else(|| id.clone());
                let forma = loja.GetValue(&PKEY_AudioEndpoint_FormFactor).ok().and_then(|p| PropVariantToUInt32(&p).ok()).unwrap_or(0);
                let forma = EndpointFormFactor(forma as i32);
                let glifo = if forma == Headphones || forma == Headset {
                    G_FONE
                } else if forma == DigitalAudioDisplayDevice {
                    G_MONITOR
                } else {
                    G_CAIXA
                };
                v.push(Saida { padrao: id == padrao, id, nome, glifo });
            }
        }
        v
    }

    /// Torna `id` a saída padrão nos três papéis, como o painel de som do Windows faz.
    pub fn definir_saida(&mut self, id: &str) {
        let u: Vec<u16> = id.encode_utf16().chain([0]).collect();
        unsafe {
            if let Ok(p) = CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) {
                for papel in [eConsole, eMultimedia, eCommunications] {
                    _ = p.SetDefaultEndpoint(PCWSTR(u.as_ptr()), papel);
                }
            }
        }
        self.renovar();
    }

    pub fn programas(&self) -> Vec<Programa> {
        let mut v: Vec<Programa> = Vec::new();
        unsafe {
            let Ok(d) = self.en.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return v };
            let Ok(gerente) = d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return v };
            let Ok(lista) = gerente.GetSessionEnumerator() else { return v };
            for i in 0..lista.GetCount().unwrap_or(0) {
                let Ok(c) = lista.GetSession(i) else { continue };
                let Ok(c2) = c.cast::<IAudioSessionControl2>() else { continue };
                // sons do sistema e sessões encerradas ficam de fora
                if c2.IsSystemSoundsSession() == HRESULT(0) || c.GetState().is_ok_and(|e| e == AudioSessionStateExpired) {
                    continue;
                }
                let Ok(vol) = c.cast::<ISimpleAudioVolume>() else { continue };
                let Some(exe) = c2.GetProcessId().ok().and_then(caminho_do_processo) else { continue };
                let nivel = vol.GetMasterVolume().unwrap_or(1.0);
                let mudo = vol.GetMute().map(|m| m.as_bool()).unwrap_or(false);
                match v.iter_mut().find(|p| p.exe.eq_ignore_ascii_case(&exe)) {
                    Some(p) => p.volumes.push(vol),
                    None => v.push(Programa { nome: nome_do_programa(&exe), exe, nivel, mudo, volumes: vec![vol] }),
                }
            }
        }
        v
    }
}

/// Caminho do executável do processo.
pub fn caminho_do_processo(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n).is_ok();
        _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// Descrição do executável ("Google Chrome"), ou o nome do arquivo sem a extensão.
fn nome_do_programa(exe: &str) -> String {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let arquivo = std::path::Path::new(exe).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let u: Vec<u16> = exe.encode_utf16().chain([0]).collect();
    let descricao = unsafe {
        let tam = GetFileVersionInfoSizeW(PCWSTR(u.as_ptr()), None);
        let mut dados = vec![0u8; tam as usize];
        (tam > 0 && GetFileVersionInfoW(PCWSTR(u.as_ptr()), None, tam, dados.as_mut_ptr() as *mut _).is_ok())
            .then(|| {
                // primeira tradução da tabela, e o FileDescription nela
                let mut p = std::ptr::null_mut();
                let mut n = 0u32;
                if !VerQueryValueW(dados.as_ptr() as *const _, windows::core::w!("\\VarFileInfo\\Translation"), &mut p, &mut n).as_bool() || n < 4 {
                    return None;
                }
                let t = *(p as *const [u16; 2]);
                let chave: Vec<u16> = format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", t[0], t[1]).encode_utf16().chain([0]).collect();
                if !VerQueryValueW(dados.as_ptr() as *const _, PCWSTR(chave.as_ptr()), &mut p, &mut n).as_bool() || n == 0 {
                    return None;
                }
                let s = std::slice::from_raw_parts(p as *const u16, n as usize);
                Some(String::from_utf16_lossy(s).trim_end_matches('\0').trim().to_string())
            })
            .flatten()
    };
    descricao.filter(|d| !d.is_empty()).unwrap_or(arquivo)
}
