//! Servidor: dono da simulação. Roda num tick fixo, recebe input/comandos dos clientes e devolve
//! snapshots e eventos. O mesmo código atende um jogador (cliente no mesmo processo), host de LAN
//! (local + clientes TCP) e servidor dedicado sem janela (`island --server`).

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::net::{self, Announcer, Connection};
use crate::protocol::{ClientMsg, Command, NetEvent, PROTOCOL_VERSION, PlayerId, ServerMsg, sanitize_name};
use crate::save;
use crate::sim::Sim;

pub const TICK_RATE: f32 = 60.0;
/// Clientes remotos recebem snapshot a cada N ticks (30 Hz); o local, todo tick.
const REMOTE_SNAPSHOT_EVERY: u64 = 2;
const AUTOSAVE: Duration = Duration::from_secs(60);
const ANNOUNCE_EVERY: Duration = Duration::from_secs(1);
/// Se o servidor atrasar muito, não tenta recuperar mais que isso de uma vez.
const MAX_CATCH_UP_TICKS: u32 = 5;

pub type ConnId = u64;

/// Tudo o que chega ao loop do servidor.
pub enum ToServer {
    Connect {
        conn: ConnId,
        out: Sender<ServerMsg>,
        local: bool,
    },
    Msg(ConnId, ClientMsg),
    Disconnected(ConnId),
    Shutdown,
}

/// Roda uma vez, logo depois que o primeiro jogador entra.
pub type SetupFn = Box<dyn FnOnce(&mut Sim) + Send>;

pub struct ServerOptions {
    /// Aceitar conexões TCP e anunciar na LAN.
    pub lan: bool,
    pub port: u16,
    pub save_path: Option<PathBuf>,
    /// Nome anunciado na LAN.
    pub name: String,
    /// Sem cliente local: não fecha quando todos saem; registra em arquivo.
    pub dedicated: bool,
    /// Roda uma vez, logo depois que o primeiro jogador entra (cenas de screenshot).
    pub setup: Option<SetupFn>,
}

impl ServerOptions {
    pub fn new(name: &str) -> Self {
        Self {
            lan: false,
            port: crate::protocol::DEFAULT_PORT,
            save_path: None,
            name: name.to_owned(),
            dedicated: false,
            setup: None,
        }
    }
}

struct ClientSlot {
    out: Sender<ServerMsg>,
    player: Option<PlayerId>,
    local: bool,
}

/// Servidor rodando numa thread do processo do jogo.
pub struct ServerHandle {
    tx: Sender<ToServer>,
    next_conn: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl ServerHandle {
    /// Cliente no mesmo processo (sem rede).
    pub fn connect_local(&self, name: &str) -> Connection {
        let conn = self.next_conn.fetch_add(1, Ordering::Relaxed);
        let (out, rx) = mpsc::channel();
        let _ = self.tx.send(ToServer::Connect { conn, out, local: true });
        let connection = Connection::local(self.tx.clone(), conn, rx);
        connection.send(ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            name: name.to_owned(),
        });
        connection
    }

    /// Pede para o servidor salvar e parar, e espera ele terminar.
    pub fn stop(mut self) {
        let _ = self.tx.send(ToServer::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Sobe o servidor numa thread. Com `lan`, também escuta TCP e anuncia na rede.
pub fn start(sim: Sim, opts: ServerOptions) -> io::Result<ServerHandle> {
    let (tx, rx) = mpsc::channel();
    let next_conn = Arc::new(AtomicU64::new(1));
    if opts.lan {
        listen(opts.port, tx.clone(), next_conn.clone())?;
    }
    let thread = thread::Builder::new()
        .name("servidor".to_owned())
        .spawn(move || run(sim, rx, opts))?;
    Ok(ServerHandle {
        tx,
        next_conn,
        thread: Some(thread),
    })
}

/// Servidor dedicado: escuta a LAN e roda o loop nesta thread até ser encerrado.
pub fn run_dedicated(sim: Sim, opts: ServerOptions) -> io::Result<()> {
    let (tx, rx) = mpsc::channel();
    listen(opts.port, tx.clone(), Arc::new(AtomicU64::new(1)))?;
    // `tx` fica vivo aqui para o canal nunca fechar sozinho.
    let _keep = tx;
    run(sim, rx, opts);
    Ok(())
}

/// Aceita conexões TCP numa thread; cada cliente ganha uma thread de leitura e uma de escrita.
fn listen(port: u16, tx: Sender<ToServer>, next_conn: Arc<AtomicU64>) -> io::Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))?;
    thread::Builder::new().name("aceitar".to_owned()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_nodelay(true);
            let Ok(reader) = stream.try_clone() else { continue };
            let conn = next_conn.fetch_add(1, Ordering::Relaxed);
            let out = net::spawn_writer::<ServerMsg>(stream);
            if tx
                .send(ToServer::Connect {
                    conn,
                    out,
                    local: false,
                })
                .is_err()
            {
                break;
            }
            let (msg_tx, close_tx) = (tx.clone(), tx.clone());
            net::spawn_reader::<ClientMsg, _, _>(
                reader,
                move |msg| msg_tx.send(ToServer::Msg(conn, msg)).is_ok(),
                move || {
                    let _ = close_tx.send(ToServer::Disconnected(conn));
                },
            );
        }
    })?;
    Ok(())
}

struct Server {
    sim: Sim,
    opts: ServerOptions,
    clients: HashMap<ConnId, ClientSlot>,
    /// Cliente local (o host): se ele sai, o servidor fecha.
    host: Option<ConnId>,
    log: Option<std::fs::File>,
}

impl Server {
    fn log(&mut self, msg: &str) {
        println!("[servidor] {msg}");
        if let Some(file) = self.log.as_mut() {
            let _ = writeln!(file, "{msg}");
        }
    }

    fn save(&mut self) {
        let Some(path) = self.opts.save_path.clone() else {
            return;
        };
        match save::write(&path, &self.sim.to_save()) {
            Ok(()) => {
                self.sim.broadcast("Jogo salvo");
                self.log(&format!("salvo em {}", path.display()));
            }
            Err(e) => self.log(&format!("falha ao salvar: {e}")),
        }
    }

    fn send(&self, conn: ConnId, msg: ServerMsg) {
        if let Some(client) = self.clients.get(&conn) {
            let _ = client.out.send(msg);
        }
    }

    /// Trata uma mensagem. Retorna `false` se o servidor deve parar.
    fn handle(&mut self, conn: ConnId, msg: ClientMsg) -> bool {
        let player = self.clients.get(&conn).and_then(|c| c.player);
        match (msg, player) {
            (ClientMsg::Hello { version, name }, None) => {
                if version != PROTOCOL_VERSION {
                    self.send(
                        conn,
                        ServerMsg::Rejected(format!(
                            "Versão diferente (servidor: {PROTOCOL_VERSION}, sua: {version}). Atualize o jogo."
                        )),
                    );
                    return true;
                }
                let id = self.sim.join(&sanitize_name(&name));
                if let Some(client) = self.clients.get_mut(&conn) {
                    client.player = Some(id);
                }
                if let Some(setup) = self.opts.setup.take() {
                    setup(&mut self.sim);
                }
                let seed = self.sim.seed();
                self.send(conn, ServerMsg::Welcome { you: id, seed });
                let joined = self
                    .sim
                    .index(id)
                    .map(|i| self.sim.players[i].name.clone())
                    .unwrap_or_default();
                self.log(&format!("{joined} entrou"));
            }
            (ClientMsg::Input(input), Some(id)) => self.sim.set_input(id, input),
            (ClientMsg::Command(Command::Save), Some(_)) => {
                // Num jogo hospedado só o host salva; no dedicado, qualquer um.
                if self.opts.dedicated || Some(conn) == self.host {
                    self.save();
                }
            }
            (ClientMsg::Command(cmd), Some(id)) => self.sim.command(id, cmd),
            (ClientMsg::Bye, _) => return self.disconnect(conn),
            _ => {}
        }
        true
    }

    /// Remove um cliente. Retorna `false` se era o host (o servidor deve parar).
    fn disconnect(&mut self, conn: ConnId) -> bool {
        let Some(client) = self.clients.remove(&conn) else {
            return true;
        };
        if let Some(id) = client.player {
            let name = self
                .sim
                .index(id)
                .map(|i| self.sim.players[i].name.clone())
                .unwrap_or_default();
            self.sim.leave(id);
            self.log(&format!("{name} saiu"));
        }
        if Some(conn) == self.host {
            return false;
        }
        if self.opts.dedicated && self.sim.players.is_empty() {
            self.save();
        }
        true
    }

    fn flush(&mut self, tick: u64) {
        let events = self.sim.take_events();
        let chat = self.sim.take_chat();
        for client in self.clients.values() {
            let Some(id) = client.player else { continue };
            let mine: Vec<NetEvent> = events
                .iter()
                .filter(|(to, _)| to.is_none_or(|to| to == id))
                .map(|(_, e)| e.clone())
                .collect();
            if !mine.is_empty() {
                let _ = client.out.send(ServerMsg::Events(mine));
            }
            for (from, text) in &chat {
                let _ = client.out.send(ServerMsg::Chat {
                    from: from.clone(),
                    text: text.clone(),
                });
            }
            if (client.local || tick.is_multiple_of(REMOTE_SNAPSHOT_EVERY))
                && let Some(snapshot) = self.sim.snapshot(id)
            {
                let _ = client.out.send(ServerMsg::Snapshot(Box::new(snapshot)));
            }
        }
    }
}

/// Loop principal do servidor (bloqueia até `Shutdown` ou o host sair).
pub fn run(sim: Sim, rx: Receiver<ToServer>, opts: ServerOptions) {
    let log = opts.dedicated.then(|| {
        let path = opts
            .save_path
            .as_ref()
            .map_or_else(|| PathBuf::from("server.log"), |p| p.with_extension("log"));
        OpenOptions::new().create(true).append(true).open(path).ok()
    });
    let announcer = if opts.lan { Announcer::new().ok() } else { None };
    let mut server = Server {
        sim,
        opts,
        clients: HashMap::new(),
        host: None,
        log: log.flatten(),
    };
    if server.opts.lan {
        let port = server.opts.port;
        let ip = net::local_ip().map_or_else(|| "?".to_owned(), |ip| ip.to_string());
        server.log(&format!("escutando em {ip}:{port}"));
    }

    let tick = Duration::from_secs_f32(1.0 / TICK_RATE);
    let mut next_tick = Instant::now();
    let mut tick_count: u64 = 0;
    let mut last_save = Instant::now();
    let mut last_announce = Instant::now() - ANNOUNCE_EVERY;
    let mut shutdown_reason = "Servidor encerrado".to_owned();

    'main: loop {
        loop {
            match rx.try_recv() {
                Ok(ToServer::Connect { conn, out, local }) => {
                    if local && server.host.is_none() {
                        server.host = Some(conn);
                    }
                    server.clients.insert(
                        conn,
                        ClientSlot {
                            out,
                            player: None,
                            local,
                        },
                    );
                }
                Ok(ToServer::Msg(conn, msg)) => {
                    if !server.handle(conn, msg) {
                        shutdown_reason = "O host saiu do jogo".to_owned();
                        break 'main;
                    }
                }
                Ok(ToServer::Disconnected(conn)) => {
                    if !server.disconnect(conn) {
                        shutdown_reason = "O host saiu do jogo".to_owned();
                        break 'main;
                    }
                }
                Ok(ToServer::Shutdown) | Err(TryRecvError::Disconnected) => break 'main,
                Err(TryRecvError::Empty) => break,
            }
        }

        let mut steps = 0;
        while next_tick <= Instant::now() && steps < MAX_CATCH_UP_TICKS {
            server.sim.tick(tick.as_secs_f32());
            next_tick += tick;
            tick_count += 1;
            steps += 1;
            server.flush(tick_count);
        }
        if steps == MAX_CATCH_UP_TICKS {
            next_tick = Instant::now();
        }

        if last_save.elapsed() >= AUTOSAVE {
            last_save = Instant::now();
            if !server.sim.players.is_empty() {
                server.save();
            }
        }
        if let Some(announcer) = &announcer
            && last_announce.elapsed() >= ANNOUNCE_EVERY
        {
            last_announce = Instant::now();
            announcer.announce(&server.opts.name, server.opts.port, server.sim.players.len() as u32);
        }

        let wait = next_tick.saturating_duration_since(Instant::now());
        thread::sleep(wait.min(Duration::from_millis(4)));
    }

    // Quem ainda está no jogo é salvo junto.
    let ids: Vec<PlayerId> = server.sim.players.iter().map(|p| p.id).collect();
    for id in ids {
        server.sim.leave(id);
    }
    server.save();
    for client in server.clients.values() {
        let _ = client.out.send(ServerMsg::Shutdown(shutdown_reason.clone()));
    }
    server.log("encerrado");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Input;

    fn wait_for<T>(rx: &Receiver<ServerMsg>, mut pick: impl FnMut(ServerMsg) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(msg) = rx.recv_timeout(Duration::from_millis(50))
                && let Some(found) = pick(msg)
            {
                return found;
            }
        }
        panic!("o servidor não respondeu a tempo");
    }

    #[test]
    fn local_client_gets_welcome_and_snapshots_and_moves() {
        let server = start(Sim::new(1337), ServerOptions::new("teste")).unwrap();
        let conn = server.connect_local("Ana");
        let me = wait_for(&conn.rx, |m| match m {
            ServerMsg::Welcome { you, .. } => Some(you),
            _ => None,
        });
        let first = wait_for(&conn.rx, |m| match m {
            ServerMsg::Snapshot(s) => Some(s),
            _ => None,
        });
        let start_x = first.players.iter().find(|p| p.id == me).unwrap().pos[0];
        conn.send(ClientMsg::Input(Input {
            dir: [1.0, 0.0],
            ..Input::default()
        }));
        thread::sleep(Duration::from_millis(300));
        let later = wait_for(&conn.rx, |m| match m {
            ServerMsg::Snapshot(s) => Some(s),
            _ => None,
        });
        let x = later.players.iter().find(|p| p.id == me).unwrap().pos[0];
        assert!(x != start_x, "o jogador deveria ter andado (ou batido em algo)");
        server.stop();
    }

    #[test]
    fn two_players_over_tcp_see_each_other() {
        let port = 39_000 + (std::process::id() % 1000) as u16;
        let server = start(
            Sim::new(7),
            ServerOptions {
                lan: true,
                port,
                ..ServerOptions::new("teste")
            },
        )
        .unwrap();
        let host = server.connect_local("Host");
        let guest = Connection::remote(([127, 0, 0, 1], port).into(), "Convidado").unwrap();
        wait_for(&guest.rx, |m| matches!(m, ServerMsg::Welcome { .. }).then_some(()));
        let seen = wait_for(&host.rx, |m| match m {
            ServerMsg::Snapshot(s) if s.players.len() == 2 => Some(s),
            _ => None,
        });
        assert!(seen.players.iter().any(|p| p.name == "Convidado"));

        guest.send(ClientMsg::Command(Command::Chat("oi".to_owned())));
        let (from, text) = wait_for(&host.rx, |m| match m {
            ServerMsg::Chat { from, text } => Some((from, text)),
            _ => None,
        });
        assert_eq!((from.as_str(), text.as_str()), ("Convidado", "oi"));

        // O host sai: o convidado é avisado.
        host.send(ClientMsg::Bye);
        wait_for(&guest.rx, |m| matches!(m, ServerMsg::Shutdown(_)).then_some(()));
        server.stop();
    }
}
