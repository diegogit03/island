# Roadmap — Island (sugestão)

Cada marco é jogável por si só. A ordem prioriza o loop central de survival antes de dinossauros, dinossauros antes de rede,
e rede antes do polimento (arte, áudio e conteúdo novo são feitos já sobre a arquitetura multiplayer).

## ✅ v0.1 — Fundação
- Personagem com movimento 8 direções e sprite esquerda/direita
- Ilha procedural com praia, água rasa e funda
- Coleta: árvores (madeira/palha), rochas (pedra/sílex), pedrinhas no chão
- Inventário de 30 slots com hotbar, empilhamento, arrastar/dividir pilhas
- Crafting: picareta, machado, lança, fogueira
- Ferramentas alteram o rendimento (estilo ARK)

## ✅ v0.2 — Sobrevivência básica
- Atributos: vida, comida, água, stamina (correr com Shift, exaustão)
- Arbustos de frutinhas (comer com R / botão direito), beber água com E na beira
- Morte e respawn na praia, com bolsa dos itens (e feixe de luz) no local da morte
- Durabilidade de ferramentas (barra no slot, quebram ao zerar)
- Largar itens (Q / Ctrl+Q / soltar fora do inventário) e pegar de volta (F)
- Save/load em JSON (F5, autosave a cada 60 s, ao fechar a janela)

## ✅ v0.3 — Construção
- Estruturas em grid com prévia verde/vermelha: fundação, parede e porta de madeira (paredes em 3/4)
- Fogueira que queima madeira e ilumina a noite
- Caixa de armazenamento (18 slots, Shift+clique transfere)
- Bancada: receitas de construção exigem estar perto dela
- Fila de crafting com tempo por receita (cancelável, devolve materiais)
- Ciclo dia/noite (12 min reais) com iluminação suave e entardecer alaranjado
- Demolir estruturas (X) devolve o item

## ✅ v0.4 — Fauna e combate
- Criaturas passivas (dodô, trilobita) e agressivas (dilofossauro, raptor) com IA simples (vagar, fugir, perseguir, atacar)
- Combate corpo a corpo com dano por arma (lança: mais dano e alcance) e durabilidade
- Carcaças coletáveis: machado rende carne, picareta rende couro
- Fogueira com inventário: queima madeira/palha e cozinha carne (carne crua faz mal)
- Spawn por bioma (praia, campo, floresta), limite de população e zonas seguras (spawn e bases)
- Ainda não: estragar comida e armaduras

## ✅ v0.5 — Domesticação ⭐
- Torpor e desmaio: clava (corpo a corpo) e estilingue (pedras, mira no cursor); torpor some com o tempo
- Narcóticos no inventário do dino desacordado o mantêm dormindo
- Domesticação alimentando o dino desacordado (herbívoros: frutinhas; carnívoros: carne crua > cozida),
  com efetividade que cai a cada golpe e define os níveis bônus
- Dinos domesticados: seguir/ficar, inventário de carga, regeneração de vida
- Montaria: raptor com Sela de Raptor (feita de couro na bancada); a montaria morde e absorve o dano
- Níveis 1–30 na natureza, escalando vida, torpor e dano
- Ainda não: domesticados lutando sozinhos, fome/ração de domesticados, XP de criaturas

## ✅ v0.6 — Progressão RPG
- XP e nível do jogador (até 30): coleta, crafting, abates e domesticação dão XP
- Pontos de atributo: vida, stamina, comida, água e dano corpo a corpo
- Engramas: receitas têm nível mínimo e custo em pontos; clicar na receita com cadeado aprende
- Bioma de montanha no interior da ilha, com rochas de metal (picareta rende minério)
- Tier de metal: forja (funde 2 minérios em 1 lingote), picareta, machado (rendem o dobro) e espada de metal
- Saves antigos mantêm as receitas que já existiam liberadas
- Movido para a v0.8: biomas de selva e pântano, cavernas

## ✅ v0.7 — Multiplayer LAN
- Servidor autoritativo com tick fixo de 60 Hz; o cliente só desenha e envia input/comandos
- Um jogador usa o mesmo servidor rodando localmente (sem rede); "Hospedar na LAN" abre o mesmo mundo para outros
- Rede com TCP (`std::net` + bincode) e descoberta automática de servidores na LAN por broadcast UDP
- Servidor dedicado sem janela: `island --server` (base para servidores dedicados de verdade)
- Menu principal: nome, um jogador, hospedar, entrar por IP ou pela lista da rede local
- Vários jogadores no mesmo mundo: nomes e cores, chat (Enter), criaturas caçam o jogador mais perto
- Co-op total: estruturas e baús são de todos; domesticados seguem o dono (quem manda "seguir")
- Save v2 com todos os jogadores (cada um volta como estava ao entrar com o mesmo nome); saves antigos viram o jogador do host
- Decisão: continuar no macroquad (a rede independe da engine; Bevy exigiria reescrever render e UI)
- Ainda não: predição no cliente (pensado para LAN; pela internet a latência aparece), tribos separadas e PvP

## ✅ v0.8 — Expansão dos biomas
- ✅ Criaturas voadoras montáveis com sela: **pteranodonte** (o mais rápido do jogo, carrega pouco) e **argentavis** (velocidade média, carrega muito; caça e pousa para morder)
  - Espaço decola/pousa; no ar ignora árvores, pedras, construções e água; predadores do chão não alcançam quem voa
  - Pouso forçado em terra ao morrer, sair do jogo ou perder a montaria no ar
- ✅ Peso de carga dos dinos (cada item tem peso; acima do limite o dino fica lento e não decola)
- ✅ Bioma de gelo no norte da ilha (neve e pinheiros nevados) com fauna da era do gelo, toda domesticável:
  - **Mamute** (territorial: revida quando atacado); montado, derruba árvores e enche a carga de madeira
  - **Smilodon** (agressivo); montado, tira muito couro das carcaças
  - **Lobo-cinzento** (agressivo, anda em alcateia de até 3)
- ✅ Bioma de pântano no leste da ilha (lama, juncos, lagoas com vitórias-régias, árvores com musgo):
  - **Beelzebufo** (passivo); montado, Espaço pula por cima de árvores e água
  - **Sarcosuchus** (agressivo); montável, mais rápido nadando
  - **Titanoboa** (agressiva, domesticável, não montável)
  - **Piranhas** em cardume nas lagoas (não domesticáveis): cuidado ao entrar na água
  - Anfíbios andam na lama e nadam nas lagoas (e na água rasa do mar); bichos de terra não entram
- ✅ Expansão aquática (mar):
  - **Jangada**: fabricada sem bancada, posta na água com o botão direito; F sobe/desce (só perto da margem), Shift rema mais rápido; dá para bater, atirar e beber de cima dela
  - **Ictiossauro** (passivo, o mais rápido no mar), **Megalodonte** (agressivo), **Kraken** e **Mosassauro** (agressivos, em mar aberto, longe da costa): todos domesticáveis e montáveis com sela
  - Selvagens do mar ficam na água funda; montado no mar, só desce perto da margem ou de uma jangada
- ✅ Bioma de deserto no oeste da ilha (dunas, cactos, palmeiras, árvores secas e oásis); bater num cacto mata um pouco a sede:
  - **Golem de pedra** (territorial): parado, parece uma rocha; revida quando atacado. Domesticável com pedra ou minério de metal; abatido, vira pedra
  - **Canguru** (passivo): domesticável e montável; Espaço pula
  - **Louva-deus** (agressivo, domesticável)
  - **Verme da morte** (agressivo, não domesticável): anda enterrado, ninguém acerta; só sai da areia para morder

## v0.9 — NPCs e tribos
- Acampamentos de NPCs espalhados pelo mapa (cabanas e fogueira), onde dá para recrutar NPCs para a sua tribo
- Membros da tribo com nome, vida e inventário; o jogador dá ferramentas e armas para eles
- Comandos: seguir, ficar, coletar (madeira, pedra...) e guardar no baú, defender, atacar um alvo
- NPCs seguem as mesmas regras de coleta e combate do jogador (a simulação no servidor já trata cada um como um agente)
- Domesticados também passam a lutar junto com o dono e a obedecer aos comandos de combate
- Multiplayer: a tribo é de quem recrutou; base para as tribos entre jogadores da v1.0

## v0.10 — Construções
- Tiers de pedra e metal (fundações, paredes, portas), teto e andares
- Portões grandes (para dinos), janelas e cercas
- Cama como ponto de respawn, móveis e decoração
- Vida das estruturas e reparo; criaturas e NPCs hostis atacam bases

## v0.11 — Industrialização
- Cadeias de produção: moedor, curtume, fornalha industrial, refino de combustível
- Estações que trabalham sozinhas com combustível; NPCs da tribo operando estações
- Gerador e energia para máquinas
- Agricultura: canteiros, sementes e ração para domesticados

## v0.12 — Armas
- Arco e flechas (inclusive tranquilizantes), besta e lanças de arremesso
- Armas de fogo simples com munição feita na indústria
- Armaduras (couro, pele, metal) e escudos
- Torretas de defesa para a base

## Polimento e conteúdo (depois da v0.12)
- Sprites pixel art substituindo as primitivas (spritesheets + animações)
- Áudio (macroquad já suporta), menus, opções, rebind de teclas
- Bioma de selva, cavernas
- Temperatura: frio no gelo e calor no deserto (roupas, fogueira esquenta)
- Mapa/minimapa, notas exploráveis, chefes simples

## v1.0+ — MMO
- Predição no cliente e reconciliação (jogar pela internet, não só LAN), snapshots com delta
- Tribos entre jogadores (posse de estruturas e dinos por grupo), PvP configurável
- Servidores persistentes com banco de dados e muitos jogadores
- Mundos maiores em chunks, várias ilhas
- Economia entre jogadores, PvE/PvP por servidor

## Decisões técnicas a revisitar
- **Engine:** na v0.7 a simulação foi separada do cliente sem trocar de engine; o servidor não depende do macroquad para nada além de tipos de matemática. Bevy só se o volume de entidades/sistemas pedir ECS.
- **Rede:** TCP + snapshots completos funcionam bem em LAN; para internet, trocar para UDP (ex.: `renet`) com deltas e predição.
- **Mapa:** hoje é uma ilha 160×160 tiles em memória; mundos maiores pedem chunks.
- **Dados:** itens/receitas em código agora; mover para arquivos (RON) quando houver muitos.
