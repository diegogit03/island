//! Menu principal: nome do jogador, jogar sozinho, hospedar na LAN ou entrar num jogo
//! (lista de servidores encontrados na rede local ou endereço digitado).

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

use macroquad::prelude::*;

use crate::net::{self, Discovery};
use crate::protocol::{DEFAULT_PORT, MAX_NAME_LEN};

pub enum Choice {
    Solo,
    Host,
    Join(SocketAddr),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Address,
}

pub struct Menu {
    pub name: String,
    address: String,
    focus: Option<Field>,
    discovery: Discovery,
    local_ip: Option<IpAddr>,
    /// Erro da última tentativa (ou motivo do fim da partida anterior).
    message: Option<String>,
}

const TEXT: Color = Color::new(0.93, 0.93, 0.93, 1.0);
const DIM: Color = Color::new(0.65, 0.65, 0.68, 1.0);
const ACCENT: Color = Color::new(1.0, 0.80, 0.35, 1.0);
const BAD: Color = Color::new(0.95, 0.45, 0.40, 1.0);
const FIELD_W: f32 = 360.0;

/// Elementos clicáveis do menu, calculados pelo tamanho da tela.
struct Layout {
    name: Rect,
    solo: Rect,
    host: Rect,
    address: Rect,
    join: Rect,
    servers: Vec<Rect>,
}

fn layout(servers: usize) -> Layout {
    let cx = screen_width() / 2.0;
    let x = cx - FIELD_W / 2.0;
    let top = (screen_height() / 2.0 - 230.0).max(20.0);
    let half = (FIELD_W - 12.0) / 2.0;
    Layout {
        name: Rect::new(x, top + 120.0, FIELD_W, 36.0),
        solo: Rect::new(x, top + 176.0, half, 44.0),
        host: Rect::new(x + half + 12.0, top + 176.0, half, 44.0),
        address: Rect::new(x, top + 270.0, FIELD_W - 112.0, 36.0),
        join: Rect::new(x + FIELD_W - 100.0, top + 270.0, 100.0, 36.0),
        servers: (0..servers)
            .map(|i| Rect::new(x, top + 350.0 + i as f32 * 40.0, FIELD_W, 34.0))
            .collect(),
    }
}

/// Aceita "192.168.0.10", "192.168.0.10:7777" ou um nome de máquina.
fn parse_address(text: &str) -> Option<SocketAddr> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let with_port = if text.contains(':') {
        text.to_owned()
    } else {
        format!("{text}:{DEFAULT_PORT}")
    };
    with_port.to_socket_addrs().ok()?.find(|a| a.is_ipv4())
}

impl Menu {
    pub fn new(name: String, message: Option<String>) -> Self {
        Self {
            name,
            address: String::new(),
            focus: None,
            discovery: Discovery::new(),
            local_ip: net::local_ip(),
            message: message.filter(|m| !m.is_empty()),
        }
    }

    pub fn set_message(&mut self, message: String) {
        self.message = Some(message);
    }

    pub fn update(&mut self) -> Option<Choice> {
        self.discovery.poll();
        let l = layout(self.discovery.servers.len());
        let m: Vec2 = mouse_position().into();

        if is_mouse_button_pressed(MouseButton::Left) {
            self.focus = if l.name.contains(m) {
                Some(Field::Name)
            } else if l.address.contains(m) {
                Some(Field::Address)
            } else {
                None
            };
            if l.solo.contains(m) {
                return Some(Choice::Solo);
            }
            if l.host.contains(m) {
                return Some(Choice::Host);
            }
            if l.join.contains(m) {
                return self.join_typed();
            }
            if let Some(i) = l.servers.iter().position(|r| r.contains(m)) {
                return Some(Choice::Join(self.discovery.servers[i].addr));
            }
        }

        if let Some(field) = self.focus {
            let (text, max) = match field {
                Field::Name => (&mut self.name, MAX_NAME_LEN),
                Field::Address => (&mut self.address, 64),
            };
            while let Some(c) = get_char_pressed() {
                if !c.is_control() && text.chars().count() < max {
                    text.push(c);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                text.pop();
            }
            if is_key_pressed(KeyCode::Tab) {
                self.focus = Some(if field == Field::Name {
                    Field::Address
                } else {
                    Field::Name
                });
            }
            if field == Field::Address && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter)) {
                return self.join_typed();
            }
        } else {
            clear_input_queue();
        }
        None
    }

    fn join_typed(&mut self) -> Option<Choice> {
        match parse_address(&self.address) {
            Some(addr) => Some(Choice::Join(addr)),
            None => {
                self.message = Some("Endereço inválido. Ex.: 192.168.0.10".to_owned());
                None
            }
        }
    }

    pub fn draw(&self) {
        clear_background(Color::new(0.07, 0.16, 0.24, 1.0));
        let l = layout(self.discovery.servers.len());
        let m: Vec2 = mouse_position().into();
        let x = l.name.x;
        let top = l.name.y - 120.0;

        centered("Island", top + 56.0, 64.0, ACCENT);
        centered("experimento: survival com dinossauros", top + 84.0, 20.0, DIM);

        draw_text("Seu nome", x, l.name.y - 8.0, 18.0, DIM);
        text_field(l.name, &self.name, self.focus == Some(Field::Name));
        button(l.solo, "Um jogador", m);
        button(l.host, "Hospedar na LAN", m);

        draw_text("Entrar num jogo (IP do host)", x, l.address.y - 8.0, 18.0, DIM);
        text_field(l.address, &self.address, self.focus == Some(Field::Address));
        button(l.join, "Entrar", m);

        let list_title = if self.discovery.is_listening() {
            "Jogos na rede local:"
        } else {
            "Busca na rede indisponível (outro Island aberto?)"
        };
        draw_text(list_title, x, l.join.bottom() + 30.0, 18.0, DIM);
        if self.discovery.servers.is_empty() && self.discovery.is_listening() {
            draw_text("procurando...", x, l.join.bottom() + 54.0, 18.0, DIM);
        }
        for (rect, server) in l.servers.iter().zip(&self.discovery.servers) {
            let hover = rect.contains(m);
            draw_rectangle(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                Color::new(1.0, 1.0, 1.0, if hover { 0.18 } else { 0.08 }),
            );
            let label = format!(
                "{}  ({} jogador{})",
                server.name,
                server.players,
                if server.players == 1 { "" } else { "es" }
            );
            draw_text(&label, rect.x + 10.0, rect.y + 23.0, 20.0, TEXT);
            let ip = server.addr.ip().to_string();
            let w = measure_text(&ip, None, 16, 1.0).width;
            draw_text(&ip, rect.right() - w - 10.0, rect.y + 22.0, 16.0, DIM);
        }

        if let Some(msg) = &self.message {
            centered(msg, screen_height() - 60.0, 20.0, BAD);
        }
        let footer = match self.local_ip {
            Some(ip) => format!("Seu IP na rede local: {ip}  (porta {DEFAULT_PORT})"),
            None => "Sem rede local detectada".to_owned(),
        };
        centered(&footer, screen_height() - 24.0, 18.0, DIM);
    }
}

fn centered(text: &str, y: f32, size: f32, color: Color) {
    let w = measure_text(text, None, size as u16, 1.0).width;
    draw_text(text, (screen_width() - w) / 2.0, y, size, color);
}

fn text_field(rect: Rect, text: &str, focused: bool) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.0, 0.0, 0.0, 0.35));
    let border = if focused {
        ACCENT
    } else {
        Color::new(1.0, 1.0, 1.0, 0.25)
    };
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 2.0, border);
    let caret = if focused && (get_time() * 2.0) as i64 % 2 == 0 {
        "_"
    } else {
        ""
    };
    draw_text(format!("{text}{caret}"), rect.x + 10.0, rect.y + 25.0, 22.0, TEXT);
}

fn button(rect: Rect, label: &str, m: Vec2) {
    let bg = if rect.contains(m) {
        ACCENT
    } else {
        Color::new(0.85, 0.65, 0.25, 1.0)
    };
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, bg);
    let size = 22.0;
    let w = measure_text(label, None, size as u16, 1.0).width;
    draw_text(
        label,
        rect.x + (rect.w - w) / 2.0,
        rect.y + rect.h / 2.0 + 7.0,
        size,
        Color::new(0.1, 0.08, 0.05, 1.0),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_get_the_default_port() {
        assert_eq!(
            parse_address("192.168.0.10"),
            Some(SocketAddr::from(([192, 168, 0, 10], DEFAULT_PORT)))
        );
        assert_eq!(
            parse_address(" 10.0.0.2:9000 "),
            Some(SocketAddr::from(([10, 0, 0, 2], 9000)))
        );
        assert_eq!(parse_address(""), None);
    }
}
