mod audio;
mod battleship;
mod blackjack;
mod cards;
mod chess;
mod ludo;
mod net;
mod solitaire;
mod war;

use audio::{Sfx, SoundManager};
use eframe::egui;
use net::{NetClient, NetEvent};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([880.0, 620.0])
            .with_title("Fiksiarski Game Hub"),
        ..Default::default()
    };

    eframe::run_native(
        "Fiksiarski Game Hub",
        options,
        Box::new(|cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.visuals = egui::Visuals::dark();
            style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
            style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
            style.visuals.widgets.hovered.rounding = egui::Rounding::same(10.0);
            cc.egui_ctx.set_style(style);

            Ok(Box::new(GameHubApp::default()))
        }),
    )
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Screen {
    #[default]
    Login,
    RoomSelect,
    Hub,
    Leaderboard,
    Chess,
    Solitaire,
    Blackjack,
    Ludo,
    War,
    Battleship,
}

#[derive(Default)]
struct GameHubApp {
    current_screen: Screen,
    is_online_session: bool,
    audio: SoundManager,
    net: NetClient,
    last_synced_chips: i32,
    blackjack: blackjack::BlackjackGame,
    solitaire: solitaire::SolitaireGame,
    chess: chess::ChessGame,
    ludo: ludo::LudoGame,
    war: war::WarGame,
    battleship: battleship::BattleshipGame,
}

impl GameHubApp {
    fn reset_all_online_games(&mut self) {
        let my_idx = self.net.my_player_idx;
        let count = self.net.room_players_count;
        self.chess.setup_online(my_idx);
        self.ludo.setup_online(my_idx, count);
        self.battleship.setup_online(my_idx);
        self.war.new_game();
    }
}

impl eframe::App for GameHubApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.is_online_session && self.net.connected_room.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
            let events = self.net.poll_events();
            for ev in events {
                match ev {
                    NetEvent::Assigned {
                        player_idx,
                        players_count,
                    } => {
                        self.chess.setup_online(player_idx);
                        self.ludo.setup_online(player_idx, players_count);
                        self.battleship.setup_online(player_idx);
                    }
                    NetEvent::PeerJoined { players_count } => {
                        self.audio.play(Sfx::Check);
                        self.chess.setup_online(self.net.my_player_idx);
                        self.ludo
                            .setup_online(self.net.my_player_idx, players_count);
                        self.battleship.setup_online(self.net.my_player_idx);
                    }
                    NetEvent::ResetRequest { .. } => {
                        self.audio.play(Sfx::Check);
                    }
                    NetEvent::ResetApproved => {
                        self.audio.play(Sfx::CardSlide);
                        self.reset_all_online_games();
                    }
                    NetEvent::ResetDeclined => {
                        self.audio.play(Sfx::Lose);
                    }
                    NetEvent::ChessMove { from, to } => {
                        self.chess.apply_remote_move(from, to, &self.audio);
                    }
                    NetEvent::LudoRoll { face } => {
                        self.ludo.apply_remote_roll(face, &self.audio);
                    }
                    NetEvent::LudoMove { pawn_idx } => {
                        self.ludo.apply_remote_pawn_move(pawn_idx, &self.audio);
                    }
                    NetEvent::WarStep => {
                        self.war.play_step(&self.audio);
                    }
                    NetEvent::BattleshipFleet { ships } => {
                        self.battleship.receive_enemy_fleet(ships);
                    }
                    NetEvent::BattleshipShot { x, y } => {
                        self.battleship.receive_enemy_shot(x, y, &self.audio);
                    }
                }
            }

            if let Some((from, to)) = self.chess.outgoing_move.take() {
                self.net.send_event(&NetEvent::ChessMove { from, to });
            }
            if let Some(face) = self.ludo.outgoing_roll.take() {
                self.net.send_event(&NetEvent::LudoRoll { face });
            }
            if let Some(pawn_idx) = self.ludo.outgoing_pawn.take() {
                self.net.send_event(&NetEvent::LudoMove { pawn_idx });
            }
            if self.war.outgoing_step {
                self.war.outgoing_step = false;
                self.net.send_event(&NetEvent::WarStep);
            }
            if let Some(ships) = self.battleship.outgoing_fleet.take() {
                self.net.send_event(&NetEvent::BattleshipFleet { ships });
            }
            if let Some((x, y)) = self.battleship.outgoing_shot.take() {
                self.net.send_event(&NetEvent::BattleshipShot { x, y });
            }
        }

        let current_bj_chips = self.blackjack.chips();
        if self.is_online_session
            && self.net.current_user.is_some()
            && current_bj_chips != self.last_synced_chips
        {
            let won_round = if current_bj_chips > self.last_synced_chips {
                1
            } else {
                0
            };
            self.last_synced_chips = current_bj_chips;
            self.net
                .report_progress("blackjack", won_round, Some(current_bj_chips));
        }

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new(format!("🎮 Fiksiarski Game Hub v{}", net::APP_VERSION))
                        .size(20.0)
                        .strong(),
                );

                if let Some(new_ver) = self.net.available_update.clone() {
                    ui.separator();
                    if ui
                        .button(
                            egui::RichText::new(format!("⬇ Aktualizuj do v{}!", new_ver))
                                .size(14.0)
                                .strong()
                                .color(egui::Color32::GOLD),
                        )
                        .clicked()
                    {
                        self.net.perform_self_update();
                    }
                }
                if !self.net.update_status.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.net.update_status)
                            .size(13.0)
                            .color(egui::Color32::from_rgb(255, 100, 100)),
                    );
                }

                if !matches!(
                    self.current_screen,
                    Screen::Login | Screen::RoomSelect | Screen::Hub
                ) {
                    ui.separator();
                    if ui
                        .button(egui::RichText::new("⬅ Menu Gier").size(15.0))
                        .clicked()
                    {
                        self.audio.play(Sfx::Click);
                        self.current_screen = Screen::Hub;
                    }
                }

                if !matches!(self.current_screen, Screen::Login | Screen::RoomSelect)
                    && self.is_online_session
                {
                    ui.separator();
                    if ui
                        .button(egui::RichText::new("🏆 Rankingi").size(15.0))
                        .clicked()
                    {
                        self.audio.play(Sfx::Click);
                        self.net.fetch_leaderboard();
                        self.current_screen = Screen::Leaderboard;
                    }

                    ui.separator();
                    if let Some(room) = &self.net.connected_room.clone() {
                        ui.label(
                            egui::RichText::new(format!(
                                "🟢 {} (Miejsce #{}, graczy: {}/4)",
                                room,
                                self.net.my_player_idx + 1,
                                self.net.room_players_count
                            ))
                            .size(14.0)
                            .strong()
                            .color(egui::Color32::from_rgb(80, 240, 120)),
                        );

                        if let Some(requester) = self.net.incoming_reset_from {
                            ui.separator();
                            ui.label(
                                egui::RichText::new(format!(
                                    "⚠ Gracz #{} chce zresetować grę!",
                                    requester + 1
                                ))
                                .strong()
                                .color(egui::Color32::GOLD),
                            );
                            if ui
                                .button(
                                    egui::RichText::new("✅ Zgoda")
                                        .color(egui::Color32::from_rgb(80, 245, 120)),
                                )
                                .clicked()
                            {
                                self.net.incoming_reset_from = None;
                                self.net.send_event(&NetEvent::ResetApproved);
                                self.audio.play(Sfx::CardSlide);
                                self.reset_all_online_games();
                            }
                            if ui
                                .button(
                                    egui::RichText::new("❌ Odrzuć")
                                        .color(egui::Color32::from_rgb(255, 95, 95)),
                                )
                                .clicked()
                            {
                                self.net.incoming_reset_from = None;
                                self.net.send_event(&NetEvent::ResetDeclined);
                                self.audio.play(Sfx::Click);
                            }
                        } else if self.net.reset_requested_by_me {
                            ui.separator();
                            ui.label(
                                egui::RichText::new("⏳ Czekanie na zgodę...")
                                    .color(egui::Color32::GOLD),
                            );
                        } else if self.net.room_players_count >= 2 {
                            if ui.button("🔄 Resetuj grę").clicked() {
                                self.audio.play(Sfx::Click);
                                self.net.reset_requested_by_me = true;
                                self.net.send_event(&NetEvent::ResetRequest {
                                    from_player: self.net.my_player_idx,
                                });
                            }
                        }

                        if ui.button("🚪 Zmień pokój").clicked() {
                            self.audio.play(Sfx::Click);
                            self.net.disconnect_room();
                            self.chess.online_mode = false;
                            self.ludo.online_mode = false;
                            self.battleship.online_mode = false;
                            self.net.fetch_rooms();
                            self.current_screen = Screen::RoomSelect;
                        }
                    } else if ui.button("🌐 Wybierz pokój MP").clicked() {
                        self.audio.play(Sfx::Click);
                        self.net.fetch_rooms();
                        self.current_screen = Screen::RoomSelect;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mute_label = if self.audio.muted { "🔇" } else { "🔊" };
                    if ui
                        .button(egui::RichText::new(mute_label).size(15.0))
                        .clicked()
                    {
                        self.audio.muted = !self.audio.muted;
                        if !self.audio.muted {
                            self.audio.play(Sfx::Click);
                        }
                    }

                    if self.current_screen != Screen::Login {
                        ui.separator();
                        if ui.button("🔄 Tryb / Konto").clicked() {
                            self.audio.play(Sfx::Click);
                            self.net.disconnect_room();
                            self.chess.online_mode = false;
                            self.ludo.online_mode = false;
                            self.battleship.online_mode = false;
                            self.is_online_session = false;
                            self.current_screen = Screen::Login;
                        }

                        if let Some(user) = &self.net.current_user.clone() {
                            if self.is_online_session {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "👤 {} | 🪙 {}",
                                        user.username, user.chips
                                    ))
                                    .size(14.0)
                                    .strong()
                                    .color(egui::Color32::GOLD),
                                );
                            }
                        } else {
                            ui.label(
                                egui::RichText::new("🕹 Tryb Offline")
                                    .size(14.0)
                                    .color(egui::Color32::LIGHT_GRAY),
                            );
                        }
                    }
                });
            });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.current_screen {
                Screen::Login => self.render_login(ui),
                Screen::RoomSelect => self.render_room_select(ui),
                Screen::Hub => self.render_hub(ui),
                Screen::Leaderboard => self.net.render_leaderboard_ui(ui),
                Screen::Blackjack => self.blackjack.ui(ui, &self.audio),
                Screen::Solitaire => {
                    self.solitaire.ui(ui, &self.audio);
                    if self.solitaire.newly_won {
                        self.solitaire.newly_won = false;
                        if self.is_online_session {
                            self.net.report_progress("solitaire", 1, None);
                        }
                    }
                }
                Screen::Chess => {
                    self.chess.ui(ui, &self.audio);
                    if self.chess.newly_won {
                        self.chess.newly_won = false;
                        if self.is_online_session {
                            self.net.report_progress("chess", 1, None);
                        }
                    }
                }
                Screen::Ludo => {
                    self.ludo.ui(ui, &self.audio);
                    if self.ludo.newly_won {
                        self.ludo.newly_won = false;
                        if self.is_online_session {
                            self.net.report_progress("ludo", 1, None);
                        }
                    }
                }
                Screen::War => {
                    self.war.ui(ui, &self.audio);
                    if self.war.newly_won {
                        self.war.newly_won = false;
                        if self.is_online_session {
                            self.net.report_progress("war", 1, None);
                        }
                    }
                }
                Screen::Battleship => {
                    self.battleship.ui(ui, &self.audio);
                    if self.battleship.newly_won {
                        self.battleship.newly_won = false;
                        if self.is_online_session {
                            self.net.report_progress("battleship", 1, None);
                        }
                    }
                }
            }
        });
    }
}

impl GameHubApp {
    fn render_login(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        ui.vertical_centered(|ui| {
            ui.add_space((avail.y * 0.16).max(30.0));

            egui::Frame::none()
                .fill(egui::Color32::from_rgb(32, 36, 46))
                .rounding(16.0)
                .inner_margin(32.0)
                .stroke(egui::Stroke::new(
                    1.5_f32,
                    egui::Color32::from_rgb(70, 80, 105),
                ))
                .show(ui, |ui| {
                    ui.set_width(410.0);

                    if self.net.has_saved_account && !self.net.show_account_form {
                        ui.heading(
                            egui::RichText::new(format!(
                                "👋 Witaj ponownie, {}!",
                                self.net.username_input
                            ))
                            .size(27.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                        );
                        ui.add_space(8.0);
                        ui.label("Wybierz w jakim trybie chcesz dzisiaj grać:");
                        ui.add_space(22.0);

                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(
                                        "🌐 Graj Online (Wybierz Pokój, Rankingi)",
                                    )
                                    .size(17.0)
                                    .strong(),
                                )
                                .min_size(egui::vec2(360.0, 48.0)),
                            )
                            .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            if self.net.login_or_register(false) {
                                if let Some(u) = &self.net.current_user {
                                    self.blackjack.set_chips(u.chips);
                                    self.last_synced_chips = u.chips;
                                }
                                self.is_online_session = true;
                                self.net.fetch_rooms();
                                self.current_screen = Screen::RoomSelect;
                            }
                        }

                        ui.add_space(12.0);

                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("🕹 Graj Offline (Lokalnie z Botami)")
                                        .size(17.0),
                                )
                                .min_size(egui::vec2(360.0, 46.0)),
                            )
                            .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.is_online_session = false;
                            self.net.current_user = None;
                            self.net.check_for_updates();
                            self.current_screen = Screen::Hub;
                        }

                        if !self.net.auth_error.is_empty() {
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new(&self.net.auth_error)
                                    .color(egui::Color32::from_rgb(255, 95, 95)),
                            );
                        }

                        ui.add_space(18.0);
                        ui.separator();
                        ui.add_space(8.0);

                        if ui
                            .button(
                                egui::RichText::new("🔄 Zmień konto / adres serwera")
                                    .size(14.0)
                                    .color(egui::Color32::LIGHT_GRAY),
                            )
                            .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.net.show_account_form = true;
                        }
                    } else {
                        ui.heading(
                            egui::RichText::new("🔐 Konto Gracza")
                                .size(28.0)
                                .strong()
                                .color(egui::Color32::WHITE),
                        );
                        ui.add_space(6.0);
                        ui.label("Zaloguj się lub utwórz konto (zostanie zapamiętane)");
                        ui.add_space(18.0);

                        ui.horizontal(|ui| {
                            ui.label("🌐 Serwer:");
                            ui.text_edit_singleline(&mut self.net.server_url);
                        });
                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            ui.label("👤 Login:  ");
                            ui.text_edit_singleline(&mut self.net.username_input);
                        });
                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            ui.label("🔑 Hasło:  ");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.net.password_input)
                                    .password(true),
                            );
                        });

                        if !self.net.auth_error.is_empty() {
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new(&self.net.auth_error)
                                    .color(egui::Color32::from_rgb(255, 95, 95)),
                            );
                        }

                        ui.add_space(20.0);

                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("🚀 Zaloguj się").size(17.0).strong(),
                                    )
                                    .min_size(egui::vec2(175.0, 42.0)),
                                )
                                .clicked()
                            {
                                self.audio.play(Sfx::Click);
                                if self.net.login_or_register(false) {
                                    if let Some(u) = &self.net.current_user {
                                        self.blackjack.set_chips(u.chips);
                                        self.last_synced_chips = u.chips;
                                    }
                                    self.is_online_session = true;
                                    self.net.fetch_rooms();
                                    self.current_screen = Screen::RoomSelect;
                                }
                            }

                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("✨ Załóż konto").size(17.0),
                                    )
                                    .min_size(egui::vec2(175.0, 42.0)),
                                )
                                .clicked()
                            {
                                self.audio.play(Sfx::Click);
                                if self.net.login_or_register(true) {
                                    if let Some(u) = &self.net.current_user {
                                        self.blackjack.set_chips(u.chips);
                                        self.last_synced_chips = u.chips;
                                    }
                                    self.is_online_session = true;
                                    self.net.fetch_rooms();
                                    self.current_screen = Screen::RoomSelect;
                                }
                            }
                        });

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(8.0);

                        if ui
                            .button(
                                egui::RichText::new("🕹 Graj Offline jako Gość (bez serwera)")
                                    .size(14.0)
                                    .color(egui::Color32::LIGHT_GRAY),
                            )
                            .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.is_online_session = false;
                            self.net.current_user = None;
                            self.net.check_for_updates();
                            self.current_screen = Screen::Hub;
                        }
                    }
                });
        });
    }

    fn render_room_select(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        let ui_scale = (avail.x / 1100.0).min(avail.y / 720.0).clamp(0.75, 1.3);

        ui.vertical_centered(|ui| {
            ui.add_space((avail.y * 0.12).max(20.0));

            ui.heading(
                egui::RichText::new("🌐 Wybierz Pokój Multiplayer")
                    .size((32.0 * ui_scale).clamp(22.0, 38.0))
                    .strong()
                    .color(egui::Color32::WHITE),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "Kliknij na wybrany pokój, aby usiąść przy stole i przejść do gier:",
                )
                .size((16.0 * ui_scale).clamp(13.0, 20.0))
                .color(egui::Color32::LIGHT_GRAY),
            );
            ui.add_space(14.0);

            if ui
                .button(egui::RichText::new("🔄 Odśwież stan pokoi").size(15.0))
                .clicked()
            {
                self.audio.play(Sfx::Click);
                self.net.fetch_rooms();
            }

            ui.add_space(22.0);

            let spacing = (avail.x * 0.03).clamp(16.0, 36.0);
            let card_w = ((avail.x * 0.82 - spacing) / 2.0).clamp(240.0, 420.0);
            let card_h = (avail.y * 0.30).clamp(140.0, 220.0);
            let total_w = card_w * 2.0 + spacing;
            let left_pad = ((avail.x - total_w) / 2.0).max(0.0);

            let rooms_snapshot = self.net.available_rooms.clone();
            let mut chosen_room: Option<String> = None;

            ui.horizontal(|ui| {
                ui.add_space(left_pad);

                for (idx, r) in rooms_snapshot.iter().enumerate() {
                    let is_full = r.players >= r.max_players;
                    let title = format!("🎲 Pokój #{} ({})", idx + 1, r.room_id);
                    let subtitle = if is_full {
                        format!("⛔ PEŁNY ({}/{} os.)", r.players, r.max_players)
                    } else if r.players == 0 {
                        format!("🟢 Wolny ({}/{} os.)", r.players, r.max_players)
                    } else {
                        format!("🔥 W pokoju: {}/{} os.", r.players, r.max_players)
                    };

                    let resp = Self::game_card(ui, egui::vec2(card_w, card_h), &title, &subtitle);
                    if resp.clicked() && !is_full {
                        chosen_room = Some(r.room_id.clone());
                    }
                    if idx == 0 {
                        ui.add_space(spacing);
                    }
                }
            });

            if let Some(room_id) = chosen_room {
                self.audio.play(Sfx::CardSlide);
                if self.net.connect_to_room(&room_id) {
                    self.current_screen = Screen::Hub;
                }
            }

            ui.add_space(28.0);

            if ui
                .button(
                    egui::RichText::new(
                        "👤 Graj Solo Online (Pasjans / Blackjack bez pokoju MP)",
                    )
                    .size((15.0 * ui_scale).clamp(12.5, 18.0))
                    .color(egui::Color32::from_rgb(150, 200, 255)),
                )
                .clicked()
            {
                self.audio.play(Sfx::Click);
                self.net.disconnect_room();
                self.current_screen = Screen::Hub;
            }
        });
    }

    fn render_hub(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();

        let spacing = (avail.x.min(avail.y) * 0.032).clamp(14.0, 36.0);
        let card_w = ((avail.x * 0.88 - spacing * 2.0) / 3.0).clamp(210.0, 420.0);
        let card_h = ((avail.y * 0.64 - spacing) / 2.0).clamp(130.0, 290.0);
        let card_size = egui::vec2(card_w, card_h);

        let title_size = (avail.y * 0.055).clamp(28.0, 52.0);
        let total_content_h = title_size + spacing * 2.1 + card_h * 2.0;
        let top_padding = ((avail.y - total_content_h) / 2.0).max(16.0);

        ui.vertical_centered(|ui| {
            ui.add_space(top_padding);
            ui.heading(
                egui::RichText::new("Wybierz grę")
                    .size(title_size)
                    .strong()
                    .color(egui::Color32::WHITE),
            );
            ui.add_space(spacing);

            let total_grid_w = card_w * 3.0 + spacing * 2.0;
            let left_pad = ((avail.x - total_grid_w) / 2.0).max(0.0);

            ui.horizontal(|ui| {
                ui.add_space(left_pad);

                egui::Grid::new("games_grid")
                    .spacing([spacing, spacing])
                    .show(ui, |ui| {
                        if Self::game_card(
                            ui,
                            card_size,
                            "♟ Szachy",
                            "Klasyczny pojedynek na szachownicy",
                        )
                        .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.current_screen = Screen::Chess;
                        }
                        if Self::game_card(
                            ui,
                            card_size,
                            "🚢 Statki",
                            "Taktyczna bitwa morska na radarze",
                        )
                        .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.current_screen = Screen::Battleship;
                        }
                        if Self::game_card(
                            ui,
                            card_size,
                            "🎲 Chińczyk",
                            "Planszówka Ludo dla 2-4 graczy",
                        )
                        .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.current_screen = Screen::Ludo;
                        }
                        ui.end_row();

                        if Self::game_card(
                            ui,
                            card_size,
                            "🃏 Pasjans",
                            "Klasyczny Klondike Solitaire",
                        )
                        .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.current_screen = Screen::Solitaire;
                        }
                        if Self::game_card(
                            ui,
                            card_size,
                            "🎰 Blackjack",
                            "Oko / 21 przeciwko krupierowi",
                        )
                        .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.current_screen = Screen::Blackjack;
                        }
                        if Self::game_card(
                            ui,
                            card_size,
                            "⚔ Wojna",
                            "Szybki pojedynek o całą talię",
                        )
                        .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.current_screen = Screen::War;
                        }
                        ui.end_row();
                    });
            });
        });
    }

    fn game_card(
        ui: &mut egui::Ui,
        desired_size: egui::Vec2,
        title: &str,
        subtitle: &str,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

        if ui.is_rect_visible(rect) {
            let scale_h = desired_size.y / 160.0;
            let scale_w = desired_size.x / 310.0;
            let scale = scale_h.min(scale_w).clamp(0.65, 1.35);

            let visuals = ui.style().interact(&response);

            let expansion = if response.hovered() { 4.0 * scale } else { 0.0 };
            let draw_rect = rect.expand(expansion);

            ui.painter().rect_filled(
                draw_rect.translate(egui::vec2(4.0 * scale, 5.0 * scale)),
                14.0 * scale,
                egui::Color32::from_black_alpha(90),
            );

            ui.painter().rect(
                draw_rect,
                14.0 * scale,
                visuals.bg_fill,
                visuals.bg_stroke,
            );

            let title_font = (23.0 * scale).clamp(15.0, 36.0);
            let sub_font = (13.0 * scale).clamp(10.5, 19.0);

            let text_pos = draw_rect.center() - egui::vec2(0.0, 15.0 * scale);
            ui.painter().text(
                text_pos,
                egui::Align2::CENTER_CENTER,
                title,
                egui::FontId::proportional(title_font),
                egui::Color32::WHITE,
            );

            let sub_pos = draw_rect.center() + egui::vec2(0.0, 20.0 * scale);
            ui.painter().text(
                sub_pos,
                egui::Align2::CENTER_CENTER,
                subtitle,
                egui::FontId::proportional(sub_font),
                egui::Color32::LIGHT_GRAY,
            );
        }

        if response.hovered() {
            ui.ctx().request_repaint();
        }

        response
    }
}