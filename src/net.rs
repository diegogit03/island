//! Transporte: quadros (tamanho + bincode) sobre TCP, conexão do cliente (local ou remota)
//! e descoberta de servidores na LAN por broadcast UDP.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::protocol::{ClientMsg, DISCOVERY_PORT, PROTOCOL_VERSION, ServerMsg};
use crate::server::{ConnId, ToServer};

/// Proteção contra lixo na rede: nenhum quadro legítimo chega perto disso.
const MAX_FRAME: usize = 16 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Servidor some da lista se não anunciar por este tempo.
const DISCOVERY_TTL: Duration = Duration::from_secs(4);
const BEACON_MAGIC: u32 = 0x1514_4E44;

pub fn write_frame<T: Serialize>(stream: &mut impl Write, msg: &T) -> io::Result<()> {
    let bytes = bincode::serialize(msg).map_err(io::Error::other)?;
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)
}

pub fn read_frame<T: DeserializeOwned>(stream: &mut impl Read) -> io::Result<T> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::other("quadro grande demais"));
    }
    let mut buf = vec![0; len];
    stream.read_exact(&mut buf)?;
    bincode::deserialize(&buf).map_err(io::Error::other)
}

/// Thread que escreve no socket tudo o que chegar pelo canal devolvido.
pub fn spawn_writer<T: Serialize + Send + 'static>(mut stream: TcpStream) -> Sender<T> {
    let (tx, rx) = mpsc::channel::<T>();
    thread::spawn(move || {
        for msg in rx {
            if write_frame(&mut stream, &msg).is_err() {
                break;
            }
        }
        let _ = stream.shutdown(std::net::Shutdown::Both);
    });
    tx
}

/// Thread que lê quadros do socket e entrega cada um a `on_msg`; chama `on_close` ao terminar.
pub fn spawn_reader<T, F, G>(mut stream: TcpStream, mut on_msg: F, on_close: G)
where
    T: DeserializeOwned + Send + 'static,
    F: FnMut(T) -> bool + Send + 'static,
    G: FnOnce() + Send + 'static,
{
    thread::spawn(move || {
        while let Ok(msg) = read_frame::<T>(&mut stream) {
            if !on_msg(msg) {
                break;
            }
        }
        on_close();
    });
}

enum Outbound {
    /// Cliente no mesmo processo que o servidor (um jogador / host).
    Local {
        tx: Sender<ToServer>,
        conn: ConnId,
    },
    Remote(Sender<ClientMsg>),
}

/// Conexão do cliente com o servidor, igual para local e remoto.
pub struct Connection {
    out: Outbound,
    pub rx: Receiver<ServerMsg>,
}

impl Connection {
    pub fn local(tx: Sender<ToServer>, conn: ConnId, rx: Receiver<ServerMsg>) -> Self {
        Self {
            out: Outbound::Local { tx, conn },
            rx,
        }
    }

    /// Conecta num servidor da LAN e já se apresenta.
    pub fn remote(addr: SocketAddr, name: &str) -> io::Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
        stream.set_nodelay(true)?;
        let (in_tx, rx) = mpsc::channel();
        let closed_tx = in_tx.clone();
        spawn_reader::<ServerMsg, _, _>(
            stream.try_clone()?,
            move |msg| in_tx.send(msg).is_ok(),
            move || {
                let _ = closed_tx.send(ServerMsg::Shutdown("Conexão com o servidor perdida".to_owned()));
            },
        );
        let conn = Self {
            out: Outbound::Remote(spawn_writer(stream)),
            rx,
        };
        conn.send(ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            name: name.to_owned(),
        });
        Ok(conn)
    }

    pub fn send(&self, msg: ClientMsg) {
        // Se o outro lado já fechou, o próximo `recv` avisa (Shutdown); aqui não há o que fazer.
        let _ = match &self.out {
            Outbound::Local { tx, conn } => tx.send(ToServer::Msg(*conn, msg)).map_err(|_| ()),
            Outbound::Remote(tx) => tx.send(msg).map_err(|_| ()),
        };
    }
}

// ---------- Descoberta na LAN ----------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Beacon {
    magic: u32,
    version: u32,
    name: String,
    port: u16,
    players: u32,
}

/// Servidor anunciando na LAN.
#[derive(Clone, Debug)]
pub struct Discovered {
    pub name: String,
    pub addr: SocketAddr,
    pub players: u32,
    last_seen: Instant,
}

/// Escuta anúncios de servidores na LAN.
pub struct Discovery {
    socket: Option<UdpSocket>,
    pub servers: Vec<Discovered>,
    /// IP local, para preferir endereços da mesma sub-rede (e não de VPNs).
    local: Option<IpAddr>,
}

/// Mesmos três primeiros octetos (sub-rede /24 doméstica).
fn same_subnet(a: IpAddr, b: Option<IpAddr>) -> bool {
    match (a, b) {
        (IpAddr::V4(a), Some(IpAddr::V4(b))) => a.octets()[..3] == b.octets()[..3],
        _ => false,
    }
}

impl Discovery {
    pub fn new() -> Self {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT))
            .and_then(|s| s.set_nonblocking(true).map(|_| s))
            .ok();
        Self {
            socket,
            servers: Vec::new(),
            local: local_ip(),
        }
    }

    /// `false` se a porta de descoberta já está em uso (outro jogo aberto na mesma máquina).
    pub fn is_listening(&self) -> bool {
        self.socket.is_some()
    }

    pub fn poll(&mut self) {
        let Some(socket) = &self.socket else { return };
        let mut buf = [0u8; 512];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            let Ok(beacon) = bincode::deserialize::<Beacon>(&buf[..n]) else {
                continue;
            };
            if beacon.magic != BEACON_MAGIC || beacon.version != PROTOCOL_VERSION {
                continue;
            }
            let addr = SocketAddr::new(from.ip(), beacon.port);
            // O mesmo servidor pode chegar por duas interfaces (LAN e VPN): fica um só,
            // preferindo o endereço da sub-rede local.
            let local = self.local;
            if let Some(existing) = self
                .servers
                .iter_mut()
                .find(|s| s.name == beacon.name && s.addr.port() == beacon.port && s.addr != addr)
            {
                if same_subnet(addr.ip(), local) && !same_subnet(existing.addr.ip(), local) {
                    existing.addr = addr;
                }
                existing.last_seen = Instant::now();
                existing.players = beacon.players;
                continue;
            }
            let entry = Discovered {
                name: beacon.name,
                addr,
                players: beacon.players,
                last_seen: Instant::now(),
            };
            match self.servers.iter_mut().find(|s| s.addr == addr) {
                Some(existing) => *existing = entry,
                None => self.servers.push(entry),
            }
        }
        self.servers.retain(|s| s.last_seen.elapsed() < DISCOVERY_TTL);
    }
}

/// Anuncia o servidor na LAN (broadcast UDP).
pub struct Announcer {
    socket: UdpSocket,
    targets: Vec<Ipv4Addr>,
}

impl Announcer {
    pub fn new() -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;
        // No Windows, 255.255.255.255 sai por uma interface só (às vezes a de uma VPN).
        // Mandar também para o broadcast da sub-rede local (/24, comum em redes domésticas)
        // garante que os PCs da LAN ouçam.
        let mut targets = vec![Ipv4Addr::BROADCAST];
        if let Some(IpAddr::V4(ip)) = local_ip() {
            let [a, b, c, _] = ip.octets();
            targets.push(Ipv4Addr::new(a, b, c, 255));
        }
        Ok(Self { socket, targets })
    }

    pub fn announce(&self, name: &str, port: u16, players: u32) {
        let beacon = Beacon {
            magic: BEACON_MAGIC,
            version: PROTOCOL_VERSION,
            name: name.to_owned(),
            port,
            players,
        };
        if let Ok(bytes) = bincode::serialize(&beacon) {
            for target in &self.targets {
                let _ = self.socket.send_to(&bytes, (*target, DISCOVERY_PORT));
            }
        }
    }
}

/// IP desta máquina na rede local (para mostrar a quem vai entrar digitando o endereço).
/// O `connect` de UDP não envia nada: só escolhe a interface de saída.
pub fn local_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(10, 255, 255, 255), 1)).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Command;

    #[test]
    fn frames_round_trip_over_a_buffer() {
        let msg = ClientMsg::Command(Command::Chat("olá".to_owned()));
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();
        let back: ClientMsg = read_frame(&mut buf.as_slice()).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn subnet_preference() {
        let local = Some(IpAddr::from([192, 168, 0, 104]));
        assert!(same_subnet(IpAddr::from([192, 168, 0, 7]), local));
        assert!(!same_subnet(IpAddr::from([26, 124, 1, 250]), local));
    }

    #[test]
    fn oversized_frames_are_rejected() {
        let mut buf = ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec();
        buf.extend([0; 8]);
        assert!(read_frame::<ClientMsg>(&mut buf.as_slice()).is_err());
    }
}

#[cfg(test)]
mod discovery_probe {
    use super::*;

    #[test]
    #[ignore]
    fn announce_is_heard_on_this_machine() {
        let mut discovery = Discovery::new();
        assert!(discovery.is_listening());
        let announcer = Announcer::new().unwrap();
        for _ in 0..20 {
            announcer.announce("sonda", 7777, 1);
            std::thread::sleep(Duration::from_millis(100));
            discovery.poll();
            if !discovery.servers.is_empty() {
                println!("ouvido: {:?}", discovery.servers);
                return;
            }
        }
        panic!("nenhum anúncio ouvido");
    }
}
