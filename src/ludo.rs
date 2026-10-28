use crate::audio::{Sfx, SoundManager};
use eframe::egui;
use rand::Rng;
use serde_json::{json, Value};
use std::time::Instant;

const MAIN_TRACK: [(usize, usize); 52] = [
    (1, 6), (2, 6), (3, 6), (4, 6), (5, 6),
    (6, 5), (6, 4), (6, 3), (6, 2), (6, 1), (6, 0),
    (7, 0), (8, 0),
    (8, 1), (8, 2), (8, 3), (8, 4), (8, 5),
    (9, 6), (10, 6), (11, 6), (12, 6), (13, 6), (14, 6),
    (14, 7), (14, 8),
    (13, 8), (12, 8), (11, 8), (10, 8), (9, 8),
    (8, 9), (8, 10), (8, 11), (8, 12), (8, 13), (8, 14),
    (7, 14), (6, 14),
    (6, 13), (6, 12), (6, 11), (6, 10), (6, 9),
    (5, 8), (4, 8), (3, 8), (2, 8), (1, 8), (0, 8),
    (0, 7), (0, 6),
];

const SAFE_GLOB_INDICES: [usize; 8] = [0, 8, 13, 21, 26, 34, 39, 47];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayerType {
    Human,
    Bot,
    Disabled, // Wyłączone miejsce (np. w grze online na 2 lub 3 osoby)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TurnPhase {
    WaitForRoll,
    RollingDice {
        start: Instant,
        last_tick: Instant,
        preview_face: u8,
        final_face: u8,
    },
    WaitForMove,
    AnimatingPawn,
    GameOver(usize),
}

struct StepAnimation {
    player: usize,
    pawn_idx: usize,
    path: Vec<u8>,
    current_leg: usize,
    leg_start: Instant,
    leg_duration: f32,
}

pub struct LudoGame {
    pawns: [[u8; 4]; 4],
    player_types: [PlayerType; 4],
    current_turn: usize,
    dice_value: u8,
    phase: TurnPhase,
    step_anim: Option<StepAnimation>,
    bot_wait_until: Option<Instant>,
    status_text: String,
    pub newly_won: bool,
    pub online_mode: bool,
    pub my_player_idx: usize,
    pub outgoing_roll: Option<u8>,
    pub outgoing_pawn: Option<usize>,
}

impl Default for LudoGame {
    fn default() -> Self {
        let mut g = Self {
            pawns: [[0; 4]; 4],
            player_types: [
                PlayerType::Human,
                PlayerType::Bot,
                PlayerType::Bot,
                PlayerType::Bot,
            ],
            current_turn: 0,
            dice_value: 6,
            phase: TurnPhase::WaitForRoll,
            step_anim: None,
            bot_wait_until: None,
            status_text: String::new(),
            newly_won: false,
            online_mode: false,
            my_player_idx: 0,
            outgoing_roll: None,
            outgoing_pawn: None,
        };
        g.new_game();
        g
    }
}

impl LudoGame {
    pub fn new_game(&mut self) {
        self.pawns = [[0; 4]; 4];
        self.current_turn = 0;
        self.dice_value = 1;
        self.phase = TurnPhase::WaitForRoll;
        self.step_anim = None;
        self.bot_wait_until = None;
        self.newly_won = false;
        self.outgoing_roll = None;
        self.outgoing_pawn = None;
        self.status_text = "🔴 Czerwony zaczyna! Rzuć kostką.".to_string();
    }

    pub fn setup_online(&mut self, my_idx: usize, players_count: usize) {
        self.online_mode = true;
        self.my_player_idx = my_idx.min(3);
        let active_humans = players_count.clamp(2, 4);
        for i in 0..4 {
            // W trybie Online NIE MA BOTÓW: aktywni są tylko żywi gracze, reszta miejsc jest Disabled!
            self.player_types[i] = if i < active_humans {
                PlayerType::Human
            } else {
                PlayerType::Disabled
            };
        }
        self.new_game();
    }

    pub fn state_json(&self) -> Value {
        let phase = match self.phase {
            TurnPhase::WaitForRoll | TurnPhase::RollingDice { .. } => "wait_roll",
            TurnPhase::WaitForMove | TurnPhase::AnimatingPawn => "wait_move",
            TurnPhase::GameOver(_) => "over",
        };
        let player_types: Vec<&str> = self
            .player_types
            .iter()
            .map(|kind| match kind {
                PlayerType::Human => "human",
                PlayerType::Bot => "bot",
                PlayerType::Disabled => "disabled",
            })
            .collect();
        json!({
            "pawns": self.pawns,
            "player_types": player_types,
            "current_turn": self.current_turn,
            "dice_value": self.dice_value,
            "phase": phase,
            "status": &self.status_text,
        })
    }

    pub fn apply_state_json(&mut self, state: &Value) -> bool {
        let Some(pawns) = state.get("pawns").and_then(Value::as_array) else {
            return false;
        };
        let mut new_pawns = [[0u8; 4]; 4];
        for (player, row) in pawns.iter().take(4).enumerate() {
            let Some(values) = row.as_array() else {
                return false;
            };
            for (pawn, value) in values.iter().take(4).enumerate() {
                new_pawns[player][pawn] = value.as_u64().unwrap_or(0) as u8;
            }
        }
        if let Some(types) = state.get("player_types").and_then(Value::as_array) {
            for (idx, kind) in types.iter().take(4).enumerate() {
                self.player_types[idx] = match kind.as_str().unwrap_or("human") {
                    "bot" => PlayerType::Bot,
                    "disabled" => PlayerType::Disabled,
                    _ => PlayerType::Human,
                };
            }
        }
        self.pawns = new_pawns;
        self.current_turn = state
            .get("current_turn")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(3) as usize;
        self.dice_value = state
            .get("dice_value")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .clamp(1, 6) as u8;
        self.status_text = state
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        self.phase = match state.get("phase").and_then(Value::as_str).unwrap_or("wait_roll") {
            "wait_move" => TurnPhase::WaitForMove,
            "over" => TurnPhase::GameOver(self.current_turn),
            _ => TurnPhase::WaitForRoll,
        };
        self.step_anim = None;
        self.bot_wait_until = None;
        self.outgoing_roll = None;
        self.outgoing_pawn = None;
        self.newly_won = false;
        self.online_mode = true;
        true
    }

    pub fn apply_remote_roll(&mut self, face: u8, audio: &SoundManager) {
        audio.play(Sfx::CardSlide);
        let now = Instant::now();
        self.phase = TurnPhase::RollingDice {
            start: now,
            last_tick: now,
            preview_face: face,
            final_face: face,
        };
    }

    pub fn apply_remote_pawn_move(&mut self, pawn_idx: usize, audio: &SoundManager) {
        audio.play(Sfx::Click);
        self.begin_pawn_move(self.current_turn, pawn_idx);
        self.outgoing_pawn = None;
    }

    fn player_name(p: usize) -> &'static str {
        match p {
            0 => "🔴 Czerwony",
            1 => "🟢 Zielony",
            2 => "🟡 Żółty",
            _ => "🔵 Niebieski",
        }
    }

    fn player_color(p: usize) -> egui::Color32 {
        match p {
            0 => egui::Color32::from_rgb(225, 55, 55),
            1 => egui::Color32::from_rgb(45, 185, 75),
            2 => egui::Color32::from_rgb(235, 195, 35),
            _ => egui::Color32::from_rgb(50, 125, 235),
        }
    }

    fn rel_to_global(player: usize, rel_step: u8) -> Option<usize> {
        if (1..=51).contains(&rel_step) {
            Some(((rel_step as usize - 1) + player * 13) % 52)
        } else {
            None
        }
    }

    fn state_to_grid(player: usize, pawn_idx: usize, state: u8) -> (f32, f32) {
        if state == 0 {
            let base_origin = match player {
                0 => (1.5, 1.5),
                1 => (10.5, 1.5),
                2 => (10.5, 10.5),
                _ => (1.5, 10.5),
            };
            let dx = if pawn_idx % 2 == 0 { 0.5 } else { 2.5 };
            let dy = if pawn_idx < 2 { 0.5 } else { 2.5 };
            return (base_origin.0 + dx, base_origin.1 + dy);
        }

        if (1..=51).contains(&state) {
            let glob = Self::rel_to_global(player, state).unwrap();
            let (c, r) = MAIN_TRACK[glob];
            return (c as f32 + 0.5, r as f32 + 0.5);
        }

        if (52..=56).contains(&state) {
            let step_in_home = (state - 51) as f32;
            return match player {
                0 => (step_in_home + 0.5, 7.5),
                1 => (7.5, step_in_home + 0.5),
                2 => (14.5 - step_in_home, 7.5),
                _ => (7.5, 14.5 - step_in_home),
            };
        }

        let offset = (pawn_idx as f32 - 1.5) * 0.28;
        match player {
            0 => (6.6, 7.5 + offset),
            1 => (7.5 + offset, 6.6),
            2 => (8.4, 7.5 + offset),
            _ => (7.5 + offset, 8.4),
        }
    }

    fn can_move_pawn(&self, player: usize, pawn_idx: usize, dice: u8) -> bool {
        let pos = self.pawns[player][pawn_idx];
        if pos == 0 {
            dice == 6
        } else if pos < 57 {
            pos + dice <= 57
        } else {
            false
        }
    }

    fn legal_pawns(&self, player: usize, dice: u8) -> Vec<usize> {
        (0..4)
            .filter(|&idx| self.can_move_pawn(player, idx, dice))
            .collect()
    }

    fn start_dice_roll(&mut self, audio: &SoundManager) {
        audio.play(Sfx::CardSlide);
        let mut rng = rand::thread_rng();
        let final_face = rng.gen_range(1..=6);
        if self.online_mode && self.current_turn == self.my_player_idx {
            self.outgoing_roll = Some(final_face);
        }
        let now = Instant::now();
        self.phase = TurnPhase::RollingDice {
            start: now,
            last_tick: now,
            preview_face: rng.gen_range(1..=6),
            final_face,
        };
    }

    fn begin_pawn_move(&mut self, player: usize, pawn_idx: usize) {
        if self.online_mode && player == self.my_player_idx {
            self.outgoing_pawn = Some(pawn_idx);
        }
        let start_pos = self.pawns[player][pawn_idx];
        let path: Vec<u8> = if start_pos == 0 {
            vec![0, 1]
        } else {
            (start_pos..=(start_pos + self.dice_value)).collect()
        };

        self.phase = TurnPhase::AnimatingPawn;
        self.step_anim = Some(StepAnimation {
            player,
            pawn_idx,
            path,
            current_leg: 0,
            leg_start: Instant::now(),
            leg_duration: 0.16,
        });
    }

    fn finish_pawn_move(
        &mut self,
        player: usize,
        pawn_idx: usize,
        final_pos: u8,
        audio: &SoundManager,
    ) {
        self.pawns[player][pawn_idx] = final_pos;
        let mut captured_any = false;

        if let Some(target_glob) = Self::rel_to_global(player, final_pos) {
            if !SAFE_GLOB_INDICES.contains(&target_glob) {
                for other_p in 0..4 {
                    if other_p == player || self.player_types[other_p] == PlayerType::Disabled {
                        continue;
                    }
                    for other_pawn in 0..4 {
                        let other_pos = self.pawns[other_p][other_pawn];
                        if let Some(other_glob) = Self::rel_to_global(other_p, other_pos) {
                            if other_glob == target_glob {
                                self.pawns[other_p][other_pawn] = 0;
                                captured_any = true;
                            }
                        }
                    }
                }
            }
        }

        if captured_any {
            audio.play(Sfx::ChessCap);
        } else if final_pos == 57 {
            audio.play(Sfx::Check);
        } else {
            audio.play(Sfx::CardPlace);
        }

        if self.pawns[player].iter().all(|&p| p == 57) {
            if self.player_types[player] == PlayerType::Human
                && (!self.online_mode || player == self.my_player_idx)
            {
                self.newly_won = true;
                audio.play(Sfx::Win);
            } else {
                audio.play(Sfx::Lose);
            }
            self.phase = TurnPhase::GameOver(player);
            self.status_text = format!(
                "🏆 WYGRYWA {}! Wszystkie pionki w mecie!",
                Self::player_name(player)
            );
            return;
        }

        if self.dice_value == 6 || captured_any {
            self.phase = TurnPhase::WaitForRoll;
            let reason = if captured_any {
                "Zbicie przeciwnika!"
            } else {
                "Wyrzucono 6!"
            };
            self.status_text = format!(
                "{} — {} Dodatkowy rzut kostką!",
                Self::player_name(player),
                reason
            );
            if !self.online_mode && self.player_types[player] == PlayerType::Bot {
                self.bot_wait_until =
                    Some(Instant::now() + std::time::Duration::from_millis(450));
            }
        } else {
            self.advance_turn();
        }
    }

    fn advance_turn(&mut self) {
        // Szukamy kolejnego aktywnego gracza (pomijamy Disabled!)
        for _ in 0..4 {
            self.current_turn = (self.current_turn + 1) % 4;
            if self.player_types[self.current_turn] != PlayerType::Disabled {
                break;
            }
        }
        self.phase = TurnPhase::WaitForRoll;
        self.status_text = format!(
            "Tura gracza: {} — Rzuć kostką!",
            Self::player_name(self.current_turn)
        );

        if !self.online_mode && self.player_types[self.current_turn] == PlayerType::Bot {
            self.bot_wait_until = Some(Instant::now() + std::time::Duration::from_millis(450));
        } else {
            self.bot_wait_until = None;
        }
    }

    fn choose_bot_pawn(&self, player: usize, legal: &[usize]) -> usize {
        let mut best_pawn = legal[0];
        let mut best_score = i32::MIN;

        for &pawn_idx in legal {
            let cur = self.pawns[player][pawn_idx];
            let next = if cur == 0 { 1 } else { cur + self.dice_value };
            let mut score = 0;

            if cur == 0 {
                score += 450;
            }
            if next == 57 {
                score += 600;
            }
            if let Some(glob) = Self::rel_to_global(player, next) {
                if !SAFE_GLOB_INDICES.contains(&glob) {
                    for other_p in 0..4 {
                        if other_p == player || self.player_types[other_p] == PlayerType::Disabled {
                            continue;
                        }
                        for &other_pos in &self.pawns[other_p] {
                            if Self::rel_to_global(other_p, other_pos) == Some(glob) {
                                score += 800;
                            }
                        }
                    }
                } else {
                    score += 120;
                }
            }

            score += next as i32 * 3;

            if score > best_score {
                best_score = score;
                best_pawn = pawn_idx;
            }
        }

        best_pawn
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        let now = Instant::now();

        // --- 1. AKTUALIZACJA ANIMACJI KOSTKI ---
        if let TurnPhase::RollingDice {
            start,
            mut last_tick,
            mut preview_face,
            final_face,
        } = self.phase
        {
            let elapsed = now.duration_since(start).as_secs_f32();
            let roll_duration = 1.15;

            if elapsed < roll_duration {
                let progress = elapsed / roll_duration;
                let dynamic_interval_ms = (55.0 + progress.powi(2) * 140.0) as u128;

                if now.duration_since(last_tick).as_millis() >= dynamic_interval_ms {
                    let mut rng = rand::thread_rng();
                    preview_face = rng.gen_range(1..=6);
                    last_tick = now;
                    audio.play(Sfx::Click);
                }
                self.phase = TurnPhase::RollingDice {
                    start,
                    last_tick,
                    preview_face,
                    final_face,
                };
                ui.ctx().request_repaint();
            } else {
                self.dice_value = final_face;
                audio.play(Sfx::ChessMove);
                let legal = self.legal_pawns(self.current_turn, self.dice_value);

                if legal.is_empty() {
                    self.status_text = format!(
                        "{} wyrzucił {}, brak dostępnych ruchów!",
                        Self::player_name(self.current_turn),
                        self.dice_value
                    );
                    self.advance_turn();
                } else if legal.len() == 1
                    && self.player_types[self.current_turn] == PlayerType::Human
                    && (!self.online_mode || self.current_turn == self.my_player_idx)
                {
                    let only_pawn = legal[0];
                    self.begin_pawn_move(self.current_turn, only_pawn);
                } else {
                    self.phase = TurnPhase::WaitForMove;
                    self.status_text = format!(
                        "{} wyrzucił {} — Wybierz pionka do ruchu!",
                        Self::player_name(self.current_turn),
                        self.dice_value
                    );
                    if !self.online_mode && self.player_types[self.current_turn] == PlayerType::Bot {
                        self.bot_wait_until =
                            Some(now + std::time::Duration::from_millis(450));
                    }
                }
                ui.ctx().request_repaint();
            }
        }

        // --- 2. AKTUALIZACJA ANIMACJI SKAKANIA PIONKA ---
        let mut finished_move: Option<(usize, usize, u8)> = None;
        if let Some(anim) = &mut self.step_anim {
            let elapsed = now.duration_since(anim.leg_start).as_secs_f32();
            if elapsed >= anim.leg_duration {
                anim.current_leg += 1;
                anim.leg_start = now;
                if anim.current_leg + 1 >= anim.path.len() {
                    let final_state = *anim.path.last().unwrap();
                    finished_move = Some((anim.player, anim.pawn_idx, final_state));
                } else {
                    audio.play(Sfx::ChessMove);
                }
            }
            ui.ctx().request_repaint();
        }
        if let Some((p, idx, final_state)) = finished_move {
            self.step_anim = None;
            self.finish_pawn_move(p, idx, final_state, audio);
        }

        // --- 3. AUTOMATYKA BOTA AI (TYLKO W TRYBIE OFFLINE!) ---
        if !self.online_mode
            && self.player_types[self.current_turn] == PlayerType::Bot
            && matches!(self.phase, TurnPhase::WaitForRoll | TurnPhase::WaitForMove)
        {
            ui.ctx().request_repaint();

            if let Some(until) = self.bot_wait_until {
                if now >= until {
                    self.bot_wait_until = None;
                    match self.phase {
                        TurnPhase::WaitForRoll => self.start_dice_roll(audio),
                        TurnPhase::WaitForMove => {
                            let legal = self.legal_pawns(self.current_turn, self.dice_value);
                            if !legal.is_empty() {
                                let chosen = self.choose_bot_pawn(self.current_turn, &legal);
                                self.begin_pawn_move(self.current_turn, chosen);
                            } else {
                                self.advance_turn();
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                self.bot_wait_until = Some(now + std::time::Duration::from_millis(450));
            }
        }

        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(25, 28, 35));

        let margin = (full_rect.width() * 0.02).clamp(10.0, 26.0);
        let content_rect = full_rect.shrink(margin);

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child_ui;

        ui.horizontal(|ui| {
            if !self.online_mode {
                if ui
                    .button(egui::RichText::new("🔄 Nowa gra").size(16.0))
                    .clicked()
                {
                    audio.play(Sfx::Click);
                    self.new_game();
                }
                ui.separator();
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "🌐 Twój kolor: {}",
                        Self::player_name(self.my_player_idx)
                    ))
                    .size(16.0)
                    .strong()
                    .color(Self::player_color(self.my_player_idx)),
                );
                ui.separator();
            }
            ui.label(
                egui::RichText::new(&self.status_text)
                    .size(19.0)
                    .strong()
                    .color(egui::Color32::WHITE),
            );
        });

        ui.add_space(10.0);

        let work_rect = ui.available_rect_before_wrap();
        let panel_w = (work_rect.width() * 0.27).clamp(210.0, 310.0);
        let max_board_w = (work_rect.width() - panel_w - 24.0).max(280.0);
        let board_px = max_board_w.min(work_rect.height() - 6.0).max(280.0);
        let cell = board_px / 15.0;

        let total_w = board_px + 20.0 + panel_w;
        let start_x = work_rect.left() + ((work_rect.width() - total_w) / 2.0).max(0.0);
        let start_y = work_rect.top() + ((work_rect.height() - board_px) / 2.0).max(0.0);

        let board_rect =
            egui::Rect::from_min_size(egui::pos2(start_x, start_y), egui::vec2(board_px, board_px));

        self.draw_ludo_board(ui.painter(), board_rect, cell);

        let legal_for_current = if self.phase == TurnPhase::WaitForMove
            && self.player_types[self.current_turn] == PlayerType::Human
            && (!self.online_mode || self.current_turn == self.my_player_idx)
        {
            self.legal_pawns(self.current_turn, self.dice_value)
        } else {
            Vec::new()
        };

        let mut clicked_pawn: Option<usize> = None;

        for p in 0..4 {
            if self.player_types[p] == PlayerType::Disabled {
                continue;
            }
            for pawn_idx in 0..4 {
                if let Some(anim) = &self.step_anim {
                    if anim.player == p && anim.pawn_idx == pawn_idx {
                        continue;
                    }
                }

                let state = self.pawns[p][pawn_idx];
                let (gx, gy) = Self::state_to_grid(p, pawn_idx, state);

                let mut stack_offset = egui::Vec2::ZERO;
                if (1..=56).contains(&state) {
                    let same_tile_idx = (0..pawn_idx)
                        .filter(|&i| self.pawns[p][i] == state)
                        .count();
                    if same_tile_idx > 0 {
                        let angle = same_tile_idx as f32 * 2.1;
                        stack_offset = egui::vec2(angle.cos(), angle.sin()) * (cell * 0.16);
                    }
                }

                let center = egui::pos2(
                    board_rect.left() + gx * cell,
                    board_rect.top() + gy * cell,
                ) + stack_offset;

                let is_selectable = p == self.current_turn && legal_for_current.contains(&pawn_idx);

                Self::draw_pawn(
                    ui.painter(),
                    center,
                    cell * 0.36,
                    Self::player_color(p),
                    is_selectable,
                );

                if is_selectable {
                    let hit_rect =
                        egui::Rect::from_center_size(center, egui::vec2(cell * 0.9, cell * 0.9));
                    let resp = ui.interact(
                        hit_rect,
                        egui::Id::new(("ludo_pawn", p, pawn_idx)),
                        egui::Sense::click(),
                    );
                    if resp.clicked() {
                        clicked_pawn = Some(pawn_idx);
                    }
                }
            }
        }

        if let Some(pawn_idx) = clicked_pawn {
            audio.play(Sfx::Click);
            self.begin_pawn_move(self.current_turn, pawn_idx);
        }

        if let Some(anim) = &self.step_anim {
            let from_state = anim.path[anim.current_leg];
            let to_state = anim.path[(anim.current_leg + 1).min(anim.path.len() - 1)];

            let (fx, fy) = Self::state_to_grid(anim.player, anim.pawn_idx, from_state);
            let (tx, ty) = Self::state_to_grid(anim.player, anim.pawn_idx, to_state);

            let from_pt = egui::pos2(board_rect.left() + fx * cell, board_rect.top() + fy * cell);
            let to_pt = egui::pos2(board_rect.left() + tx * cell, board_rect.top() + ty * cell);

            let t = (now.duration_since(anim.leg_start).as_secs_f32() / anim.leg_duration)
                .clamp(0.0, 1.0);
            let hop_arc = (t * std::f32::consts::PI).sin() * (cell * 0.45);
            let cur_pt = from_pt.lerp(to_pt, t) - egui::vec2(0.0, hop_arc);

            Self::draw_pawn(
                ui.painter(),
                cur_pt,
                cell * 0.42,
                Self::player_color(anim.player),
                true,
            );
        }

        let panel_rect = egui::Rect::from_min_size(
            egui::pos2(board_rect.right() + 20.0, board_rect.top()),
            egui::vec2(panel_w, board_px),
        );

        let mut panel_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(panel_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(36, 40, 48))
            .rounding(12.0)
            .inner_margin(16.0)
            .show(&mut panel_ui, |ui| {
                ui.set_min_size(ui.available_size());

                ui.heading(
                    egui::RichText::new("🎲 Kostka do gry")
                        .size(20.0)
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(12.0);

                let dice_px = (panel_w * 0.44).clamp(80.0, 120.0);
                let (dice_rect, dice_resp) =
                    ui.allocate_exact_size(egui::vec2(dice_px, dice_px), egui::Sense::click());

                let shown_face = match self.phase {
                    TurnPhase::RollingDice { preview_face, .. } => preview_face,
                    _ => self.dice_value,
                };
                let is_rolling = matches!(self.phase, TurnPhase::RollingDice { .. });

                Self::draw_dice_widget(
                    ui.painter(),
                    dice_rect,
                    shown_face,
                    Self::player_color(self.current_turn),
                    is_rolling,
                );

                ui.add_space(12.0);

                let can_human_roll = self.phase == TurnPhase::WaitForRoll
                    && self.player_types[self.current_turn] == PlayerType::Human
                    && (!self.online_mode || self.current_turn == self.my_player_idx);

                if ui
                    .add_enabled(
                        can_human_roll,
                        egui::Button::new(
                            egui::RichText::new("🎲 Rzuć kostką!").size(18.0).strong(),
                        )
                        .min_size(egui::vec2(panel_w - 32.0, 42.0)),
                    )
                    .clicked()
                    || (can_human_roll && dice_resp.clicked())
                {
                    self.start_dice_roll(audio);
                }

                ui.add_space(18.0);
                ui.separator();
                ui.add_space(8.0);

                ui.heading(
                    egui::RichText::new("👥 Gracze przy stole")
                        .size(18.0)
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(8.0);

                for p in 0..4 {
                    let is_active = self.current_turn == p;
                    let frame_bg = if is_active {
                        egui::Color32::from_rgb(55, 62, 76)
                    } else {
                        egui::Color32::from_rgb(28, 31, 38)
                    };

                    egui::Frame::none()
                        .fill(frame_bg)
                        .rounding(8.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let finished_count =
                                    self.pawns[p].iter().filter(|&&s| s == 57).count();
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} ({}/4)",
                                        Self::player_name(p),
                                        finished_count
                                    ))
                                    .strong()
                                    .color(Self::player_color(p)),
                                );

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if self.online_mode {
                                            let label = match self.player_types[p] {
                                                PlayerType::Human => {
                                                    if p == self.my_player_idx {
                                                        "🧑 Ty (Online)"
                                                    } else {
                                                        "🌐 Gracz Online"
                                                    }
                                                }
                                                _ => "⛔ Wolne miejsce",
                                            };
                                            ui.label(
                                                egui::RichText::new(label)
                                                    .size(13.0)
                                                    .color(egui::Color32::LIGHT_GRAY),
                                            );
                                        } else {
                                            let mode_txt = match self.player_types[p] {
                                                PlayerType::Human => "🧑 Gracz",
                                                PlayerType::Bot => "🤖 Bot",
                                                PlayerType::Disabled => "⛔ Wył.",
                                            };
                                            if ui.button(mode_txt).clicked() {
                                                audio.play(Sfx::Click);
                                                self.player_types[p] = match self.player_types[p] {
                                                    PlayerType::Human => PlayerType::Bot,
                                                    PlayerType::Bot => PlayerType::Disabled,
                                                    PlayerType::Disabled => PlayerType::Human,
                                                };
                                                if self.current_turn == p
                                                    && self.player_types[p] == PlayerType::Bot
                                                {
                                                    self.bot_wait_until = Some(
                                                        Instant::now()
                                                            + std::time::Duration::from_millis(250),
                                                    );
                                                } else if self.current_turn == p
                                                    && self.player_types[p] == PlayerType::Disabled
                                                {
                                                    self.advance_turn();
                                                }
                                            }
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(6.0);
                }
            });
    }

    fn draw_ludo_board(&self, painter: &egui::Painter, board_rect: egui::Rect, cell: f32) {
        painter.rect_filled(
            board_rect.expand(6.0),
            10.0,
            egui::Color32::from_rgb(55, 42, 32),
        );
        painter.rect_filled(board_rect, 4.0, egui::Color32::from_rgb(242, 238, 228));

        let stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(180, 175, 165));

        let bases = [
            (0, 0, Self::player_color(0)),
            (9, 0, Self::player_color(1)),
            (9, 9, Self::player_color(2)),
            (0, 9, Self::player_color(3)),
        ];

        for &(bx, by, col) in &bases {
            let r = egui::Rect::from_min_size(
                egui::pos2(
                    board_rect.left() + bx as f32 * cell,
                    board_rect.top() + by as f32 * cell,
                ),
                egui::vec2(6.0 * cell, 6.0 * cell),
            );
            painter.rect(r, 0.0, col, stroke);

            let inner = r.shrink(cell * 0.85);
            painter.rect(
                inner,
                8.0,
                egui::Color32::from_rgb(248, 246, 240),
                egui::Stroke::new(2.0_f32, egui::Color32::from_black_alpha(60)),
            );
        }

        for p in 0..4 {
            for idx in 0..4 {
                let (gx, gy) = Self::state_to_grid(p, idx, 0);
                let c = egui::pos2(
                    board_rect.left() + gx * cell,
                    board_rect.top() + gy * cell,
                );
                painter.circle(
                    c,
                    cell * 0.38,
                    Self::player_color(p).gamma_multiply(0.35),
                    egui::Stroke::new(1.5_f32, Self::player_color(p)),
                );
            }
        }

        for (glob_idx, &(cx, cy)) in MAIN_TRACK.iter().enumerate() {
            let r = egui::Rect::from_min_size(
                egui::pos2(
                    board_rect.left() + cx as f32 * cell,
                    board_rect.top() + cy as f32 * cell,
                ),
                egui::vec2(cell, cell),
            );

            let tile_fill = match glob_idx {
                0 => Self::player_color(0),
                13 => Self::player_color(1),
                26 => Self::player_color(2),
                39 => Self::player_color(3),
                _ => egui::Color32::from_rgb(248, 246, 240),
            };

            painter.rect(r, 0.0, tile_fill, stroke);

            if SAFE_GLOB_INDICES.contains(&glob_idx) {
                let star_col = if matches!(glob_idx, 0 | 13 | 26 | 39) {
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_rgb(160, 150, 135)
                };
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    "★",
                    egui::FontId::proportional((cell * 0.55).max(10.0)),
                    star_col,
                );
            }
        }

        for p in 0..4 {
            let col = Self::player_color(p);
            for state in 52..=56 {
                let (gx, gy) = Self::state_to_grid(p, 0, state);
                let r = egui::Rect::from_center_size(
                    egui::pos2(
                        board_rect.left() + gx * cell,
                        board_rect.top() + gy * cell,
                    ),
                    egui::vec2(cell, cell),
                );
                painter.rect(r, 0.0, col.gamma_multiply(0.85), stroke);
            }
        }

        let center_rect = egui::Rect::from_min_size(
            egui::pos2(board_rect.left() + 6.0 * cell, board_rect.top() + 6.0 * cell),
            egui::vec2(3.0 * cell, 3.0 * cell),
        );
        let mid = center_rect.center();
        let lt = center_rect.left_top();
        let rt = center_rect.right_top();
        let rb = center_rect.right_bottom();
        let lb = center_rect.left_bottom();

        let triangles = [
            (vec![lt, lb, mid], Self::player_color(0)),
            (vec![lt, rt, mid], Self::player_color(1)),
            (vec![rt, rb, mid], Self::player_color(2)),
            (vec![lb, rb, mid], Self::player_color(3)),
        ];

        for (pts, col) in triangles {
            painter.add(egui::Shape::convex_polygon(pts, col, stroke));
        }
    }

    fn draw_pawn(
        painter: &egui::Painter,
        center: egui::Pos2,
        radius: f32,
        color: egui::Color32,
        highlighted: bool,
    ) {
        if highlighted {
            painter.circle_stroke(
                center,
                radius * 1.32,
                egui::Stroke::new(3.0_f32, egui::Color32::GOLD),
            );
        }

        painter.circle_filled(
            center + egui::vec2(2.0, 3.0),
            radius,
            egui::Color32::from_black_alpha(120),
        );

        painter.circle(
            center,
            radius,
            color,
            egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
        );

        painter.circle_filled(
            center - egui::vec2(radius * 0.28, radius * 0.28),
            radius * 0.32,
            egui::Color32::from_white_alpha(110),
        );
    }

    fn draw_dice_widget(
        painter: &egui::Painter,
        rect: egui::Rect,
        face: u8,
        border_color: egui::Color32,
        is_rolling: bool,
    ) {
        let draw_rect = if is_rolling {
            rect.shrink(3.0)
        } else {
            rect
        };

        painter.rect_filled(
            draw_rect.translate(egui::vec2(3.0, 4.0)),
            14.0,
            egui::Color32::from_black_alpha(110),
        );

        painter.rect(
            draw_rect,
            14.0,
            egui::Color32::from_rgb(250, 250, 245),
            egui::Stroke::new(3.5_f32, border_color),
        );

        let r = draw_rect.width() * 0.085;
        let c = draw_rect.center();
        let off = draw_rect.width() * 0.25;

        let tl = c + egui::vec2(-off, -off);
        let tr = c + egui::vec2(off, -off);
        let ml = c + egui::vec2(-off, 0.0);
        let mr = c + egui::vec2(off, 0.0);
        let bl = c + egui::vec2(-off, off);
        let br = c + egui::vec2(off, off);

        let pips: &[egui::Pos2] = match face {
            1 => &[c],
            2 => &[tl, br],
            3 => &[tl, c, br],
            4 => &[tl, tr, bl, br],
            5 => &[tl, tr, c, bl, br],
            _ => &[tl, tr, ml, mr, bl, br],
        };

        let pip_col = if face == 6 {
            egui::Color32::from_rgb(215, 45, 45)
        } else {
            egui::Color32::from_rgb(30, 30, 35)
        };

        for &pt in pips {
            painter.circle_filled(pt, r, pip_col);
        }
    }
}