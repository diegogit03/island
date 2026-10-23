// Em release no Windows, abre só a janela do jogo (sem console).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod building;
mod client;
mod combat;
mod crafting;
mod creatures;
mod daynight;
mod gathering;
mod inventory;
mod items;
mod menu;
mod net;
mod player;
mod progression;
mod protocol;
mod raft;
mod render;
mod rng;
mod save;
mod server;
mod sim;
mod slots;
mod taming;
mod ui;
mod vitals;
mod world;

use std::path::{Path, PathBuf};

use macroquad::prelude::*;

use client::{Client, Status};
use menu::{Choice, Menu};
use net::Connection;
use protocol::DEFAULT_PORT;
use server::{ServerHandle, ServerOptions};
use sim::Sim;

const DEFAULT_SAVE: &str = "saves/save.json";
/// Último nome usado no menu.
const NAME_FILE: &str = "saves/nome.txt";

fn window_conf() -> Conf {
    Conf {
        window_title: "Island".to_owned(),
        window_width: 1280,
        window_height: 720,
        ..Default::default()
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn seed() -> u64 {
    env("ISLAND_SEED").and_then(|s| s.parse().ok()).unwrap_or(1337)
}

fn save_path() -> PathBuf {
    PathBuf::from(env("ISLAND_SAVE").unwrap_or_else(|| DEFAULT_SAVE.to_owned()))
}

/// Carrega o mundo salvo se existir. Um save inválido é preservado como `.bak` antes de começar do zero.
fn load_or_new(path: &Path, seed: u64) -> Sim {
    if env("ISLAND_NEW").is_some() {
        return Sim::new(seed);
    }
    match save::read(path) {
        Ok(Some(data)) => Sim::from_save(data),
        Ok(None) => Sim::new(seed),
        Err(e) => {
            let backup = path.with_extension("json.bak");
            eprintln!("Save inválido ({e}); copiado para {}", backup.display());
            let _ = std::fs::copy(path, &backup);
            Sim::new(seed)
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--server") {
        dedicated(&args);
        return;
    }
    macroquad::Window::from_config(window_conf(), run());
}

/// `island --server [--port 7777] [--save caminho.json] [--name "Nome"]`: servidor sem janela.
fn dedicated(args: &[String]) {
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let port = value("--port").and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
    let path = value("--save").map_or_else(save_path, PathBuf::from);
    let name = value("--name").unwrap_or_else(|| "Servidor dedicado".to_owned());
    let sim = load_or_new(&path, seed());
    println!(
        "Island: servidor dedicado \"{name}\" na porta {port}, mundo em {}",
        path.display()
    );
    let opts = ServerOptions {
        lan: true,
        port,
        save_path: Some(path),
        dedicated: true,
        ..ServerOptions::new(&name)
    };
    if let Err(e) = server::run_dedicated(sim, opts) {
        eprintln!("Não foi possível abrir a porta {port}: {e}");
        std::process::exit(1);
    }
}

async fn run() {
    if let Some(path) = env("ISLAND_SHOT") {
        screenshot(&path).await;
        return;
    }
    // Fechar a janela salva antes de sair.
    prevent_quit();
    let name = std::fs::read_to_string(NAME_FILE)
        .map(|n| n.trim().to_owned())
        .unwrap_or_default();
    let mut menu = Menu::new(if name.is_empty() { "Jogador".to_owned() } else { name }, None);

    loop {
        // ----- Menu -----
        let choice = loop {
            if let Some(choice) = menu.update() {
                break choice;
            }
            menu.draw();
            if is_quit_requested() {
                return;
            }
            next_frame().await;
        };
        let name = protocol::sanitize_name(&menu.name);
        if let Some(dir) = Path::new(NAME_FILE).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(NAME_FILE, &name);

        // ----- Conexão -----
        let (connection, server) = match start_session(choice, &name) {
            Ok(session) => session,
            Err(msg) => {
                menu.set_message(msg);
                continue;
            }
        };
        let mut client = Client::new(connection);

        // ----- Partida -----
        let reason = loop {
            let dt = get_frame_time().min(1.0 / 20.0);
            client.update(dt);
            client.draw();
            if is_quit_requested() {
                client.quit();
                if let Some(server) = server {
                    server.stop();
                }
                return;
            }
            if let Status::Closed(reason) = &client.status {
                break reason.clone();
            }
            next_frame().await;
        };
        if let Some(server) = server {
            server.stop();
        }
        menu = Menu::new(name, Some(reason));
    }
}

/// Sobe o servidor local (um jogador / host) ou conecta num servidor da LAN.
fn start_session(choice: Choice, name: &str) -> Result<(Connection, Option<ServerHandle>), String> {
    match choice {
        Choice::Join(addr) => Connection::remote(addr, name)
            .map(|c| (c, None))
            .map_err(|e| format!("Não foi possível conectar em {addr}: {e}")),
        Choice::Solo | Choice::Host => {
            let lan = matches!(choice, Choice::Host);
            let path = save_path();
            let opts = ServerOptions {
                lan,
                save_path: Some(path.clone()),
                ..ServerOptions::new(&format!("Ilha de {name}"))
            };
            let server = server::start(load_or_new(&path, seed()), opts)
                .map_err(|e| format!("Não foi possível abrir a porta {DEFAULT_PORT}: {e}"))?;
            Ok((server.connect_local(name), Some(server)))
        }
    }
}

/// Smoke test: ISLAND_SHOT=caminho.png salva um screenshot e encerra (sem ler/gravar save).
/// Cenas: ISLAND_SHOT_INVENTORY, _DEAD, _BUILD, _HOUR=22, _CREATURES, _FIRE, _TAME, _RIDE, _PET,
/// _MOUNTAIN, _COOP (um segundo jogador "Bia" ao lado, com mensagem no chat), _FLY (voando de
/// pteranodonte com um argentavis), _CARGO (painel de carga do argentavis), _ICE (bioma de gelo,
/// montado num mamute), _SWAMP (pântano, montado num sarcosuchus entre piranhas), _SEA (jangada no mar
/// com fauna marinha) e _SEA_RIDE (montado num mosassauro).
/// Com ISLAND_SHOT_JOIN=ip:porta, entra num servidor já rodando em vez de subir um local;
/// ISLAND_SHOT_MENU mostra o menu principal.
async fn screenshot(path: &str) {
    if env("ISLAND_SHOT_MENU").is_some() {
        let mut menu = Menu::new("Jogador".to_owned(), None);
        // Dá tempo de ouvir os anúncios da LAN (o servidor anuncia a cada 1 s).
        let start = get_time();
        while get_time() - start < 3.0 {
            menu.update();
            menu.draw();
            next_frame().await;
        }
        menu.draw();
        get_screen_data().export_png(path);
        return;
    }
    let setup = |sim: &mut Sim| {
        if env("ISLAND_SHOT_INVENTORY").is_some() {
            sim.give_debug_items(0);
            sim.debug_xp(1500);
        }
        if env("ISLAND_SHOT_MOUNTAIN").is_some() {
            sim.demo_mountain();
        }
        if env("ISLAND_SHOT_CREATURES").is_some() {
            sim.spawn_demo_creatures();
        }
        if env("ISLAND_SHOT_BUILD").is_some() {
            sim.build_demo_base();
        }
        if env("ISLAND_SHOT_TAME").is_some() {
            sim.demo_taming();
        }
        if env("ISLAND_SHOT_RIDE").is_some() {
            sim.demo_riding();
        }
        if env("ISLAND_SHOT_PET").is_some() {
            sim.demo_pet_panel();
        }
        if env("ISLAND_SHOT_FLY").is_some() {
            sim.demo_flying();
        }
        if env("ISLAND_SHOT_CARGO").is_some() {
            sim.demo_cargo();
        }
        if env("ISLAND_SHOT_ICE").is_some() {
            sim.demo_ice();
        }
        if env("ISLAND_SHOT_SWAMP").is_some() {
            sim.demo_swamp();
        }
        if env("ISLAND_SHOT_SEA").is_some() {
            sim.demo_sea();
        }
        if env("ISLAND_SHOT_SEA_RIDE").is_some() {
            sim.demo_sea_ride();
        }
        if env("ISLAND_SHOT_FIRE").is_some() {
            sim.build_demo_base();
            sim.open_first_campfire();
        }
        if env("ISLAND_SHOT_COOP").is_some() {
            sim.demo_coop();
        }
        if let Some(hour) = env("ISLAND_SHOT_HOUR").and_then(|h| h.parse().ok()) {
            sim.skip_to_hour(hour);
        }
        if env("ISLAND_SHOT_DEAD").is_some() {
            sim.give_debug_items(0);
            sim.kill();
        }
    };
    // ISLAND_SHOT_JOIN=ip:porta entra num servidor de outro processo (ex.: `island --server`).
    let (connection, server) = match env("ISLAND_SHOT_JOIN").and_then(|a| a.parse().ok()) {
        Some(addr) => (
            Connection::remote(addr, "Visitante").expect("conectar no servidor"),
            None,
        ),
        None => {
            let opts = ServerOptions {
                setup: Some(Box::new(setup)),
                ..ServerOptions::new("screenshot")
            };
            let server = server::start(Sim::new(seed()), opts).expect("servidor local");
            (server.connect_local("Jogador"), Some(server))
        }
    };
    let mut client = Client::new(connection);
    if env("ISLAND_SHOT_INVENTORY").is_some() {
        client.ui.inventory_open = true;
    }
    for _ in 0..600 {
        client.update(1.0 / 60.0);
        client.draw();
        if client.frames_ready >= 20 {
            get_screen_data().export_png(path);
            break;
        }
        next_frame().await;
    }
    client.quit();
    if let Some(server) = server {
        server.stop();
    }
}
