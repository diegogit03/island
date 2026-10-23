# AGENTS.md — Island

Survival 2D em Rust inspirado em ARK (coleta, crafting, construção, fauna e domesticação de dinossauros) com elementos de MMORPG. Visão 3/4 top-down; o personagem anda em 8 direções mas o sprite só vira para **esquerda/direita**. Desktop, multiplayer em LAN (servidor autoritativo).

## Stack
- Rust (edition 2024) + [macroquad 0.4](https://docs.rs/macroquad/0.4) + serde/serde_json (save) + bincode 1.3 (rede). Evite novas dependências sem motivo.
- Rede só com `std::net` (TCP para o jogo, UDP broadcast para achar servidores na LAN).
- Arte provisória desenhada com primitivas em `render.rs`/`ui.rs` (sem assets).

## Comandos
```
cargo run                 # menu: um jogador, hospedar na LAN ou entrar (debug: F1 itens, F2 -25 vida, F3 +3h, F4 +200 XP)
cargo run -- --server [--port 7777] [--save saves/save.json] [--name "Nome"]   # servidor dedicado, sem janela
cargo test                # lógica + servidor de verdade (cliente local e dois clientes TCP)
cargo fmt && cargo clippy --all-targets -- -D warnings   # rustfmt.toml: max_width 120
ISLAND_SHOT=out.png cargo run                          # smoke test: sobe servidor local, salva screenshot e sai
ISLAND_SHOT=out.png ISLAND_SHOT_<CENA>=1 cargo run     # cenas: INVENTORY, DEAD, BUILD, CREATURES, FIRE, TAME, RIDE, PET, FLY, CARGO, ICE, SWAMP, SEA, SEA_RIDE, DESERT, TRIBE, RECRUIT, MEMBER, FORTRESS, RAID, MOUNTAIN, COOP, MENU
ISLAND_SHOT=out.png ISLAND_SHOT_HOUR=22 cargo run      # ajusta o relógio (combina com as cenas)
ISLAND_SHOT=out.png ISLAND_SHOT_JOIN=127.0.0.1:7777 cargo run   # entra num servidor já rodando (teste com processos reais)
ISLAND_NEW=1 ISLAND_SEED=42 cargo run  # ignora o save e gera outra ilha
ISLAND_SAVE=outro.json cargo run       # caminho do save (padrão: saves/save.json)
cargo test announce_is_heard -- --ignored   # sonda: o broadcast da LAN volta para esta máquina?
```
Se `target/debug/island.exe` estiver em uso (jogo aberto), use `CARGO_TARGET_DIR=target/verify`.
Para encerrar um servidor de teste, mate **pelo PID**, nunca por nome (`taskkill /IM island.exe` fecha o jogo do usuário também).
O modo `ISLAND_SHOT` nunca lê nem grava save. O servidor salva com F5 (host), a cada 60 s, quando o host sai e, no dedicado, quando o último jogador sai.

## Arquitetura (`src/`)
Servidor autoritativo: `Sim` roda num tick fixo (60 Hz) dentro de `server.rs`; o cliente só desenha e envia `Input`/`Command`.
Um jogador = servidor numa thread + cliente local por canal (sem rede). Hospedar = o mesmo + TCP. Dedicado = só o servidor.

| Módulo | Papel |
|---|---|
| `main.rs` | Menu → partida → menu; `--server` (dedicado); screenshots. |
| `sim.rs` | **Simulação** de todos os jogadores: regras, comandos, snapshots por jogador, demos. Sem input nem desenho. |
| `server.rs` | Loop do servidor (tick, snapshots, eventos, chat, autosave, anúncio na LAN), threads TCP. |
| `client.rs` | Recebe snapshots, suaviza posições, teclado/mouse → `Input`/`Command`, desenha. |
| `protocol.rs` | Mensagens (`ClientMsg`, `ServerMsg`, `Command`, `Snapshot`, …). Mudou o formato? Suba `PROTOCOL_VERSION`. |
| `net.rs` | Quadros (u32 + bincode) sobre TCP, `Connection` (local/remota), descoberta/anúncio por UDP. |
| `menu.rs` | Tela inicial: nome, um jogador, hospedar, entrar por IP ou pela lista da LAN. |
| `slots.rs` | Pegar/soltar/dividir/transferir pilhas (roda no servidor). |
| `world.rs` | Tiles da ilha, geração procedural, nós de recurso, colisão. |
| `player.rs` | Movimento, corrida, facing esquerda/direita, timers de golpe. |
| `vitals.rs` | Vida, comida, água, stamina (máximos por jogador). |
| `progression.rs` | XP, nível, pontos de atributo e engramas. |
| `items.rs` / `inventory.rs` | Itens e inventário em slots (hotbar = slots `0..HOTBAR_SIZE`). |
| `crafting.rs` | Receitas (tempo, estação, nível, custo de engrama) e fila de crafting. |
| `building.rs` | Estruturas no grid (peça + material, portão de 3 tiles, camadas piso/objeto/teto), colisão, vida e reparo, fogo (fogueira cozinha, forja funde). |
| `npc.rs` / `sim/tribe.rs` | NPCs (estado, ferramentas, ordens, save) e o comportamento da tribo no tick: moradores, recrutar, coletar e guardar, lutar, apitos, domesticados lutando. |
| `sim/raid.rs` | Saqueadores: quando e onde aparecem (noite, perto de base), quebrar estruturas, atacar jogadores/tribo/dinos, ir embora. Não vão para o save. |
| `raft.rs` | Jangada: flutuação, remo, onde pôr e onde descer. Fica em `Sim::rafts`; o jogador nela tem `PlayerState::raft`. |
| `creatures.rs` / `taming.rs` | Fauna (IA contra vários jogadores, dono de domesticados, voadores, peso de carga), torpor e domesticação. |
| `gathering.rs` / `combat.rs` | Rendimento de coleta; pedras do estilingue. |
| `daynight.rs` | Relógio do jogo e curvas de escuridão/entardecer. |
| `render.rs` / `ui.rs` | Desenho do mundo (y-sort, vários avatares, iluminação) e HUD/painéis/chat. Só leem estado. |
| `save.rs` | Save JSON v2 (vários jogadores por nome; v1 é convertido). |
| `rng.rs` | RNG/ruído determinísticos (mesma seed = mesma ilha). |

## Convenções
- Regras de jogo ficam em módulos puros (sem desenho/input) e têm testes. Toda mudança de estado do jogo passa pelo servidor (`Sim::command` / `Sim::set_input`); o cliente nunca altera o estado dele.
- Consultas usadas pelos dois lados (alvo do golpe, interação com F) são funções livres em `sim.rs`.
- O snapshot manda inventários só do contêiner aberto por aquele jogador; o resto (tiles, posições dos nós) o cliente gera pela seed.
- Coordenadas de mundo em pixels, Y para baixo, `TILE = 32`. `pos` de entidade = ponto do pé (base), usado no y-sort e na colisão.
- Item novo: variante em `ItemId` + seus `match` em `items.rs` (inclusive `weight`) + ícone em `ui::draw_item_icon`.
- Geração do mundo: não mude a sequência do RNG de `scatter_nodes` (saves guardam nós por índice). Nós novos vão no fim, com RNG próprio (ver `scatter_mountain_extras`). Biomas novos trocam tiles depois de espalhar os nós, sem RNG (ver `freeze_north`, `flood_east` e `dry_west`). Acampamentos também vêm da seed (`place_camps`).
- Save: campo novo leva `#[serde(default)]`; quebrar o formato exige subir `SAVE_VERSION` e converter o antigo em `save.rs`.
- Espécie nova: `Species` + seus `match` + `render::draw_creature`. Voadora: `fly_speed` (no ar, colisão só com a borda do mapa; ver `Creature::flying`). Onde pisa: `Species::terrain` (terra, anfíbio, só lagoa, mar, debaixo da areia). Criaturas são referenciadas por `id`, nunca por índice guardado.
- Voar não é salvo: ao carregar ou ao descer à força (morte, saída), voadores e jogadores vão para o chão livre mais próximo (`World::find_near`).
- Estrutura nova: `StructureKind` (com `piece`, `material` e `max_health`) + item + receita + `render::draw_structure` (e `ui::draw_piece_icon` se for peça de construção) + `render::structure_label` se for interativa. Estruturas são referenciadas por índice em `world.structures`: quem remove (demolir, destruir) fecha os painéis de estrutura abertos.
- Código e identificadores em inglês; textos do jogo e docs em português.
- Antes de concluir: `cargo test`, `cargo clippy` sem warnings e um `ISLAND_SHOT` quando mexer em render/UI.
- CI roda `fmt --check`, `clippy -D warnings` e testes. Publicar uma release dispara `release.yml` (builds de Windows/Linux/macOS).

Planejamento: ver [ROADMAP.md](ROADMAP.md).
