//! Placas de rede (IP Helper): as físicas sempre, com o estado do cabo; as virtuais só quando
//! estão ligadas e são VPN ou têm gateway (as do WSL e do Hyper-V ficam de fora).

use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN};

pub const G_CABO: &str = "\u{f0200}";
pub const G_SEM_REDE: &str = "\u{f0319}";
pub const G_WIFI: &str = "\u{f05a9}";
const G_VPN: &str = "\u{f0582}";

pub struct Placa {
    pub nome: String,
    pub glifo: &'static str,
    pub conectada: bool,
    /// Com gateway: é por ela que sai a internet.
    pub internet: bool,
    /// IPv4, ou vazio.
    pub ip: String,
}

fn fisica(p: &IP_ADAPTER_ADDRESSES_LH) -> bool {
    let mut linha = MIB_IF_ROW2 { InterfaceLuid: p.Luid, ..Default::default() };
    // bit 0 de InterfaceAndOperStatusFlags: HardwareInterface
    unsafe { GetIfEntry2(&mut linha).is_ok() && linha.InterfaceAndOperStatusFlags._bitfield & 1 != 0 }
}

fn ipv4(p: &IP_ADAPTER_ADDRESSES_LH) -> String {
    let mut u = p.FirstUnicastAddress;
    while !u.is_null() {
        let e = unsafe { &*u };
        let sa = e.Address.lpSockaddr;
        if !sa.is_null() && unsafe { (*sa).sa_family } == AF_INET {
            let b = unsafe { (*(sa as *const SOCKADDR_IN)).sin_addr.S_un.S_un_b };
            return format!("{}.{}.{}.{}", b.s_b1, b.s_b2, b.s_b3, b.s_b4);
        }
        u = e.Next;
    }
    String::new()
}

pub fn placas() -> Vec<Placa> {
    let mut buf: Vec<u64> = vec![0; 2048];
    let mut tam = (buf.len() * 8) as u32;
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut r = unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(buf.as_mut_ptr() as *mut _), &mut tam) };
    if r == 111 {
        // ERROR_BUFFER_OVERFLOW: `tam` voltou com o tamanho certo
        buf = vec![0; tam as usize / 8 + 1];
        r = unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(buf.as_mut_ptr() as *mut _), &mut tam) };
    }
    let mut v = Vec::new();
    if r != 0 {
        return v;
    }
    let mut p = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !p.is_null() {
        let a = unsafe { &*p };
        p = a.Next;
        // loopback e túneis do sistema (Teredo, 6to4)
        if a.IfType == 24 || a.IfType == 131 {
            continue;
        }
        let nome = unsafe { a.FriendlyName.to_string().unwrap_or_default() };
        let descricao = unsafe { a.Description.to_string().unwrap_or_default() }.to_lowercase();
        let conectada = a.OperStatus == IfOperStatusUp;
        let internet = conectada && !a.FirstGatewayAddress.is_null();
        let vpn = ["tap-windows", "wintun", "openvpn", "wireguard", "vpn", "fortinet", "cisco anyconnect"].iter().any(|n| descricao.contains(n));
        if !fisica(a) && !(conectada && (vpn || internet)) {
            continue;
        }
        let glifo = if !conectada {
            G_SEM_REDE
        } else if vpn {
            G_VPN
        } else if a.IfType == 71 {
            G_WIFI
        } else {
            G_CABO
        };
        v.push(Placa { nome, glifo, conectada, internet, ip: if conectada { ipv4(a) } else { String::new() } });
    }
    // a da internet primeiro, depois as outras ligadas, por último as desligadas
    v.sort_by_key(|p| (!p.internet, !p.conectada));
    v
}
