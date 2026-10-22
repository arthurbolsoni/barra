//! Bluetooth: rádio ligado/desligado (Windows.Devices.Radios), dispositivos pareados (clássicos
//! pela API Win32, que dá a classe do aparelho; BLE pela enumeração do WinRT) e conectar ou
//! desconectar fones e caixas pelo IKsControl do filtro de áudio Bluetooth, como o ToothTray.
//! Tudo aqui pode demorar: roda numa thread à parte, com COM próprio.


use windows::core::{h, Interface, HSTRING};
use windows::Devices::Bluetooth::BluetoothLEDevice;
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationKind};
use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Foundation::IReference;
use windows::Win32::Devices::Bluetooth::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::KernelStreaming::{IKsControl, KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0, KSPROPERTY_TYPE_GET};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};

pub const G_BLUETOOTH: &str = "\u{f00af}";
pub const G_BLUETOOTH_DESLIGADO: &str = "\u{f00b2}";
const G_FONE: &str = "\u{f02cb}";
const G_HEADSET: &str = "\u{f02ce}";
const G_CAIXA: &str = "\u{f04c3}";
const G_MOUSE: &str = "\u{f037d}";
const G_TECLADO: &str = "\u{f030c}";
const G_CONTROLE: &str = "\u{f0297}";
const G_CELULAR: &str = "\u{f011c}";
const G_RELOGIO: &str = "\u{f0589}";
const G_COMPUTADOR: &str = "\u{f0379}";

/// KSPROPSETID_BtAudio: KSPROPERTY_ONESHOT_RECONNECT (0) e KSPROPERTY_ONESHOT_DISCONNECT (1).
const KSPROPSETID_BTAUDIO: windows::core::GUID = windows::core::GUID::from_u128(0x7fa06c40_b8f6_4c7e_8556_e8c33a12e54d);

#[derive(Clone)]
pub struct Dispositivo {
    pub endereco: u64,
    pub nome: String,
    pub glifo: &'static str,
    pub conectado: bool,
    /// Fone ou caixa: dá para conectar e desconectar por aqui.
    pub audio: bool,
}

#[derive(Clone, Default)]
pub struct Situacao {
    /// None: sem adaptador Bluetooth.
    pub radio: Option<bool>,
    pub dispositivos: Vec<Dispositivo>,
}

/// Roda `f` com COM multithread (as chamadas assíncronas do WinRT esperam nela).
fn com_mta<R>(f: impl FnOnce() -> R) -> R {
    let ok = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
    let r = f();
    if ok {
        unsafe { CoUninitialize() };
    }
    r
}

fn radio() -> Option<Radio> {
    let l = Radio::GetRadiosAsync().ok()?.get().ok()?;
    (0..l.Size().ok()?).filter_map(|i| l.GetAt(i).ok()).find(|r| r.Kind().ok() == Some(RadioKind::Bluetooth))
}

pub fn ligar_radio(ligar: bool) {
    com_mta(|| {
        if let Some(r) = radio() {
            _ = Radio::RequestAccessAsync().and_then(|a| a.get());
            _ = r.SetStateAsync(if ligar { RadioState::On } else { RadioState::Off }).and_then(|a| a.get());
        }
    })
}

/// Glifo pela classe do aparelho (Class of Device), ou pelo nome quando ela não diz.
fn glifo_de(classe: u32, nome: &str) -> &'static str {
    let maior = (classe >> 8) & 0x1F;
    let menor = (classe >> 2) & 0x3F;
    match maior {
        1 => return G_COMPUTADOR,
        2 => return G_CELULAR,
        4 => {
            return match menor {
                1 | 2 => G_HEADSET,
                5 | 7 => G_CAIXA,
                _ => G_FONE,
            }
        }
        5 => match menor >> 4 {
            1 => return G_TECLADO,
            2 | 3 => return G_MOUSE,
            _ if menor & 0xF == 1 || menor & 0xF == 2 => return G_CONTROLE,
            _ => {}
        },
        7 => return G_RELOGIO,
        _ => {}
    }
    let n = nome.to_lowercase();
    let tem = |p: &[&str]| p.iter().any(|x| n.contains(x));
    if tem(&["mouse", "mx master", "mx anywhere"]) {
        G_MOUSE
    } else if tem(&["keyboard", "teclado", "keys"]) {
        G_TECLADO
    } else if tem(&["controller", "xbox", "gamepad", "dualsense", "dualshock"]) {
        G_CONTROLE
    } else if tem(&["buds", "fone", "headphone", "headset", "airpods", "wh-", "wf-"]) {
        G_FONE
    } else if tem(&["watch", "band"]) {
        G_RELOGIO
    } else {
        G_BLUETOOTH
    }
}

fn classicos() -> Vec<(Dispositivo, u32)> {
    let mut v = Vec::new();
    let busca = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: true.into(),
        fReturnRemembered: true.into(),
        fReturnConnected: true.into(),
        ..Default::default()
    };
    let mut info = BLUETOOTH_DEVICE_INFO { dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32, ..Default::default() };
    unsafe {
        let Ok(h) = BluetoothFindFirstDevice(&busca, &mut info) else { return v };
        loop {
            let fim = info.szName.iter().position(|&c| c == 0).unwrap_or(info.szName.len());
            let nome = String::from_utf16_lossy(&info.szName[..fim]);
            if info.fRemembered.as_bool() || info.fAuthenticated.as_bool() {
                v.push((
                    Dispositivo {
                        endereco: info.Address.Anonymous.ullLong,
                        glifo: glifo_de(info.ulClassofDevice, &nome),
                        nome,
                        conectado: info.fConnected.as_bool(),
                        audio: false,
                    },
                    info.ulClassofDevice,
                ));
            }
            info = BLUETOOTH_DEVICE_INFO { dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32, ..Default::default() };
            if BluetoothFindNextDevice(h, &mut info).is_err() {
                break;
            }
        }
        _ = BluetoothFindDeviceClose(h);
    }
    v
}

/// "aa:bb:cc:dd:ee:ff" -> número.
fn endereco_de(s: &str) -> Option<u64> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (hex.len() == 12).then(|| u64::from_str_radix(&hex, 16).ok()).flatten()
}

fn ble() -> Vec<Dispositivo> {
    let mut v = Vec::new();
    let conectado = h!("System.Devices.Aep.IsConnected");
    let endereco = h!("System.Devices.Aep.DeviceAddress");
    let Ok(aqs) = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true) else { return v };
    let props: windows_collections::IIterable<HSTRING> = vec![conectado.clone(), endereco.clone()].into();
    let Ok(lista) = DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(&aqs, &props, DeviceInformationKind::AssociationEndpoint).and_then(|a| a.get()) else {
        return v;
    };
    for i in 0..lista.Size().unwrap_or(0) {
        let Ok(d) = lista.GetAt(i) else { continue };
        let nome = d.Name().map(|n| n.to_string()).unwrap_or_default();
        let Ok(p) = d.Properties() else { continue };
        let ligado = p.Lookup(conectado).ok().and_then(|x| x.cast::<IReference<bool>>().ok()).and_then(|r| r.Value().ok()).unwrap_or(false);
        let Some(end) = p.Lookup(endereco).ok().and_then(|x| x.cast::<IReference<HSTRING>>().ok()).and_then(|r| r.Value().ok()).and_then(|s| endereco_de(&s.to_string())) else {
            continue;
        };
        v.push(Dispositivo { endereco: end, glifo: glifo_de(0, &nome), nome, conectado: ligado, audio: false });
    }
    v
}

/// Controles KS dos filtros de áudio Bluetooth, com o id do filtro (traz o endereço do aparelho).
fn controles_audio() -> Vec<(String, IKsControl)> {
    let mut v = Vec::new();
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return v };
        let Ok(lista) = en.EnumAudioEndpoints(eAll, DEVICE_STATE(DEVICE_STATEMASK_ALL)) else { return v };
        for i in 0..lista.GetCount().unwrap_or(0) {
            let r = (|| -> windows::core::Result<(String, IKsControl)> {
                let d = lista.Item(i)?;
                let topo: IDeviceTopology = d.Activate(CLSCTX_ALL, None)?;
                let outro = topo.GetConnector(0)?.GetConnectedTo()?;
                let parte: IPart = outro.cast()?;
                // o filtro KS do outro lado do conector: o IKsControl vem do dispositivo dele
                let id = crate::audio::texto_com(parte.GetTopologyObject()?.GetDeviceId()?);
                let filtro = en.GetDevice(&windows::core::HSTRING::from(id.as_str()))?;
                Ok((id.to_lowercase(), filtro.Activate::<IKsControl>(CLSCTX_ALL, None)?))
            })();
            if let Ok((id, ks)) = r {
                if id.contains("bth") {
                    v.push((id, ks));
                }
            }
        }
    }
    v
}

fn do_aparelho(id: &str, endereco: u64) -> bool {
    id.contains(&format!("{endereco:012x}"))
}

pub fn ler() -> Situacao {
    com_mta(|| {
        let radio = radio().map(|r| r.State().ok() == Some(RadioState::On));
        let audio = controles_audio();
        let mut v: Vec<Dispositivo> = classicos().into_iter().map(|(d, _)| d).collect();
        for d in ble() {
            match v.iter_mut().find(|x| x.endereco == d.endereco) {
                Some(x) => x.conectado |= d.conectado,
                None => v.push(d),
            }
        }
        for d in &mut v {
            d.audio = audio.iter().any(|(id, _)| do_aparelho(id, d.endereco));
        }
        // conectados primeiro
        v.sort_by_key(|d| (!d.conectado, d.nome.to_lowercase()));
        Situacao { radio, dispositivos: v }
    })
}

/// Conecta (ou desconecta) o fone/caixa: o pedido vai para cada filtro de áudio do aparelho.
pub fn conectar(endereco: u64, conectar: bool) {
    com_mta(|| {
        let p = KSIDENTIFIER {
            Anonymous: KSIDENTIFIER_0 { Anonymous: KSIDENTIFIER_0_0 { Set: KSPROPSETID_BTAUDIO, Id: if conectar { 0 } else { 1 }, Flags: KSPROPERTY_TYPE_GET } },
        };
        for (id, ks) in controles_audio() {
            if do_aparelho(&id, endereco) {
                let mut n = 0u32;
                unsafe { _ = ks.KsProperty(&p, size_of::<KSIDENTIFIER>() as u32, std::ptr::null_mut(), 0, &mut n) };
            }
        }
    })
}
