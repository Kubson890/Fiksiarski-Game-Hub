#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
pub mod audio;
pub mod battleship;
pub mod blackjack;
pub mod cards;
pub mod categories;
pub mod chess;
pub mod ludo;
pub mod net;
pub mod solitaire;
pub mod uno;
pub mod war;

use audio::{Sfx, SoundManager};
use eframe::egui;
use net::{NetClient, NetEvent};
use std::time::Instant;

pub fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "noto_emoji_full".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/NotoEmoji.ttf")),
    );

    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.push("noto_emoji_full".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        family.push("noto_emoji_full".to_owned());
    }

    ctx.set_fonts(fonts);
}

pub fn apply_hub_style(ctx: &egui::Context) {
    setup_custom_fonts(ctx);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(10.0);
    ctx.set_style(style);
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum Screen {
    #[default]
    Login,
    RoomSelect,
    Hub,
    Leaderboard,
    Chess,
    Solitaire,
    Blackjack,
    Ludo,
    Uno,
    Categories,
    War,
    Battleship,
}

#[derive(Default)]
pub struct GameHubApp {
    current_screen: Screen,
    is_online_session: bool,
    audio: SoundManager,
    net: NetClient,
    last_synced_chips: i32,
    last_room_order: Vec<usize>,
    last_room_focus: Vec<String>,
    last_room_game_slots: [Vec<usize>; 3],
    last_sent_focus: Option<String>,
    last_my_pair_game: Option<String>,
    last_watched_pair: Option<(String, usize)>,
    last_war_active: bool,
    peer_left_notice: Option<(String, Instant)>,
    blackjack: blackjack::BlackjackGame,
    solitaire: solitaire::SolitaireGame,
    chess: chess::ChessGame,
    ludo: ludo::LudoGame,
    uno: uno::UnoGame,
    categories: categories::CategoriesGame,
    war: war::WarGame,
    battleship: battleship::BattleshipGame,
}

pub fn is_two_player_screen(screen: Screen) -> bool {
    matches!(screen, Screen::Chess | Screen::Battleship | Screen::War)
}

pub fn screen_game_key(screen: Screen) -> Option<&'static str> {
    match screen {
        Screen::Chess => Some("chess"),
        Screen::Battleship => Some("battleship"),
        Screen::War => Some("war"),
        Screen::Ludo => Some("ludo"),
        Screen::Uno => Some("uno"),
        Screen::Categories => Some("categories"),
        _ => None,
    }
}

const ROOM_GAMES: [&str; 3] = ["ludo", "uno", "categories"];

pub fn is_room_game(game: &str) -> bool {
    ROOM_GAMES.contains(&game)
}

impl GameHubApp {
    fn reset_online_game(&mut self, game: &str) {
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
            "war" => {
                self.war.setup_online(self.net.my_pair_seat());
                let present = self.net.my_pair_active("war");
                self.war.set_enemy_present(present);
                self.last_war_active = present;
                if self.current_game_key() == "war" {
                    self.last_my_pair_game = Some("war".to_string());
                }
            }
            "ludo" | "uno" | "categories" => self.reset_room_game(game),
            _ => {}
        }
    }

    fn reset_room_game(&mut self, game: &str) {
        let slots = self.net.game_focused_slots(game);
        let my_seat = slots
            .iter()
            .position(|slot| *slot == self.net.my_slot)
            .unwrap_or(0);
        let count = slots.len();
        if let Some(idx) = ROOM_GAMES.iter().position(|g| *g == game) {
            self.last_room_game_slots[idx] = slots;
        }
        match game {
            "ludo" => self.ludo.setup_online(my_seat, count),
            "uno" => self.uno.setup_online(my_seat, count),
            "categories" => self.categories.setup_online(my_seat, count),
            _ => {}
        }
    }

    fn sync_room_games(&mut self) {
        if !self.is_online_session || self.net.connected_room.is_none() {
            return;
        }
        for (idx, game) in ROOM_GAMES.iter().enumerate() {
            let slots = self.net.game_focused_slots(game);
            if slots == self.last_room_game_slots[idx] {
                continue;
            }
            self.last_room_game_slots[idx] = slots.clone();
            let my_seat = slots
                .iter()
                .position(|slot| *slot == self.net.my_slot)
                .unwrap_or(0);
            let count = slots.len();
            match *game {
                "ludo" => self.ludo.setup_online(my_seat, count),
                "uno" => self.uno.setup_online(my_seat, count),
                "categories" => self.categories.setup_online(my_seat, count),
                _ => {}
            }
        }
    }

    fn handle_room_state_change(&mut self) {
        let new_order = self.net.room_order.clone();
        let new_focus = self.net.room_focus.clone();
        if new_order == self.last_room_order && new_focus == self.last_room_focus {
            return;
        }

        let old_order = std::mem::take(&mut self.last_room_order);
        std::mem::take(&mut self.last_room_focus);
        let partner_slot = self.net.my_pair() * 2 + (1 - self.net.my_pair_seat());
        let old_partner = old_order.contains(&partner_slot);
        let new_partner = new_order.contains(&partner_slot);

        if old_partner != new_partner {
            self.last_my_pair_game = None;
            self.last_watched_pair = None;
            self.last_war_active = false;
        }

        if new_order.len() > old_order.len() {
            self.audio.play(Sfx::Check);
        } else if new_order.len() < old_order.len() {
            self.audio.play(Sfx::Click);
        }

        self.last_room_order = new_order;
        self.last_room_focus = new_focus;
        self.sync_room_games();
    }

    fn sync_game_modes(&mut self) {
        if !self.is_online_session || self.net.connected_room.is_none() {
            return;
        }
        let screen_game = match self.current_screen {
            Screen::Chess => Some("chess"),
            Screen::Battleship => Some("battleship"),
            Screen::War => Some("war"),
            _ => None,
        };

        if let Some(prev) = self.last_my_pair_game.clone() {
            if screen_game != Some(prev.as_str()) {
                self.last_my_pair_game = None;
                self.last_watched_pair = None;
                let seat = self.net.my_pair_seat();
                match prev.as_str() {
                    "chess" => self.chess.setup_online(seat),
                    "battleship" => self.battleship.setup_online(seat),
                    "war" => {
                        self.war.reset_online_waiting();
                        self.last_war_active = false;
                    }
                    _ => {}
                }
            }
        }

        let Some(game) = screen_game else {
            self.last_watched_pair = None;
            self.sync_room_games();
            return;
        };
        let seat = self.net.my_pair_seat();
        let count = self.net.spectator_count_for_game(game);

        if game == "war" {
            let active = self.net.pair_active(self.net.my_pair(), "war");
            if !self.war.online_mode || (active && !self.last_war_active) {
                self.war.setup_online(seat);
            }
            self.last_war_active = active;
            self.war.online_mode = true;
            self.war.set_enemy_present(active);
            self.last_my_pair_game = Some("war".to_string());
            self.last_watched_pair = None;
            self.sync_room_games();
            return;
        }

        if self.net.my_pair_active(game) {
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
                self.last_my_pair_game = None;
                self.last_watched_pair = Some(watch_key);
                self.net.send_event(&NetEvent::StateRequest {
                    game: game.to_string(),
                    pair: owner,
                });
            }
        } else {
            if self.last_my_pair_game.as_deref() == Some(game) {
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
        self.sync_room_games();
    }

    fn handle_state_request(&mut self, game: &str, pair: usize) {
        if game == "chess" || game == "battleship" || game == "war" {
            if self.net.my_pair() != pair || !self.net.my_pair_active(game) {
                return;
            }
        }
        if let Some(state) = self.game_state_json(game) {
            let room_game = is_room_game(game);
            self.net.send_event(&NetEvent::GameState {
                game: game.to_string(),
                from_seat: if room_game {
                    self.net.room_game_seat(game).unwrap_or(0)
                } else {
                    self.net.my_pair_seat()
                },
                pair: if room_game { 2 } else { pair },
                state,
            });
        }
    }

    fn game_state_json(&self, game: &str) -> Option<serde_json::Value> {
        match game {
            "chess" => Some(self.chess.state_json()),
            "battleship" => Some(self.battleship.state_json(self.net.my_pair_seat())),
            "ludo" => Some(self.ludo.state_json()),
            "uno" => Some(self.uno.state_json()),
            "categories" => Some(self.categories.state_json()),
            _ => None,
        }
    }

    fn apply_game_state(&mut self, game: &str, pair: usize, state: &serde_json::Value) {
        if state.get("cat_sync").and_then(|v| v.as_bool()) == Some(true) {
            self.categories.apply_state_json(state, &self.audio);
            return;
        }

        if game == "chess" || game == "battleship" {
            if self.net.playing_pair(game) != Some(pair) || pair == self.net.my_pair() {
                return;
            }
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
            "uno" => {
                self.uno.apply_state_json(state);
            }
            "categories" => {
                self.categories.apply_state_json(state, &self.audio);
            }
            _ => {}
        }
    }

    fn open_game(&mut self, screen: Screen) {
        self.audio.play(Sfx::Click);
        self.current_screen = screen;
        if self.is_online_session && self.net.connected_room.is_some() {
            if let Some(game) = screen_game_key(screen) {
                if is_room_game(game) {
                    self.reset_room_game(game);
                }
            }
        }
    }

    fn leave_online_games(&mut self) {
        self.chess.online_mode = false;
        self.chess.spectator = false;
        self.chess.waiting = false;
        self.ludo.online_mode = false;
        self.ludo.online_lobby = false;
        self.uno.online_mode = false;
        self.uno.online_lobby = false;
        self.war.online_mode = false;
        self.war.online_lobby = false;
        self.war.new_game();
        self.battleship.leave_online();
        self.last_my_pair_game = None;
        self.last_watched_pair = None;
        self.last_war_active = false;
        self.last_room_game_slots = Default::default();
        self.peer_left_notice = None;
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

    fn can_request_reset(&self) -> bool {
        if !self.is_online_session || self.net.connected_room.is_none() {
            return false;
        }
        let game = self.current_game_key();
        if game == "chess" || game == "battleship" || game == "war" {
            self.net.my_pair_active(game) && !self.net.is_spectator_for_game(game)
        } else {
            is_room_game(game) && self.net.room_game_players(game) >= 2
        }
    }

    fn can_vote_on_pending_reset(&self) -> bool {
        let Some(game) = self.net.incoming_reset_game.as_deref() else {
            return false;
        };
        let Some(pair) = self.net.incoming_reset_pair else {
            return false;
        };
        if game == "chess" || game == "battleship" || game == "war" {
            return pair == self.net.my_pair()
                && self.net.my_pair_active(game)
                && self.net.incoming_reset_from != Some(self.net.my_pair_seat());
        }
        if is_room_game(game) {
            return self.net.room_game_seat(game).is_some();
        }
        false
    }
}

pub fn game_label(key: &str) -> &'static str {
    match key {
        "chess" => "Szachy",
        "battleship" => "Statki",
        "ludo" => "Chińczyk",
        "uno" => "UNO",
        "categories" => "Państwa-Miasta",
        "war" => "Wojna",
        _ => "grę",
    }
}

impl eframe::App for GameHubApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !ctx.style().visuals.dark_mode {
            let mut style = (*ctx.style()).clone();
            style.visuals = egui::Visuals::dark();
            style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
            style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
            style.visuals.widgets.hovered.rounding = egui::Rounding::same(10.0);
            ctx.set_style(style);
        }

        if let Some(ok) = self.net.poll_login() {
            if ok {
                if let Some(u) = &self.net.current_user {
                    self.blackjack.set_chips(u.chips);
                    self.last_synced_chips = u.chips;
                }
                if self.current_screen == Screen::Login {
                    self.is_online_session = true;
                    self.current_screen = Screen::RoomSelect;
                }
            }
            ctx.request_repaint();
        }
        self.net.poll_updates();
        if self.net.login_pending {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }

        if self.is_online_session && self.net.connected_room.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
            let events = self.net.poll_events();
            self.handle_room_state_change();
            self.sync_game_modes();

            for ev in events {
                match ev {
                    NetEvent::Assigned { .. }
                    | NetEvent::RoomState { .. }
                    | NetEvent::FocusGame { .. } => {}
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
                        if self.current_screen == Screen::Ludo && self.ludo.online_mode {
                            self.ludo.apply_remote_roll(face, &self.audio);
                        }
                    }
                    NetEvent::LudoMove { pawn_idx } => {
                        if self.current_screen == Screen::Ludo && self.ludo.online_mode {
                            self.ludo.apply_remote_pawn_move(pawn_idx, &self.audio);
                        }
                    }
                    NetEvent::LudoStart => {
                        if self.ludo.online_mode {
                            self.audio.play(Sfx::Click);
                            self.ludo.apply_remote_start();
                        }
                    }
                    NetEvent::UnoStart { seed } => {
                        if self.current_screen == Screen::Uno && self.uno.online_mode {
                            self.audio.play(Sfx::CardSlide);
                            self.uno.apply_remote_start(seed);
                        }
                    }
                    NetEvent::UnoPlay {
                        card_idx,
                        chosen_color,
                    } => {
                        if self.current_screen == Screen::Uno && self.uno.online_mode {
                            self.uno
                                .apply_remote_play(card_idx, chosen_color, &self.audio);
                        }
                    }
                    NetEvent::UnoDraw => {
                        if self.current_screen == Screen::Uno && self.uno.online_mode {
                            self.uno.apply_remote_draw(&self.audio);
                        }
                    }
                    NetEvent::CategoriesStart { letter, round } => {
                        self.categories.apply_start_round(letter, round, &self.audio);
                    }
                    NetEvent::CategoriesStop { from_room_seat } => {
                        self.categories
                            .apply_remote_stop(from_room_seat, &self.audio);
                    }
                    NetEvent::CategoriesSubmit {
                        answers,
                        from_room_seat,
                    } => {
                        self.categories
                            .apply_remote_submit(from_room_seat, answers, &self.audio);
                    }
                    NetEvent::CategoriesVote {
                        votes,
                        from_room_seat,
                    } => {
                        self.categories
                            .apply_remote_vote(from_room_seat, votes, &self.audio);
                    }
                    NetEvent::WarReady { from_seat } => {
                        if self.current_screen == Screen::War && self.war.online_mode {
                            self.war.apply_remote_ready(from_seat, &self.audio);
                        }
                    }
                    NetEvent::WarStart { seed } => {
                        if self.current_screen == Screen::War && self.war.online_mode {
                            self.audio.play(Sfx::CardSlide);
                            self.war.apply_remote_start(seed);
                        }
                    }
                    NetEvent::WarStep => {
                        if self.current_screen == Screen::War && self.war.online_mode {
                            self.war.apply_remote_step(&self.audio);
                        }
                    }
                    NetEvent::PeerLeftGame { slot, nick, game } => {
                        let who = if nick.trim().is_empty() {
                            format!("Gracz #{}", slot + 1)
                        } else {
                            nick
                        };
                        let msg = format!("🚪 {} wyszedł z gry: {}", who, game_label(&game));
                        self.peer_left_notice = Some((msg, Instant::now()));
                        self.audio.play(Sfx::Click);
                    }
                    NetEvent::GameClosed { game, pair } => {
                        self.net.reset_requested_by_me = false;
                        self.net.incoming_reset_from = None;
                        self.net.incoming_reset_game = None;
                        self.net.incoming_reset_pair = None;
                        if pair == self.net.my_pair() || pair == 2 {
                            self.reset_online_game(&game);
                        }
                        let msg = format!("🔄 Gra {} została zresetowana.", game_label(&game));
                        self.peer_left_notice = Some((msg, Instant::now()));
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
                    NetEvent::Ping | NetEvent::PeerNick { .. } => {}
                }
            }

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
            if self.ludo.outgoing_start {
                self.ludo.outgoing_start = false;
                self.net.send_event(&NetEvent::LudoStart);
            }
            if let Some(seed) = self.uno.outgoing_start_seed.take() {
                self.net.send_event(&NetEvent::UnoStart { seed });
            }
            if let Some((card_idx, chosen_color)) = self.uno.outgoing_play.take() {
                self.net.send_event(&NetEvent::UnoPlay {
                    card_idx,
                    chosen_color,
                });
            }
            if self.uno.outgoing_draw {
                self.uno.outgoing_draw = false;
                self.net.send_event(&NetEvent::UnoDraw);
            }
            if let Some((letter, round)) = self.categories.outgoing_start.take() {
                self.net
                    .send_event(&NetEvent::CategoriesStart { letter, round });
            }
            if self.categories.outgoing_stop {
                self.categories.outgoing_stop = false;
                self.net.send_event(&NetEvent::CategoriesStop {
                    from_room_seat: self.categories.my_player_idx,
                });
            }
            if let Some(answers) = self.categories.outgoing_submit.take() {
                self.net.send_event(&NetEvent::CategoriesSubmit {
                    answers,
                    from_room_seat: self.categories.my_player_idx,
                });
            }
            if let Some(votes) = self.categories.outgoing_vote.take() {
                self.net.send_event(&NetEvent::CategoriesVote {
                    votes,
                    from_room_seat: self.categories.my_player_idx,
                });
            }
            if self.categories.force_broadcast_state {
                self.categories.force_broadcast_state = false;
                self.net.send_event(&NetEvent::GameState {
                    game: "categories".to_string(),
                    from_seat: self.categories.my_player_idx,
                    pair: 2,
                    state: self.categories.state_json(),
                });
            }
            if self.war.outgoing_ready {
                self.war.outgoing_ready = false;
                self.net.send_event(&NetEvent::WarReady {
                    from_seat: self.net.my_pair_seat(),
                });
            }
            if let Some(seed) = self.war.outgoing_start_seed.take() {
                self.net.send_event(&NetEvent::WarStart { seed });
            }
            if self.war.outgoing_step {
                self.war.outgoing_step = false;
                if self.war.online_mode && self.net.my_pair() == 0 {
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
            let bar_width = ui.available_width();
            let scale = (bar_width / 1400.0).clamp(0.72, 1.0);
            let narrow = bar_width < 860.0;
            let fs_title = 19.0 * scale;
            let fs_btn = 14.5 * scale;
            let fs_info = 13.5 * scale;
            let fs_small = 12.5 * scale;

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = (8.0 * scale).max(4.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.spacing_mut().button_padding = egui::vec2(8.0 * scale, 4.5 * scale);

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
                        .button(egui::RichText::new("⬅ Menu").size(fs_btn))
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

                        if let Some((notice, at)) = self.peer_left_notice.clone() {
                            if at.elapsed().as_secs_f32() < 8.0 {
                                ui.separator();
                                ui.label(
                                    egui::RichText::new(notice)
                                        .size(fs_info)
                                        .strong()
                                        .color(egui::Color32::from_rgb(255, 190, 90)),
                                );
                            } else {
                                self.peer_left_notice = None;
                            }
                        }

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
                                    "⏳ Czekam..."
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
                                let g_key = self.current_game_key().to_string();
                                let pair_id = if is_room_game(&g_key) {
                                    2
                                } else {
                                    self.net.my_pair()
                                };
                                self.net.send_event(&NetEvent::ResetRequest {
                                    from_player: self.net.my_pair_seat(),
                                    game: g_key,
                                    pair: pair_id,
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
                            egui::RichText::new("🕹 Offline")
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
                Screen::Uno => {
                    self.uno.ui(ui, &self.audio);
                    if self.uno.newly_won {
                        self.uno.newly_won = false;
                        if self.is_online_session {
                            self.net.report_progress("uno", 1, None);
                        }
                    }
                }
                Screen::Categories => {
                    self.categories.ui(ui, &self.audio);
                    if self.categories.newly_won {
                        self.categories.newly_won = false;
                        if self.is_online_session {
                            self.net.report_progress("categories", 1, None);
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

#[cfg(not(target_os = "android"))]
pub fn run_desktop() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([340.0, 560.0])
            .with_title("Fiksiarski Game Hub"),
        ..Default::default()
    };

    eframe::run_native(
        "Fiksiarski Game Hub",
        options,
        Box::new(|cc| {
            setup_custom_fonts(&cc.egui_ctx);
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

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(_app: winit::platform::android::activity::AndroidApp) {
    std::env::set_var("RUST_BACKTRACE", "full");

    let options = eframe::NativeOptions::default();

    let _ = eframe::run_native(
        "Fiksiarski Game Hub",
        options,
        Box::new(|cc| {
            setup_custom_fonts(&cc.egui_ctx);
            let mut style = (*cc.egui_ctx.style()).clone();
            style.visuals = egui::Visuals::dark();
            style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
            style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
            style.visuals.widgets.hovered.rounding = egui::Rounding::same(10.0);
            cc.egui_ctx.set_style(style);
            Ok(Box::new(GameHubApp::default()))
        }),
    );
}

impl GameHubApp {
    fn render_login(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        let card_width = (avail.x - 32.0).clamp(280.0, 410.0);
        let btn_w = (card_width - 48.0).max(220.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((avail.y * 0.10).max(16.0));

                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(32, 36, 46))
                        .rounding(16.0)
                        .inner_margin(24.0)
                        .stroke(egui::Stroke::new(
                            1.5_f32,
                            egui::Color32::from_rgb(70, 80, 105),
                        ))
                        .show(ui, |ui| {
                            ui.set_width(card_width);

                            if self.net.has_saved_account && !self.net.show_account_form {
                                ui.heading(
                                    egui::RichText::new(format!(
                                        "👋 Witaj ponownie, {}!",
                                        self.net.username_input
                                    ))
                                    .size(24.0)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                                );
                                ui.add_space(8.0);
                                ui.label("Wybierz w jakim trybie chcesz dzisiaj grać:");
                                ui.add_space(20.0);

                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(
                                                "🌐 Graj Online (Wybierz Pokój, Rankingi)",
                                            )
                                            .size(16.0)
                                            .strong(),
                                        )
                                        .min_size(egui::vec2(btn_w, 48.0)),
                                    )
                                    .clicked()
                                {
                                    self.audio.play(Sfx::Click);
                                    self.net.start_login(false);
                                }

                                ui.add_space(12.0);

                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(
                                                "🕹 Graj Offline (Lokalnie z Botami)",
                                            )
                                            .size(16.0),
                                        )
                                        .min_size(egui::vec2(btn_w, 46.0)),
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
                                        .size(25.0)
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                );
                                ui.add_space(6.0);
                                ui.label("Zaloguj się lub utwórz konto (zostanie zapamiętane)");
                                ui.add_space(16.0);

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

                                ui.add_space(18.0);

                                let half_btn = ((btn_w - 10.0) / 2.0).max(110.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("🚀 Zaloguj")
                                                    .size(16.0)
                                                    .strong(),
                                            )
                                            .min_size(egui::vec2(half_btn, 42.0)),
                                        )
                                        .clicked()
                                    {
                                        self.audio.play(Sfx::Click);
                                        self.net.start_login(false);
                                    }

                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("✨ Załóż konto").size(16.0),
                                            )
                                            .min_size(egui::vec2(half_btn, 42.0)),
                                        )
                                        .clicked()
                                    {
                                        self.audio.play(Sfx::Click);
                                        self.net.start_login(true);
                                    }
                                });

                                ui.add_space(14.0);
                                ui.separator();
                                ui.add_space(8.0);

                                if ui
                                    .button(
                                        egui::RichText::new("🕹 Graj Offline jako Gość")
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

                            if self.net.login_pending {
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(
                                        egui::RichText::new("Łączenie z serwerem…")
                                            .color(egui::Color32::LIGHT_GRAY),
                                    );
                                });
                            }
                        });
                });
            });
    }

    fn render_room_select(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        let is_portrait = avail.y > avail.x || avail.x < 680.0;
        let ui_scale = (avail.x / 1100.0).min(avail.y / 720.0).clamp(0.75, 1.3);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((avail.y * 0.08).max(16.0));

                    ui.heading(
                        egui::RichText::new("🌐 Wybierz Pokój Multiplayer")
                            .size((30.0 * ui_scale).clamp(20.0, 36.0))
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Kliknij na wybrany pokój, aby usiąść przy stole i przejść do gier:",
                        )
                        .size((15.5 * ui_scale).clamp(12.5, 19.0))
                        .color(egui::Color32::LIGHT_GRAY),
                    );
                    ui.add_space(12.0);

                    if ui
                        .button(egui::RichText::new("🔄 Odśwież stan pokoi").size(15.0))
                        .clicked()
                    {
                        self.audio.play(Sfx::Click);
                        self.net.fetch_rooms();
                    }

                    ui.add_space(20.0);

                    let rooms_snapshot = self.net.available_rooms.clone();
                    let mut chosen_room: Option<String> = None;

                    if is_portrait {
                        let card_w = (avail.x * 0.88).clamp(240.0, 420.0);
                        let card_h = 135.0;
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
                            let resp = Self::game_card(
                                ui,
                                egui::vec2(card_w, card_h),
                                &title,
                                &subtitle,
                                0,
                                true,
                            );
                            if resp.clicked() && !is_full {
                                chosen_room = Some(r.room_id.clone());
                            }
                            ui.add_space(14.0);
                        }
                    } else {
                        let spacing = (avail.x * 0.03).clamp(16.0, 36.0);
                        let card_w = ((avail.x * 0.82 - spacing) / 2.0).clamp(240.0, 420.0);
                        let card_h = (avail.y * 0.30).clamp(140.0, 220.0);
                        let total_w = card_w * 2.0 + spacing;
                        let left_pad = ((avail.x - total_w) / 2.0).max(0.0);

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

                                let resp = Self::game_card(
                                    ui,
                                    egui::vec2(card_w, card_h),
                                    &title,
                                    &subtitle,
                                    0,
                                    true,
                                );
                                if resp.clicked() && !is_full {
                                    chosen_room = Some(r.room_id.clone());
                                }
                                if idx == 0 {
                                    ui.add_space(spacing);
                                }
                            }
                        });
                    }

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

                    ui.add_space(24.0);

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
                    ui.add_space(16.0);
                });
            });
    }

    fn render_hub(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        let is_portrait = avail.y > avail.x || avail.x < 760.0;
        let cols: usize = if is_portrait { 2 } else { 4 };
        let spacing = (avail.x.min(avail.y) * 0.026).clamp(10.0, 26.0);
        let card_w = if is_portrait {
            ((avail.x * 0.92 - spacing) / 2.0).clamp(145.0, 310.0)
        } else {
            ((avail.x * 0.90 - spacing * 3.0) / 4.0).clamp(180.0, 340.0)
        };
        let card_h = if is_portrait {
            (card_w * 0.72).clamp(105.0, 175.0)
        } else {
            ((avail.y * 0.64 - spacing) / 2.0).clamp(130.0, 265.0)
        };
        let card_size = egui::vec2(card_w, card_h);

        let title_size = (avail.y * 0.05).clamp(24.0, 48.0);
        let rows = if is_portrait { 4.0 } else { 2.0 };
        let total_content_h = title_size + spacing * rows + card_h * rows;
        let top_padding = ((avail.y - total_content_h) / 2.0).max(12.0);

        let chess_spectators = self.spectator_count_for_screen(Screen::Chess);
        let battleship_spectators = self.spectator_count_for_screen(Screen::Battleship);
        let mp_room_connected = self.is_online_session && self.net.connected_room.is_some();

        let cat_sub = if mp_room_connected {
            "Słowna rywalizacja z głosowaniem 3/4"
        } else {
            "Dostępne tylko w pokoju Multiplayer!"
        };

        let items: [(Screen, &str, &str, usize, bool); 8] = [
            (
                Screen::Chess,
                "♟ Szachy",
                "Klasyczny pojedynek na szachownicy",
                chess_spectators,
                true,
            ),
            (
                Screen::Battleship,
                "🚢 Statki",
                "Taktyczna bitwa morska na radarze",
                battleship_spectators,
                true,
            ),
            (
                Screen::Ludo,
                "🎲 Chińczyk",
                "Planszówka Ludo dla 2-4 graczy",
                0,
                true,
            ),
            (
                Screen::Uno,
                "🌈 UNO",
                "Kolorowa gra karciana dla 2-4 graczy",
                0,
                true,
            ),
            (
                Screen::Solitaire,
                "🃏 Pasjans",
                "Klasyczny Klondike Solitaire",
                0,
                true,
            ),
            (
                Screen::Blackjack,
                "🎰 Blackjack",
                "Oko / 21 przeciwko krupierowi",
                0,
                true,
            ),
            (
                Screen::War,
                "⚔ Wojna",
                "Szybki pojedynek o całą talię",
                0,
                true,
            ),
            (
                Screen::Categories,
                "🌍 Państwa-Miasta",
                cat_sub,
                0,
                mp_room_connected,
            ),
        ];

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(top_padding);
                    ui.heading(
                        egui::RichText::new("Wybierz grę")
                            .size(title_size)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                    ui.add_space(spacing);

                    let total_grid_w =
                        card_w * cols as f32 + spacing * (cols.saturating_sub(1)) as f32;
                    let left_pad = ((avail.x - total_grid_w) / 2.0).max(0.0);

                    ui.horizontal(|ui| {
                        ui.add_space(left_pad);

                        egui::Grid::new("games_grid")
                            .spacing([spacing, spacing])
                            .show(ui, |ui| {
                                for (idx, &(screen, title, sub, spec, enabled)) in
                                    items.iter().enumerate()
                                {
                                    if Self::game_card(ui, card_size, title, sub, spec, enabled)
                                        .clicked()
                                        && enabled
                                    {
                                        self.open_game(screen);
                                    }
                                    if (idx + 1) % cols == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                    ui.add_space(16.0);
                });
            });
    }

    fn game_card(
        ui: &mut egui::Ui,
        desired_size: egui::Vec2,
        title: &str,
        subtitle: &str,
        spectators: usize,
        enabled: bool,
    ) -> egui::Response {
        let sense = if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(desired_size, sense);

        if ui.is_rect_visible(rect) {
            let scale_h = desired_size.y / 160.0;
            let scale_w = desired_size.x / 310.0;
            let scale = scale_h.min(scale_w).clamp(0.58, 1.35);

            let visuals = ui.style().interact(&response);

            let expansion = if enabled && response.hovered() {
                4.0 * scale
            } else {
                0.0
            };
            let draw_rect = rect.expand(expansion);

            ui.painter().rect_filled(
                draw_rect.translate(egui::vec2(4.0 * scale, 5.0 * scale)),
                14.0 * scale,
                egui::Color32::from_black_alpha(90),
            );

            let bg_fill = if enabled {
                visuals.bg_fill
            } else {
                egui::Color32::from_rgb(24, 26, 33)
            };
            let bg_stroke = if enabled {
                visuals.bg_stroke
            } else {
                egui::Stroke::new(1.2 * scale, egui::Color32::from_rgb(55, 58, 68))
            };

            ui.painter()
                .rect(draw_rect, 14.0 * scale, bg_fill, bg_stroke);

            let title_font = (22.0 * scale).clamp(14.0, 34.0);
            let sub_font = (12.0 * scale).clamp(9.5, 18.0);

            let text_pos = draw_rect.center() - egui::vec2(0.0, 14.0 * scale);
            ui.painter().text(
                text_pos,
                egui::Align2::CENTER_CENTER,
                title,
                egui::FontId::proportional(title_font),
                if enabled {
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_rgb(115, 120, 132)
                },
            );

            let sub_pos = draw_rect.center() + egui::vec2(0.0, 19.0 * scale);
            ui.painter().text(
                sub_pos,
                egui::Align2::CENTER_CENTER,
                subtitle,
                egui::FontId::proportional(sub_font),
                if enabled {
                    egui::Color32::LIGHT_GRAY
                } else {
                    egui::Color32::from_rgb(205, 125, 95)
                },
            );

            if !enabled {
                let lock_badge = egui::Rect::from_center_size(
                    egui::pos2(draw_rect.center().x, draw_rect.top() + 16.0 * scale),
                    egui::vec2(145.0 * scale, 20.0 * scale),
                );
                ui.painter().rect_filled(
                    lock_badge,
                    6.0 * scale,
                    egui::Color32::from_rgba_unmultiplied(60, 30, 30, 210),
                );
                ui.painter().text(
                    lock_badge.center(),
                    egui::Align2::CENTER_CENTER,
                    "TYLKO MULTIPLAYER",
                    egui::FontId::proportional((10.0 * scale).max(8.0)),
                    egui::Color32::from_rgb(255, 175, 130),
                );
            }

            if spectators > 0 && enabled {
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

        if enabled && response.hovered() {
            ui.ctx().request_repaint();
        }

        response
    }
}