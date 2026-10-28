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
    /// Skład pokoju z poprzedniej klatki — pozwala odróżnić dołączenie
    /// obserwatora (gra trwa dalej) od zmiany składu graczy (gra się resetuje).
    last_room_order: Vec<usize>,
    blackjack: blackjack::BlackjackGame,
    solitaire: solitaire::SolitaireGame,
    chess: chess::ChessGame,
    ludo: ludo::LudoGame,
    war: war::WarGame,
    battleship: battleship::BattleshipGame,
}

/// Gry, w których pokój 4-osobowy dzieli się na 2 graczy + obserwatorów.
fn is_two_player_screen(screen: Screen) -> bool {
    matches!(screen, Screen::Chess | Screen::Battleship)
}

fn screen_game_key(screen: Screen) -> Option<&'static str> {
    match screen {
        Screen::Chess => Some("chess"),
        Screen::Battleship => Some("battleship"),
        Screen::Ludo => Some("ludo"),
        _ => None,
    }
}

impl GameHubApp {
    /// Reset tylko tej gry, której dotyczyła zatwierdzona prośba.
    fn reset_online_game(&mut self, game: &str) {
        let my_idx = self.net.my_player_idx;
        let count = self.net.room_players_count.max(1);
        match game {
            "chess" => self.chess.setup_online(my_idx),
            "battleship" => self.battleship.setup_online(my_idx),
            "ludo" => self.ludo.setup_online(my_idx, count),
            _ => {}
        }
    }

    /// Reaguje na zmianę kolejki miejsc w pokoju.
    ///
    /// - dołączenie/odejście obserwatora nie resetuje gry 2-osobowej,
    /// - zmiana pierwszych dwóch miejsc (graczy) resetuje grę i automatycznie
    ///   promuje kolejne osoby z kolejki,
    /// - Chińczyk korzysta ze wszystkich czterech miejsc, więc resetuje się
    ///   przy każdej zmianie składu.
    fn handle_room_state_change(&mut self) {
        let new_order = self.net.room_order.clone();
        if new_order == self.last_room_order {
            return;
        }

        let old_order = std::mem::take(&mut self.last_room_order);
        let old_two: Vec<usize> = old_order.iter().copied().take(2).collect();
        let new_two: Vec<usize> = new_order.iter().copied().take(2).collect();
        let players_changed = old_two != new_two;
        let roster_changed = old_order != new_order;

        if players_changed {
            self.chess.setup_online(self.net.my_player_idx);
            self.battleship.setup_online(self.net.my_player_idx);
            self.war.new_game();
        }

        if roster_changed {
            self.ludo
                .setup_online(self.net.my_player_idx, self.net.room_players_count.max(1));
        }

        if new_order.len() > old_order.len() {
            self.audio.play(Sfx::Check);
        } else if new_order.len() < old_order.len() {
            self.audio.play(Sfx::Click);
        }

        self.last_room_order = new_order;

        // Jeśli jesteśmy obserwatorami i właśnie zmienił się skład (np. weszliśmy
        // do pokoju z otwartym ekranem gry), dociągnij świeży stan planszy.
        self.request_game_state(self.current_screen);
    }

    /// Odpowiada na prośbę obserwatora o aktualny stan gry.
    fn handle_state_request(&mut self, game: &str) {
        let max_players = if matches!(game, "chess" | "battleship") {
            2
        } else {
            4
        };
        if self.net.is_spectator(max_players) {
            return;
        }
        if let Some(state) = self.game_state_json(game) {
            self.net.send_event(&NetEvent::GameState {
                game: game.to_string(),
                from_seat: self.net.my_player_idx,
                state,
            });
        }
    }

    fn game_state_json(&self, game: &str) -> Option<serde_json::Value> {
        match game {
            "chess" => Some(self.chess.state_json()),
            "battleship" => Some(self.battleship.state_json(self.net.my_player_idx)),
            "ludo" => Some(self.ludo.state_json()),
            _ => None,
        }
    }

    fn apply_game_state(&mut self, game: &str, state: &serde_json::Value) {
        let max_players = if matches!(game, "chess" | "battleship") {
            2
        } else {
            4
        };
        // Stan z sieci przyjmują tylko obserwatorzy. Aktywni gracze są na
        // bieżąco przez zdarzenia ruchów — nadpisanie mogłoby zniszczyć
        // np. własne rozmieszczanie flot w Statkach.
        if !self.net.is_spectator(max_players) {
            return;
        }
        match game {
            "chess" => {
                self.chess.apply_state_json(state);
            }
            "battleship" => {
                self.battleship.apply_state_json(state);
            }
            "ludo" => {
                self.ludo.apply_state_json(state);
            }
            _ => {}
        }
    }

    fn request_game_state(&self, screen: Screen) {
        if !self.is_online_session || self.net.connected_room.is_none() {
            return;
        }
        if let Some(game) = screen_game_key(screen) {
            let max_players = if matches!(game, "chess" | "battleship") {
                2
            } else {
                4
            };
            // O stan proszą tylko osoby, które dołączyły do trwającej gry.
            if !self.net.is_spectator(max_players) {
                return;
            }
            self.net.send_event(&NetEvent::StateRequest {
                game: game.to_string(),
            });
        }
    }

    fn open_game(&mut self, screen: Screen) {
        self.audio.play(Sfx::Click);
        self.current_screen = screen;
        self.request_game_state(screen);
    }

    /// Wychodzimy z trybu pokoju — gry muszą wrócić do trybu lokalnego,
    /// inaczej dawny obserwator zostałby zablokowany na własnej planszy.
    fn leave_online_games(&mut self) {
        self.chess.online_mode = false;
        self.chess.spectator = false;
        self.ludo.online_mode = false;
        self.battleship.leave_online();
    }

    fn spectator_count_for_screen(&self, screen: Screen) -> usize {
        if self.is_online_session && is_two_player_screen(screen) {
            self.net.spectator_count(2)
        } else {
            0
        }
    }

    fn is_spectator_for_screen(&self, screen: Screen) -> bool {
        self.is_online_session && is_two_player_screen(screen) && self.net.is_spectator(2)
    }

    fn current_game_key(&self) -> &'static str {
        screen_game_key(self.current_screen).unwrap_or("hub")
    }

    /// Czy lokalny gracz może głosować nad oczekującą prośbą o reset.
    /// Liczy się gra z prośby, a nie aktualnie otwarty ekran.
    fn can_vote_on_pending_reset(&self) -> bool {
        let Some(game) = self.net.incoming_reset_game.as_deref() else {
            return false;
        };
        let max_players = if matches!(game, "chess" | "battleship") {
            2
        } else {
            4
        };
        !self.net.is_spectator(max_players)
    }
}

fn game_label(key: &str) -> &'static str {
    match key {
        "chess" => "Szachy",
        "battleship" => "Statki",
        "ludo" => "Chińczyk",
        _ => "grę",
    }
}

impl eframe::App for GameHubApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.is_online_session && self.net.connected_room.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
            let events = self.net.poll_events();
            for ev in events {
                match ev {
                    NetEvent::Assigned { .. } | NetEvent::RoomState { .. } => {
                        // Skład pokoju obsługujemy zbiorczo po odebraniu wszystkich
                        // zdarzeń — wtedy widzimy już finalną kolejkę miejsc.
                    }
                    NetEvent::ResetRequest { .. } => {
                        self.audio.play(Sfx::Check);
                    }
                    NetEvent::ResetApproved { game } => {
                        self.audio.play(Sfx::CardSlide);
                        self.reset_online_game(&game);
                    }
                    NetEvent::ResetDeclined { .. } => {
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
                    NetEvent::BattleshipFleet { ships, from_seat } => {
                        self.battleship.receive_enemy_fleet(ships, from_seat);
                    }
                    NetEvent::BattleshipShot { x, y, from_seat } => {
                        self.battleship
                            .receive_enemy_shot(x, y, from_seat, &self.audio);
                    }
                    NetEvent::StateRequest { game } => {
                        self.handle_state_request(&game);
                    }
                    NetEvent::GameState { game, state, .. } => {
                        self.apply_game_state(&game, &state);
                    }
                }
            }

            self.handle_room_state_change();

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
                if self.net.my_player_idx < 2 {
                    self.net.send_event(&NetEvent::WarStep);
                }
            }
            if let Some(ships) = self.battleship.outgoing_fleet.take() {
                self.net.send_event(&NetEvent::BattleshipFleet {
                    ships,
                    from_seat: self.net.my_player_idx,
                });
            }
            if let Some((x, y)) = self.battleship.outgoing_shot.take() {
                self.net.send_event(&NetEvent::BattleshipShot {
                    x,
                    y,
                    from_seat: self.net.my_player_idx,
                });
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
                        let spectator = self.is_spectator_for_screen(self.current_screen);
                        let spectators = self.spectator_count_for_screen(self.current_screen);
                        let role = if spectator {
                            format!("👁 Obserwator (miejsce #{})", self.net.my_player_idx + 1)
                        } else {
                            format!("Miejsce #{}", self.net.my_player_idx + 1)
                        };
                        let watching = match spectators {
                            0 => String::new(),
                            1 => " | 👁 1 osoba obserwuje".to_string(),
                            n => format!(" | 👁 {} osoby obserwują", n),
                        };
                        ui.label(
                            egui::RichText::new(format!(
                                "🟢 {} ({}, graczy: {}/4{})",
                                room, role, self.net.room_players_count, watching
                            ))
                            .size(14.0)
                            .strong()
                            .color(if spectator {
                                egui::Color32::GOLD
                            } else {
                                egui::Color32::from_rgb(80, 240, 120)
                            }),
                        );

                        let in_game = screen_game_key(self.current_screen).is_some();
                        if let Some(requester) = self.net.incoming_reset_from {
                            ui.separator();
                            let can_vote = self.can_vote_on_pending_reset();
                            let pending_label = self
                                .net
                                .incoming_reset_game
                                .as_deref()
                                .map(game_label)
                                .unwrap_or("grę");
                            if !can_vote {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "⚠ Gracz #{} chce zresetować: {} (obserwatorzy nie głosują)",
                                        requester + 1,
                                        pending_label
                                    ))
                                    .strong()
                                    .color(egui::Color32::GOLD),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "⚠ Gracz #{} chce zresetować: {}!",
                                        requester + 1,
                                        pending_label
                                    ))
                                    .strong()
                                    .color(egui::Color32::GOLD),
                                );
                                let game = self
                                    .net
                                    .incoming_reset_game
                                    .clone()
                                    .unwrap_or_else(|| self.current_game_key().to_string());
                                if ui
                                    .button(
                                        egui::RichText::new("✅ Zgoda")
                                            .color(egui::Color32::from_rgb(80, 245, 120)),
                                    )
                                    .clicked()
                                {
                                    // Reset wykonamy po potwierdzeniu z serwera,
                                    // żeby nie rozjechać stanu, gdy głos jest spóźniony.
                                    self.net.send_event(&NetEvent::ResetApproved {
                                        game: game.clone(),
                                    });
                                    self.audio.play(Sfx::CardSlide);
                                }
                                if ui
                                    .button(
                                        egui::RichText::new("❌ Odrzuć")
                                            .color(egui::Color32::from_rgb(255, 95, 95)),
                                    )
                                    .clicked()
                                {
                                    self.net.send_event(&NetEvent::ResetDeclined { game });
                                    self.audio.play(Sfx::Click);
                                }
                            }
                        } else if self.net.reset_requested_by_me {
                            ui.separator();
                            ui.label(
                                egui::RichText::new("⏳ Czekanie na zgodę...")
                                    .color(egui::Color32::GOLD),
                            );
                        } else if self.net.room_players_count >= 2 && !spectator && in_game {
                            if ui.button("🔄 Resetuj grę").clicked() {
                                self.audio.play(Sfx::Click);
                                self.net.reset_requested_by_me = true;
                                self.net.send_event(&NetEvent::ResetRequest {
                                    from_player: self.net.my_player_idx,
                                    game: self.current_game_key().to_string(),
                                });
                            }
                        }

                        if ui.button("🚪 Zmień pokój").clicked() {
                            self.audio.play(Sfx::Click);
                            self.net.disconnect_room();
                            self.last_room_order.clear();
                            self.leave_online_games();
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
                            self.last_room_order.clear();
                            self.leave_online_games();
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
                Screen::Blackjack => {
                    self.blackjack.is_online = self.is_online_session;
                    self.blackjack.ui(ui, &self.audio);
                }
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
                    self.chess.spectators = self.spectator_count_for_screen(Screen::Chess);
                    self.chess.ui(ui, &self.audio);
                    if self.chess.newly_won {
                        self.chess.newly_won = false;
                        if self.is_online_session && !self.chess.spectator {
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
                    self.battleship.spectators =
                        self.spectator_count_for_screen(Screen::Battleship);
                    self.battleship.ui(ui, &self.audio);
                    if self.battleship.newly_won {
                        self.battleship.newly_won = false;
                        if self.is_online_session && !self.battleship.spectator {
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

                    let resp =
                        Self::game_card(ui, egui::vec2(card_w, card_h), &title, &subtitle, 0);
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
                    self.last_room_order.clear();
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
                self.last_room_order.clear();
                self.leave_online_games();
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
            let chess_spectators = self.spectator_count_for_screen(Screen::Chess);
            let battleship_spectators = self.spectator_count_for_screen(Screen::Battleship);

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
                            chess_spectators,
                        )
                        .clicked()
                        {
                            self.open_game(Screen::Chess);
                        }
                        if Self::game_card(
                            ui,
                            card_size,
                            "🚢 Statki",
                            "Taktyczna bitwa morska na radarze",
                            battleship_spectators,
                        )
                        .clicked()
                        {
                            self.open_game(Screen::Battleship);
                        }
                        if Self::game_card(
                            ui,
                            card_size,
                            "🎲 Chińczyk",
                            "Planszówka Ludo dla 2-4 graczy",
                            0,
                        )
                        .clicked()
                        {
                            self.open_game(Screen::Ludo);
                        }
                        ui.end_row();

                        if Self::game_card(
                            ui,
                            card_size,
                            "🃏 Pasjans",
                            "Klasyczny Klondike Solitaire",
                            0,
                        )
                        .clicked()
                        {
                            self.open_game(Screen::Solitaire);
                        }
                        if Self::game_card(
                            ui,
                            card_size,
                            "🎰 Blackjack",
                            "Oko / 21 przeciwko krupierowi",
                            0,
                        )
                        .clicked()
                        {
                            self.open_game(Screen::Blackjack);
                        }
                        if Self::game_card(
                            ui,
                            card_size,
                            "⚔ Wojna",
                            "Szybki pojedynek o całą talię",
                            0,
                        )
                        .clicked()
                        {
                            self.open_game(Screen::War);
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
        spectators: usize,
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

            if spectators > 0 {
                let badge_text = format!("👁 {}", spectators);
                let badge_font = (13.0 * scale).clamp(10.0, 18.0);
                let badge_size = egui::vec2(52.0 * scale, 24.0 * scale);
                let badge_rect = egui::Rect::from_min_size(
                    egui::pos2(
                        draw_rect.right() - badge_size.x - 10.0 * scale,
                        draw_rect.top() + 10.0 * scale,
                    ),
                    badge_size,
                );
                ui.painter().rect_filled(
                    badge_rect,
                    badge_size.y * 0.5,
                    egui::Color32::from_rgba_unmultiplied(35, 30, 15, 220),
                );
                ui.painter().rect_stroke(
                    badge_rect,
                    badge_size.y * 0.5,
                    egui::Stroke::new(1.2 * scale, egui::Color32::GOLD),
                );
                ui.painter().text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    badge_text,
                    egui::FontId::proportional(badge_font),
                    egui::Color32::GOLD,
                );
            }
        }

        if response.hovered() {
            ui.ctx().request_repaint();
        }

        response
    }
}