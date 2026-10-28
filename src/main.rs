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
    /// Ostatnio wysłany serwerowi ekran/gra — wysyłamy tylko przy zmianie.
    last_sent_focus: Option<String>,
    /// Gra, w której własny stolik ostatnio grał jako gracze.
    last_my_pair_game: Option<String>,
    /// Stolik, którego grę aktualnie obserwujemy (gra, numer stolika).
    last_watched_pair: Option<(String, usize)>,
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
        let count = self.net.room_players_count.max(1);
        match game {
            "chess" => {
                self.chess.setup_online(self.net.my_pair_seat());
                if self.current_game_key() == "chess" {
                    self.last_my_pair_game = Some("chess".to_string());
                }
            }
            "battleship" => {
                self.battleship.setup_online(self.net.my_pair_seat());
                if self.current_game_key() == "battleship" {
                    self.last_my_pair_game = Some("battleship".to_string());
                }
            }
            "ludo" => self
                .ludo
                .setup_online(self.net.my_player_idx, count),
            _ => {}
        }
    }

    /// Reaguje na zmianę składu pokoju.
    ///
    /// Stoliki to stałe pary slotów (0/1 i 2/3), więc zmiana w drugim
    /// stoliku nie przerywa gry pierwszego. Resetujemy tylko grę własnej
    /// pary, gdy zmieni się jej skład.
    fn handle_room_state_change(&mut self) {
        let new_order = self.net.room_order.clone();
        if new_order == self.last_room_order {
            return;
        }

        let old_order = std::mem::take(&mut self.last_room_order);
        let partner_slot = self.net.my_pair() * 2 + (1 - self.net.my_pair_seat());
        let old_partner = old_order.contains(&partner_slot);
        let new_partner = new_order.contains(&partner_slot);
        let pair_roster_changed = old_partner != new_partner;

        if pair_roster_changed {
            // Przy kolejnym wejściu obu graczy na ten sam ekran gra
            // zostanie bezpiecznie rozpoczęta od nowa.
            self.last_my_pair_game = None;
            self.last_watched_pair = None;
            if self.net.my_pair() == 0 {
                self.war.new_game();
            }
        }

        if old_order != new_order {
            self.ludo
                .setup_online(self.net.my_player_idx, self.net.room_players_count.max(1));
        }

        if new_order.len() > old_order.len() {
            self.audio.play(Sfx::Check);
        } else if new_order.len() < old_order.len() {
            self.audio.play(Sfx::Click);
        }

        self.last_room_order = new_order;
        // Świeży stan planszy dociąga `sync_game_modes` — tu nie wysyłamy
        // drugiej prośby w tej samej klatce.
    }

    /// Ustawia tryb gry (gracz / obserwator / oczekiwanie) dla aktualnego
    /// ekranu, na podstawie tego, co robią oba stoliki.
    fn sync_game_modes(&mut self) {
        if !self.is_online_session || self.net.connected_room.is_none() {
            return;
        }
        let screen_game = match self.current_screen {
            Screen::Chess => Some("chess"),
            Screen::Battleship => Some("battleship"),
            _ => None,
        };

        // Jeśli wyszliśmy z ekranu, przy którym nasz stolik grał, od razu
        // zerujemy tę planszę. Partner wchodzi wtedy w stan oczekiwania i robi
        // dokładnie to samo, więc po powrocie oboje zaczynamy świeżą partię
        // zamiast zostać z niespójnym stanem planszy.
        if let Some(prev) = self.last_my_pair_game.clone() {
            if screen_game != Some(prev.as_str()) {
                self.last_my_pair_game = None;
                let seat = self.net.my_pair_seat();
                match prev.as_str() {
                    "chess" => self.chess.setup_online(seat),
                    "battleship" => self.battleship.setup_online(seat),
                    _ => {}
                }
            }
        }

        let Some(game) = screen_game else {
            self.last_watched_pair = None;
            return;
        };
        let seat = self.net.my_pair_seat();
        let count = self.net.spectator_count_for_game(game);

        if self.net.my_pair_active(game) {
            // Własny stolik gra. Po przerwie/obserwowaniu zaczynamy świeżo,
            // ale zwykłe pozostanie na ekranie nie resetuje partii.
            if self.last_my_pair_game.as_deref() != Some(game) {
                match game {
                    "chess" => self.chess.setup_online(seat),
                    "battleship" => self.battleship.setup_online(seat),
                    _ => {}
                }
                self.last_my_pair_game = Some(game.to_string());
            }
            match game {
                "chess" => {
                    self.chess.set_online_role(false, false);
                    self.chess.spectators = count;
                }
                "battleship" => {
                    self.battleship.set_online_role(false, false);
                    self.battleship.spectators = count;
                }
                _ => {}
            }
            self.last_watched_pair = None;
        } else if self.net.is_spectator_for_game(game) {
            // Drugi stolik gra — możemy tylko obserwować.
            let owner = self
                .net
                .playing_pair(game)
                .unwrap_or_else(|| self.net.other_pair());
            let watch_key = (game.to_string(), owner);
            let new_watch = self.last_watched_pair.as_ref() != Some(&watch_key);
            match game {
                "chess" => {
                    self.chess.set_online_role(true, false);
                    self.chess.spectators = count;
                }
                "battleship" => {
                    self.battleship.set_online_role(true, false);
                    self.battleship.spectators = count;
                }
                _ => {}
            }
            if new_watch {
                // Snapshot może nadpisać naszą lokalną planszę, więc po
                // powrocie do gry własnego stolika zaczniemy ją od nowa.
                self.last_my_pair_game = None;
                self.last_watched_pair = Some(watch_key);
                self.net.send_event(&NetEvent::StateRequest {
                    game: game.to_string(),
                    pair: owner,
                });
            }
        } else {
            // Brak aktywnego stolika — czekamy na partnera, plansza zablokowana.
            if self.last_my_pair_game.as_deref() == Some(game) {
                // Przerwana partia nie może zostać wznowiona tylko u jednej
                // osoby — oboje zaczniemy od świeżej planszy, gdy wrócimy.
                match game {
                    "chess" => self.chess.setup_online(seat),
                    "battleship" => self.battleship.setup_online(seat),
                    _ => {}
                }
                self.last_my_pair_game = None;
            }
            match game {
                "chess" => {
                    self.chess.set_online_role(false, true);
                    self.chess.spectators = count;
                }
                "battleship" => {
                    self.battleship.set_online_role(false, true);
                    self.battleship.spectators = count;
                }
                _ => {}
            }
            self.last_watched_pair = None;
        }
    }

    /// Odpowiada na prośbę obserwatora o aktualny stan gry.
    fn handle_state_request(&mut self, game: &str, pair: usize) {
        if game == "chess" || game == "battleship" {
            if self.net.my_pair() != pair || !self.net.my_pair_active(game) {
                return;
            }
        }
        if let Some(state) = self.game_state_json(game) {
            self.net.send_event(&NetEvent::GameState {
                game: game.to_string(),
                from_seat: if game == "ludo" {
                    self.net.my_player_idx
                } else {
                    self.net.my_pair_seat()
                },
                pair: if game == "ludo" { 2 } else { pair },
                state,
            });
        }
    }

    fn game_state_json(&self, game: &str) -> Option<serde_json::Value> {
        match game {
            "chess" => Some(self.chess.state_json()),
            "battleship" => Some(self.battleship.state_json(self.net.my_pair_seat())),
            "ludo" => Some(self.ludo.state_json()),
            _ => None,
        }
    }

    fn apply_game_state(&mut self, game: &str, pair: usize, state: &serde_json::Value) {
        // Stan z sieci przyjmują tylko obserwatorzy drugiego stolika.
        // Aktywni gracze są na bieżąco przez zdarzenia ruchów.
        if game == "chess" || game == "battleship" {
            if self.net.playing_pair(game) != Some(pair) || pair == self.net.my_pair() {
                return;
            }
        } else if !self.net.is_spectator(4) {
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

    fn open_game(&mut self, screen: Screen) {
        self.audio.play(Sfx::Click);
        self.current_screen = screen;
        // O świeży stan planszy poprosi `sync_game_modes` w kolejnej klatce
        // (tylko gdy faktycznie jesteśmy obserwatorami innego stolika).
    }

    /// Wychodzimy z trybu pokoju — gry muszą wrócić do trybu lokalnego,
    /// inaczej dawny obserwator zostałby zablokowany na własnej planszy.
    fn leave_online_games(&mut self) {
        self.chess.online_mode = false;
        self.chess.spectator = false;
        self.chess.waiting = false;
        self.ludo.online_mode = false;
        self.battleship.leave_online();
        self.last_my_pair_game = None;
        self.last_watched_pair = None;
    }

    fn spectator_count_for_screen(&self, screen: Screen) -> usize {
        if self.is_online_session && is_two_player_screen(screen) {
            let game = screen_game_key(screen).unwrap_or("");
            return self.net.spectator_count_for_game(game);
        }
        0
    }

    fn is_spectator_for_screen(&self, screen: Screen) -> bool {
        if self.is_online_session && is_two_player_screen(screen) {
            let game = screen_game_key(screen).unwrap_or("");
            return self.net.is_spectator_for_game(game);
        }
        false
    }

    fn current_game_key(&self) -> &'static str {
        screen_game_key(self.current_screen).unwrap_or("hub")
    }

    /// Czy można poprosić o reset aktualnie otwartej gry.
    fn can_request_reset(&self) -> bool {
        if !self.is_online_session || self.net.connected_room.is_none() {
            return false;
        }
        let game = self.current_game_key();
        if game == "chess" || game == "battleship" {
            self.net.my_pair_active(game) && !self.net.is_spectator_for_game(game)
        } else {
            game == "ludo" && self.net.room_players_count >= 2
        }
    }

    /// Czy lokalny gracz może głosować nad oczekującą prośbą o reset.
    /// Liczy się gra i stolik z prośby, a nie aktualnie otwarty ekran.
    fn can_vote_on_pending_reset(&self) -> bool {
        let Some(game) = self.net.incoming_reset_game.as_deref() else {
            return false;
        };
        let Some(pair) = self.net.incoming_reset_pair else {
            return false;
        };
        if game == "chess" || game == "battleship" {
            return pair == self.net.my_pair()
                && self.net.my_pair_active(game)
                && self.net.incoming_reset_from != Some(self.net.my_pair_seat());
        }
        !self.net.is_spectator(4)
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
            // Najpierw aktualizujemy skład pokoju i role przy stolikach,
            // dopiero potem stosujemy zdarzenia ruchów z sieci.
            self.handle_room_state_change();
            self.sync_game_modes();

            for ev in events {
                match ev {
                    NetEvent::Assigned { .. }
                    | NetEvent::RoomState { .. }
                    | NetEvent::FocusGame { .. } => {
                        // Skład pokoju obsługujemy zbiorczo po odebraniu wszystkich
                        // zdarzeń — wtedy widzimy już finalną kolejkę miejsc.
                    }
                    NetEvent::ResetRequest { pair, .. } => {
                        if pair == self.net.my_pair() || pair == 2 {
                            self.audio.play(Sfx::Check);
                        }
                    }
                    NetEvent::ResetApproved { game, pair } => {
                        if pair == self.net.my_pair() || pair == 2 {
                            self.audio.play(Sfx::CardSlide);
                            self.reset_online_game(&game);
                        }
                    }
                    NetEvent::ResetDeclined { pair, .. } => {
                        if pair == self.net.my_pair() || pair == 2 {
                            self.audio.play(Sfx::Lose);
                        }
                    }
                    NetEvent::ChessMove { from, to, pair } => {
                        if self.current_screen == Screen::Chess
                            && self.net.playing_pair("chess") == Some(pair)
                        {
                            self.chess.apply_remote_move(from, to, &self.audio);
                        }
                    }
                    NetEvent::LudoRoll { face } => {
                        self.ludo.apply_remote_roll(face, &self.audio);
                    }
                    NetEvent::LudoMove { pawn_idx } => {
                        self.ludo.apply_remote_pawn_move(pawn_idx, &self.audio);
                    }
                    NetEvent::WarStep => {
                        if self.current_screen == Screen::War {
                            self.war.play_step(&self.audio);
                        }
                    }
                    NetEvent::BattleshipFleet {
                        ships,
                        from_seat,
                        pair,
                    } => {
                        if self.current_screen == Screen::Battleship
                            && self.net.playing_pair("battleship") == Some(pair)
                        {
                            self.battleship.receive_enemy_fleet(ships, from_seat);
                        }
                    }
                    NetEvent::BattleshipShot {
                        x,
                        y,
                        from_seat,
                        pair,
                    } => {
                        if self.current_screen == Screen::Battleship
                            && self.net.playing_pair("battleship") == Some(pair)
                        {
                            self.battleship
                                .receive_enemy_shot(x, y, from_seat, &self.audio);
                        }
                    }
                    NetEvent::StateRequest { game, pair } => {
                        self.handle_state_request(&game, pair);
                    }
                    NetEvent::GameState {
                        game, pair, state, ..
                    } => {
                        self.apply_game_state(&game, pair, &state);
                    }
                }
            }

            // Informujemy serwer o aktualnie otwartej grze, ale tylko gdy
            // faktycznie się zmieniła — bez spamu jednym pakietem na klatkę.
            let focus = self.current_game_key();
            if self.last_sent_focus.as_deref() != Some(focus) {
                self.net.send_event(&NetEvent::FocusGame {
                    game: focus.to_string(),
                });
                self.last_sent_focus = Some(focus.to_string());
            }

            if let Some((from, to)) = self.chess.outgoing_move.take() {
                self.net.send_event(&NetEvent::ChessMove {
                    from,
                    to,
                    pair: self.net.my_pair(),
                });
            }
            if let Some(face) = self.ludo.outgoing_roll.take() {
                self.net.send_event(&NetEvent::LudoRoll { face });
            }
            if let Some(pawn_idx) = self.ludo.outgoing_pawn.take() {
                self.net.send_event(&NetEvent::LudoMove { pawn_idx });
            }
            if self.war.outgoing_step {
                self.war.outgoing_step = false;
                if self.net.my_pair() == 0 {
                    self.net.send_event(&NetEvent::WarStep);
                }
            }
            if let Some(ships) = self.battleship.outgoing_fleet.take() {
                self.net.send_event(&NetEvent::BattleshipFleet {
                    ships,
                    from_seat: self.net.my_pair_seat(),
                    pair: self.net.my_pair(),
                });
            }
            if let Some((x, y)) = self.battleship.outgoing_shot.take() {
                self.net.send_event(&NetEvent::BattleshipShot {
                    x,
                    y,
                    from_seat: self.net.my_pair_seat(),
                    pair: self.net.my_pair(),
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
            // Skalujemy pasek do szerokości okna. Przy wąskim oknie zmniejszamy
            // czcionki i odstępy, a `horizontal_wrapped` pozwala zawinąć resztę
            // do drugiej linii, zamiast ucinać informacje.
            let bar_width = ui.available_width();
            let scale = (bar_width / 1500.0).clamp(0.64, 1.0);
            let narrow = scale < 0.82;
            let fs_title = 20.0 * scale;
            let fs_btn = 15.0 * scale;
            let fs_info = 14.0 * scale;
            let fs_small = 13.0 * scale;

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = (8.0 * scale).max(3.0);
                ui.spacing_mut().button_padding = egui::vec2(6.0 * scale, 3.0 * scale);

                ui.heading(
                    egui::RichText::new(if narrow {
                        format!("🎮 FGH v{}", net::APP_VERSION)
                    } else {
                        format!("🎮 Fiksiarski Game Hub v{}", net::APP_VERSION)
                    })
                    .size(fs_title)
                    .strong(),
                );

                if let Some(new_ver) = self.net.available_update.clone() {
                    ui.separator();
                    if ui
                        .button(
                            egui::RichText::new(format!("⬇ Aktualizuj do v{}!", new_ver))
                                .size(fs_info)
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
                            .size(fs_small)
                            .color(egui::Color32::from_rgb(255, 100, 100)),
                    );
                }

                if !matches!(
                    self.current_screen,
                    Screen::Login | Screen::RoomSelect | Screen::Hub
                ) {
                    ui.separator();
                    if ui
                        .button(egui::RichText::new("⬅ Menu Gier").size(fs_btn))
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
                        .button(egui::RichText::new("🏆 Rankingi").size(fs_btn))
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
                        let two_player = is_two_player_screen(self.current_screen);
                        let table_no = self.net.my_pair() + 1;
                        let seat_no = self.net.my_pair_seat() + 1;
                        let role_full = if two_player {
                            if spectator {
                                format!("👁 Obserwator (stolik {}, #{})", table_no, seat_no)
                            } else {
                                format!("Stolik {}, #{}", table_no, seat_no)
                            }
                        } else if spectator {
                            format!("👁 Obserwator (miejsce #{})", self.net.my_player_idx + 1)
                        } else {
                            format!("Miejsce #{}", self.net.my_player_idx + 1)
                        };
                        let role_short = if two_player {
                            if spectator {
                                format!("👁 S{}#{}", table_no, seat_no)
                            } else {
                                format!("S{}#{}", table_no, seat_no)
                            }
                        } else if spectator {
                            format!("👁 #{}", self.net.my_player_idx + 1)
                        } else {
                            format!("#{}", self.net.my_player_idx + 1)
                        };
                        let watching_full = match spectators {
                            0 => String::new(),
                            1 => " | 👁 1 osoba obserwuje".to_string(),
                            n => format!(" | 👁 {} osoby obserwują", n),
                        };
                        let watching_short = if spectators > 0 {
                            format!(" | 👁 {}", spectators)
                        } else {
                            String::new()
                        };
                        let room_text = if narrow {
                            format!(
                                "🟢 {} ({}, {}/4{})",
                                room,
                                role_short,
                                self.net.room_players_count,
                                watching_short
                            )
                        } else {
                            format!(
                                "🟢 {} ({}, graczy: {}/4{})",
                                room,
                                role_full,
                                self.net.room_players_count,
                                watching_full
                            )
                        };
                        ui.label(
                            egui::RichText::new(room_text)
                                .size(fs_info)
                                .strong()
                                .color(if spectator {
                                    egui::Color32::GOLD
                                } else {
                                    egui::Color32::from_rgb(80, 240, 120)
                                }),
                        );

                        if let Some(requester) = self.net.incoming_reset_from {
                            ui.separator();
                            let can_vote = self.can_vote_on_pending_reset();
                            let pair = self.net.incoming_reset_pair.unwrap_or(self.net.my_pair());
                            let pending_label = self
                                .net
                                .incoming_reset_game
                                .as_deref()
                                .map(game_label)
                                .unwrap_or("grę");
                            if !can_vote {
                                ui.label(
                                    egui::RichText::new(if narrow {
                                        format!(
                                            "⚠ #{}: reset {} (👁 brak głosu)",
                                            requester + 1,
                                            pending_label
                                        )
                                    } else {
                                        format!(
                                            "⚠ Gracz #{} chce zresetować: {} (obserwatorzy nie głosują)",
                                            requester + 1,
                                            pending_label
                                        )
                                    })
                                    .size(fs_info)
                                    .strong()
                                    .color(egui::Color32::GOLD),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new(if narrow {
                                        format!("⚠ #{}: reset {}?", requester + 1, pending_label)
                                    } else {
                                        format!(
                                            "⚠ Gracz #{} chce zresetować: {}!",
                                            requester + 1,
                                            pending_label
                                        )
                                    })
                                    .size(fs_info)
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
                                            .size(fs_btn)
                                            .color(egui::Color32::from_rgb(80, 245, 120)),
                                    )
                                    .clicked()
                                {
                                    // Reset wykonamy po potwierdzeniu z serwera,
                                    // żeby nie rozjechać stanu, gdy głos jest spóźniony.
                                    self.net.send_event(&NetEvent::ResetApproved {
                                        game: game.clone(),
                                        pair,
                                    });
                                    self.audio.play(Sfx::CardSlide);
                                }
                                if ui
                                    .button(
                                        egui::RichText::new("❌ Odrzuć")
                                            .size(fs_btn)
                                            .color(egui::Color32::from_rgb(255, 95, 95)),
                                    )
                                    .clicked()
                                {
                                    self.net.send_event(&NetEvent::ResetDeclined { game, pair });
                                    self.audio.play(Sfx::Click);
                                }
                            }
                        } else if self.net.reset_requested_by_me {
                            ui.separator();
                            ui.label(
                                egui::RichText::new(if narrow {
                                    "⏳ Czekam na zgodę..."
                                } else {
                                    "⏳ Czekanie na zgodę..."
                                })
                                .size(fs_info)
                                .color(egui::Color32::GOLD),
                            );
                        } else if self.can_request_reset() {
                            let reset_label = if narrow {
                                "🔄 Reset"
                            } else {
                                "🔄 Resetuj grę"
                            };
                            if ui
                                .button(egui::RichText::new(reset_label).size(fs_btn))
                                .clicked()
                            {
                                self.audio.play(Sfx::Click);
                                self.net.reset_requested_by_me = true;
                                self.net.send_event(&NetEvent::ResetRequest {
                                    from_player: self.net.my_pair_seat(),
                                    game: self.current_game_key().to_string(),
                                    pair: self.net.my_pair(),
                                });
                            }
                        }

                        let room_label = if narrow {
                            "🚪 Pokój"
                        } else {
                            "🚪 Zmień pokój"
                        };
                        if ui
                            .button(egui::RichText::new(room_label).size(fs_btn))
                            .clicked()
                        {
                            self.audio.play(Sfx::Click);
                            self.net.disconnect_room();
                            self.last_room_order.clear();
                            self.last_sent_focus = None;
                            self.leave_online_games();
                            self.net.fetch_rooms();
                            self.current_screen = Screen::RoomSelect;
                        }
                    } else if ui
                        .button(
                            egui::RichText::new(if narrow {
                                "🌐 Pokój MP"
                            } else {
                                "🌐 Wybierz pokój MP"
                            })
                            .size(fs_btn),
                        )
                        .clicked()
                    {
                        self.audio.play(Sfx::Click);
                        self.net.fetch_rooms();
                        self.current_screen = Screen::RoomSelect;
                    }
                }

                // Przyciski po prawej są częścią tego samego, zawijanego
                // układu — dzięki temu nigdy nie wychodzą poza okno.
                let mute_label = if self.audio.muted { "🔇" } else { "🔊" };
                if ui
                    .button(egui::RichText::new(mute_label).size(fs_btn))
                    .clicked()
                {
                    self.audio.muted = !self.audio.muted;
                    if !self.audio.muted {
                        self.audio.play(Sfx::Click);
                    }
                }

                if self.current_screen != Screen::Login {
                    ui.separator();
                    let account_label = if narrow {
                        "⚙ Konto"
                    } else {
                        "🔄 Tryb / Konto"
                    };
                    if ui
                        .button(egui::RichText::new(account_label).size(fs_btn))
                        .clicked()
                    {
                        self.audio.play(Sfx::Click);
                        self.net.disconnect_room();
                        self.last_room_order.clear();
                        self.last_sent_focus = None;
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
                                .size(fs_info)
                                .strong()
                                .color(egui::Color32::GOLD),
                            );
                        }
                    } else {
                        ui.label(
                            egui::RichText::new("🕹 Tryb Offline")
                                .size(fs_info)
                                .color(egui::Color32::LIGHT_GRAY),
                        );
                    }
                }
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
                    self.last_sent_focus = None;
                    self.last_my_pair_game = None;
                    self.last_watched_pair = None;
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
                self.last_sent_focus = None;
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