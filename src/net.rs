use eframe::egui;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use tungstenite::{connect, Message};

pub const APP_VERSION: &str = "0.1.0";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SavedCredentials {
    pub server_url: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RoomStatus {
    pub room_id: String,
    pub players: usize,
    pub max_players: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserProfile {
    pub username: String,
    pub chips: i32,
    pub chess_wins: i32,
    pub solitaire_wins: i32,
    pub blackjack_wins: i32,
    pub ludo_wins: i32,
    pub war_wins: i32,
    pub battleship_wins: i32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LeaderboardRow {
    pub username: String,
    pub chips: i32,
    pub chess_wins: i32,
    pub solitaire_wins: i32,
    pub blackjack_wins: i32,
    pub ludo_wins: i32,
    pub war_wins: i32,
    pub battleship_wins: i32,
    pub total_wins: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NetEvent {
    Assigned { player_idx: usize, players_count: usize },
    PeerJoined { players_count: usize },
    ResetRequest { from_player: usize },
    ResetApproved,
    ResetDeclined,
    ChessMove { from: u32, to: u32 },
    LudoRoll { face: u8 },
    LudoMove { pawn_idx: usize },
    WarStep,
    BattleshipFleet { ships: Vec<(usize, usize, usize, bool)> },
    BattleshipShot { x: usize, y: usize },
}

pub struct NetClient {
    pub server_url: String,
    pub username_input: String,
    pub password_input: String,
    pub has_saved_account: bool,
    pub show_account_form: bool,
    pub current_user: Option<UserProfile>,
    pub auth_error: String,
    pub leaderboard: Vec<LeaderboardRow>,
    // --- AUTO-UPDATER ---
    pub available_update: Option<String>,
    pub update_status: String,
    // --- MULTIPLAYER POKOJE ---
    pub available_rooms: Vec<RoomStatus>,
    pub connected_room: Option<String>,
    pub my_player_idx: usize,
    pub room_players_count: usize,
    pub reset_requested_by_me: bool,
    pub incoming_reset_from: Option<usize>,
    ws_tx: Option<Sender<String>>,
    ws_rx: Option<Receiver<NetEvent>>,
}

impl Default for NetClient {
    fn default() -> Self {
        let mut client = Self {
            server_url: "http://127.0.0.1:47832".to_string(),
            username_input: String::new(),
            password_input: String::new(),
            has_saved_account: false,
            show_account_form: false,
            current_user: None,
            auth_error: String::new(),
            leaderboard: Vec::new(),
            available_update: None,
            update_status: String::new(),
            available_rooms: vec![
                RoomStatus {
                    room_id: "POKOJ1".to_string(),
                    players: 0,
                    max_players: 4,
                },
                RoomStatus {
                    room_id: "POKOJ2".to_string(),
                    players: 0,
                    max_players: 4,
                },
            ],
            connected_room: None,
            my_player_idx: 0,
            room_players_count: 1,
            reset_requested_by_me: false,
            incoming_reset_from: Option::None,
            ws_tx: None,
            ws_rx: None,
        };
        client.load_credentials();
        client
    }
}

impl NetClient {
    fn config_file_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let dir = PathBuf::from(home).join(".config");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("linux_game_hub_auth.json")
    }

  pub fn load_credentials(&mut self) {
        let path = Self::config_file_path();
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(creds) = serde_json::from_str::<SavedCredentials>(&data) {
                if !creds.username.is_empty() && !creds.password.is_empty() {
                    // Jeśli w starym konfigu był port 8000, automatycznie migrujemy na 47832
                    self.server_url = creds.server_url.replace(":8000", ":47832");
                    self.username_input = creds.username;
                    self.password_input = creds.password;
                    self.has_saved_account = true;
                    self.show_account_form = false;
                }
            }
        }
    }

    pub fn save_credentials(&mut self) {
        let creds = SavedCredentials {
            server_url: self.server_url.clone(),
            username: self.username_input.clone(),
            password: self.password_input.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&creds) {
            let _ = std::fs::write(Self::config_file_path(), json);
            self.has_saved_account = true;
            self.show_account_form = false;
        }
    }

    pub fn check_for_updates(&mut self) {
        let url = format!("{}/api/version", self.server_url.trim_end_matches('/'));
        if let Ok(resp) = ureq::get(&url).call() {
            if let Ok(val) = resp.into_json::<serde_json::Value>() {
                let srv_ver = val
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or(APP_VERSION)
                    .trim();
                let has_bin = val
                    .get("has_binary")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                if srv_ver != APP_VERSION && has_bin {
                    self.available_update = Some(srv_ver.to_string());
                } else {
                    self.available_update = None;
                }
            }
        }
    }

    pub fn perform_self_update(&mut self) {
        let url = format!(
            "{}/api/download_update",
            self.server_url.trim_end_matches('/')
        );
        let Ok(resp) = ureq::get(&url).call() else {
            self.update_status = "❌ Błąd pobierania aktualizacji z serwera!".to_string();
            return;
        };

        let mut bytes = Vec::new();
        if resp.into_reader().read_to_end(&mut bytes).is_err() || bytes.len() < 1024 {
            self.update_status = "❌ Pobrany plik jest niekompletny!".to_string();
            return;
        }

        let target_path = std::env::var("APPIMAGE")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok());

        let Some(target) = target_path else {
            self.update_status = "❌ Nie można ustalić ścieżki pliku gry.".to_string();
            return;
        };

        let new_bin_tmp = target.with_extension("new_version");
        if std::fs::write(&new_bin_tmp, &bytes).is_err() {
            self.update_status = "❌ Brak uprawnień do zapisu w tym katalogu!".to_string();
            return;
        }
        let _ = std::fs::set_permissions(&new_bin_tmp, std::fs::Permissions::from_mode(0o755));

        let updater_script = "/tmp/lgh_updater.sh";
        let script_content = format!(
            "#!/usr/bin/env bash\nsleep 0.4\nmv -f \"{}\" \"{}\"\nchmod +x \"{}\"\nexec \"{}\" &\n",
            new_bin_tmp.display(),
            target.display(),
            target.display(),
            target.display()
        );

        if std::fs::write(updater_script, script_content).is_ok() {
            let _ =
                std::fs::set_permissions(updater_script, std::fs::Permissions::from_mode(0o755));
            let _ = std::process::Command::new("bash")
                .arg(updater_script)
                .spawn();
            std::process::exit(0);
        } else {
            self.update_status = "❌ Błąd uruchamiania instalatora aktualizacji.".to_string();
        }
    }

    pub fn fetch_rooms(&mut self) {
        let url = format!("{}/api/rooms", self.server_url.trim_end_matches('/'));
        if let Ok(resp) = ureq::get(&url).call() {
            if let Ok(list) = resp.into_json::<Vec<RoomStatus>>() {
                if !list.is_empty() {
                    self.available_rooms = list;
                }
            }
        }
    }

    pub fn connect_to_room(&mut self, room_id: &str) -> bool {
        self.disconnect_room();
        let room = room_id.trim().to_uppercase();
        if room.is_empty() {
            return false;
        }

        let ws_base = self
            .server_url
            .trim_end_matches('/')
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        let url = format!("{}/ws/{}", ws_base, room);

        let Ok((mut socket, _)) = connect(&url) else {
            self.auth_error = format!("Nie udało się połączyć z pokojem {}!", room);
            return false;
        };

        if let tungstenite::stream::MaybeTlsStream::Plain(s) = socket.get_mut() {
            let _ = s.set_nonblocking(true);
        }

        let (out_tx, out_rx) = mpsc::channel::<String>();
        let (in_tx, in_rx) = mpsc::channel::<NetEvent>();

        std::thread::spawn(move || loop {
            loop {
                match out_rx.try_recv() {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg)).is_err() {
                            return;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        let _ = socket.close(None);
                        return;
                    }
                }
            }

            match socket.read() {
                Ok(Message::Text(txt)) => {
                    if let Ok(ev) = serde_json::from_str::<NetEvent>(&txt) {
                        let _ = in_tx.send(ev);
                    }
                }
                Ok(Message::Close(_)) => return,
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return,
                _ => {}
            }

            std::thread::sleep(std::time::Duration::from_millis(12));
        });

        self.connected_room = Some(room);
        self.reset_requested_by_me = false;
        self.incoming_reset_from = None;
        self.ws_tx = Some(out_tx);
        self.ws_rx = Some(in_rx);
        true
    }

    pub fn disconnect_room(&mut self) {
        self.connected_room = None;
        self.ws_tx = None;
        self.ws_rx = None;
        self.my_player_idx = 0;
        self.room_players_count = 1;
        self.reset_requested_by_me = false;
        self.incoming_reset_from = None;
    }

    pub fn send_event(&self, ev: &NetEvent) {
        if let Some(tx) = &self.ws_tx {
            if let Ok(json) = serde_json::to_string(ev) {
                let _ = tx.send(json);
            }
        }
    }

    pub fn poll_events(&mut self) -> Vec<NetEvent> {
        let mut events = Vec::new();
        if let Some(rx) = &self.ws_rx {
            while let Ok(ev) = rx.try_recv() {
                match &ev {
                    NetEvent::Assigned {
                        player_idx,
                        players_count,
                    } => {
                        self.my_player_idx = *player_idx;
                        self.room_players_count = *players_count;
                    }
                    NetEvent::PeerJoined { players_count } => {
                        self.room_players_count = *players_count;
                    }
                    NetEvent::ResetRequest { from_player } => {
                        self.incoming_reset_from = Some(*from_player);
                    }
                    NetEvent::ResetApproved | NetEvent::ResetDeclined => {
                        self.reset_requested_by_me = false;
                        self.incoming_reset_from = None;
                    }
                    _ => {}
                }
                events.push(ev);
            }
        }
        events
    }

    pub fn login_or_register(&mut self, is_register: bool) -> bool {
        let endpoint = if is_register {
            "/api/register"
        } else {
            "/api/login"
        };
        let url = format!("{}{}", self.server_url.trim_end_matches('/'), endpoint);

        let payload = serde_json::json!({
            "username": self.username_input,
            "password": self.password_input,
        });

        match ureq::post(&url).send_json(payload) {
            Ok(resp) => {
                if let Ok(profile) = resp.into_json::<UserProfile>() {
                    self.current_user = Some(profile);
                    self.auth_error.clear();
                    self.save_credentials();
                    self.fetch_leaderboard();
                    self.fetch_rooms();
                    self.check_for_updates();
                    return true;
                }
                self.auth_error = "Błąd parsowania odpowiedzi serwera.".to_string();
                false
            }
            Err(ureq::Error::Status(_code, resp)) => {
                if let Ok(val) = resp.into_json::<serde_json::Value>() {
                    if let Some(detail) = val.get("detail").and_then(|d| d.as_str()) {
                        self.auth_error = detail.to_string();
                        return false;
                    }
                }
                self.auth_error = "Błąd autoryzacji.".to_string();
                false
            }
            Err(_) => {
                self.auth_error =
                    "Brak połączenia z serwerem! Upewnij się, że działa server.py.".to_string();
                false
            }
        }
    }

    pub fn report_progress(&mut self, game: &str, win_delta: i32, new_chips: Option<i32>) {
        let Some(user) = &self.current_user else { return };
        let url = format!("{}/api/stats/update", self.server_url.trim_end_matches('/'));

        let payload = serde_json::json!({
            "username": user.username,
            "password": self.password_input,
            "game": game,
            "win_delta": win_delta,
            "chips": new_chips,
        });

        if let Ok(resp) = ureq::post(&url).send_json(payload) {
            if let Ok(updated) = resp.into_json::<UserProfile>() {
                self.current_user = Some(updated);
            }
        }
    }

    pub fn fetch_leaderboard(&mut self) {
        let url = format!("{}/api/leaderboard", self.server_url.trim_end_matches('/'));
        if let Ok(resp) = ureq::get(&url).call() {
            if let Ok(rows) = resp.into_json::<Vec<LeaderboardRow>>() {
                self.leaderboard = rows;
            }
        }
    }

    pub fn render_leaderboard_ui(&mut self, ui: &mut egui::Ui) {
        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(22, 25, 34));

        let margin = (full_rect.width() * 0.025).clamp(14.0, 32.0);
        let inner_rect = full_rect.shrink(margin);

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child_ui;

        let avail_w = ui.available_width();
        let scale = (avail_w / 1100.0).clamp(0.78, 1.35);
        let header_font = (16.5 * scale).clamp(13.0, 22.0);
        let row_font = (16.0 * scale).clamp(12.5, 21.0);

        ui.horizontal(|ui| {
            ui.heading(
                egui::RichText::new("🏆 Rankingi Graczy Online")
                    .size((28.0 * scale).clamp(20.0, 36.0))
                    .strong()
                    .color(egui::Color32::GOLD),
            );
            ui.add_space(16.0);
            if ui
                .button(egui::RichText::new("🔄 Odśwież tabelę").size(header_font))
                .clicked()
            {
                self.fetch_leaderboard();
            }
        });

        ui.add_space(14.0 * scale);

        let col_spacing = (14.0 * scale).clamp(8.0, 24.0);
        let col_width = ((avail_w - col_spacing * 9.0) / 10.2).max(55.0);

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(30, 34, 46))
            .rounding(10.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("leaderboard_grid")
                            .striped(true)
                            .min_col_width(col_width)
                            .spacing([col_spacing, 12.0 * scale])
                            .show(ui, |ui| {
                                let headers = [
                                    "Miejsce",
                                    "Gracz",
                                    "🪙 Żetony",
                                    "♟ Szachy",
                                    "🚢 Statki",
                                    "🎲 Chińczyk",
                                    "🃏 Pasjans",
                                    "🎰 Blackjack",
                                    "⚔ Wojna",
                                    "🏆 SUMA",
                                ];
                                for h in headers {
                                    ui.label(
                                        egui::RichText::new(h)
                                            .size(header_font)
                                            .strong()
                                            .color(egui::Color32::from_rgb(140, 200, 255)),
                                    );
                                }
                                ui.end_row();

                                for (idx, row) in self.leaderboard.iter().enumerate() {
                                    let medal = match idx {
                                        0 => "🥇 1.".to_string(),
                                        1 => "🥈 2.".to_string(),
                                        2 => "🥉 3.".to_string(),
                                        _ => format!("{}.", idx + 1),
                                    };
                                    ui.label(egui::RichText::new(medal).size(row_font).strong());
                                    ui.label(
                                        egui::RichText::new(&row.username)
                                            .size(row_font)
                                            .strong()
                                            .color(egui::Color32::WHITE),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{}", row.chips))
                                            .size(row_font)
                                            .strong()
                                            .color(egui::Color32::GOLD),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{}", row.chess_wins))
                                            .size(row_font),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{}", row.battleship_wins))
                                            .size(row_font),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{}", row.ludo_wins))
                                            .size(row_font),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{}", row.solitaire_wins))
                                            .size(row_font),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{}", row.blackjack_wins))
                                            .size(row_font),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{}", row.war_wins))
                                            .size(row_font),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{}", row.total_wins))
                                            .size(row_font)
                                            .strong()
                                            .color(egui::Color32::from_rgb(100, 240, 140)),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
            });
    }
}