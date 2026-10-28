use crate::audio::{Sfx, SoundManager};
use eframe::egui;
use rand::seq::SliceRandom;
use serde_json::{json, Value};
use shakmaty::fen::Fen;
use shakmaty::{
    CastlingMode, Chess, Color, EnPassantMode, Move, Outcome, Position, Role, Square,
};
use std::time::Instant;

struct MoveAnimation {
    from: Square,
    to: Square,
    color: Color,
    role: Role,
    start_time: Instant,
    duration: f32,
}

pub struct ChessGame {
    pos: Chess,
    selected_sq: Option<Square>,
    dragging_sq: Option<Square>,
    vs_bot: bool,
    bot_color: Color,
    flipped: bool,
    last_move: Option<(Square, Square)>,
    move_history: Vec<String>,
    status_text: String,
    active_anim: Option<MoveAnimation>,
    bot_think_until: Option<Instant>,
    pub newly_won: bool,
    pub online_mode: bool,
    pub my_online_color: Color,
    pub outgoing_move: Option<(u32, u32)>,
    /// Gracz z miejsca 3/4 tylko obserwuje partię.
    pub spectator: bool,
    /// Ilu obserwatorów aktualnie patrzy na partię (do ikony 👁).
    pub spectators: usize,
}

impl Default for ChessGame {
    fn default() -> Self {
        let mut game = Self {
            pos: Chess::default(),
            selected_sq: None,
            dragging_sq: None,
            vs_bot: true,
            bot_color: Color::Black,
            flipped: false,
            last_move: None,
            move_history: Vec::new(),
            status_text: String::new(),
            active_anim: None,
            bot_think_until: None,
            newly_won: false,
            online_mode: false,
            my_online_color: Color::White,
            outgoing_move: None,
            spectator: false,
            spectators: 0,
        };
        game.update_status();
        game
    }
}

impl ChessGame {
    pub fn new_game(&mut self) {
        self.pos = Chess::default();
        self.selected_sq = None;
        self.dragging_sq = None;
        self.last_move = None;
        self.move_history.clear();
        self.active_anim = None;
        self.bot_think_until = None;
        self.outgoing_move = None;
        self.newly_won = false;
        self.update_status();
    }

    pub fn setup_online(&mut self, player_idx: usize) {
        self.online_mode = true;
        self.vs_bot = false;
        self.spectator = player_idx >= 2;
        // Miejsca 3/4 nie kontrolują koloru — tylko obserwują.
        self.my_online_color = if !self.spectator && player_idx == 1 {
            Color::Black
        } else {
            Color::White
        };
        self.flipped = self.my_online_color == Color::Black;
        self.new_game();
        if self.spectator {
            self.status_text = "👁 Obserwujesz partię — czekam na synchronizację...".to_string();
        }
    }

    /// Stan partii wystarczający do odtworzenia planszy u obserwatora
    /// lub u gracza, który dopiero wszedł na ekran szachów.
    pub fn state_json(&self) -> Value {
        let fen = Fen::from_position(self.pos.clone(), EnPassantMode::Legal).to_string();
        json!({
            "fen": fen,
            "last_move": self.last_move.map(|(from, to)| [from as u32, to as u32]),
            "history": &self.move_history,
        })
    }

    pub fn apply_state_json(&mut self, state: &Value) -> bool {
        let Some(fen) = state.get("fen").and_then(Value::as_str) else {
            return false;
        };
        let Ok(fen) = Fen::from_ascii(fen.as_bytes()) else {
            return false;
        };
        let Ok(pos) = fen.into_position::<Chess>(CastlingMode::Standard) else {
            return false;
        };

        let new_history: Vec<String> = state
            .get("history")
            .and_then(Value::as_array)
            .map(|history| {
                history
                    .iter()
                    .filter_map(|entry| entry.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        // Nie cofamy lokalnej partii starszym snapshotem (np. gdy odpowiedzi
        // od obu graczy dotrą w różnej kolejności).
        if new_history.len() < self.move_history.len() {
            return false;
        }

        self.pos = pos;
        self.last_move = state
            .get("last_move")
            .and_then(Value::as_array)
            .and_then(|arr| {
                let from = arr.first()?.as_u64()? as u32;
                let to = arr.get(1)?.as_u64()? as u32;
                if from < 64 && to < 64 {
                    Some((Square::new(from), Square::new(to)))
                } else {
                    None
                }
            });
        self.move_history = new_history;
        self.selected_sq = None;
        self.dragging_sq = None;
        self.active_anim = None;
        self.bot_think_until = None;
        self.online_mode = true;
        self.vs_bot = false;
        self.update_status();
        if self.spectator {
            self.status_text = format!("👁 Obserwujesz | {}", self.status_text);
        }
        true
    }

    pub fn apply_remote_move(&mut self, from_u32: u32, to_u32: u32, audio: &SoundManager) {
        if from_u32 < 64 && to_u32 < 64 {
            let from = Square::new(from_u32);
            let to = Square::new(to_u32);
            let _ = self.try_make_move(from, to, true, audio);
            self.outgoing_move = None;
        }
    }

    fn update_status(&mut self) {
        if let Some(outcome) = self.pos.outcome() {
            self.status_text = match outcome {
                Outcome::Decisive { winner: Color::White } => {
                    "🏆 SZACH-MAT! Wygrywają Białe!".to_string()
                }
                Outcome::Decisive { winner: Color::Black } => {
                    "🏆 SZACH-MAT! Wygrywają Czarne!".to_string()
                }
                Outcome::Draw => "🤝 REMIS (Pat / brak materiału)!".to_string(),
            };
        } else {
            let turn_str = match self.pos.turn() {
                Color::White => "⚪ Ruch Białych",
                Color::Black => "⚫ Ruch Czarnych",
            };
            if self.pos.is_check() {
                self.status_text = format!("{} — ⚠ SZACH!", turn_str);
            } else {
                self.status_text = turn_str.to_string();
            }
        }
    }

    fn legal_moves_from(&self, sq: Square) -> Vec<Move> {
        self.pos
            .legal_moves()
            .into_iter()
            .filter(|m| m.from() == Some(sq))
            .collect()
    }

    fn try_make_move(
        &mut self,
        from: Square,
        to: Square,
        animate: bool,
        audio: &SoundManager,
    ) -> bool {
        let legal = self.pos.legal_moves();
        let chosen = legal
            .iter()
            .find(|m| m.from() == Some(from) && m.to() == to && m.promotion() == Some(Role::Queen))
            .or_else(|| legal.iter().find(|m| m.from() == Some(from) && m.to() == to));

        if let Some(m) = chosen.cloned() {
            self.apply_move(m, animate, audio);
            return true;
        }
        false
    }

    fn apply_move(&mut self, m: Move, animate: bool, audio: &SoundManager) {
        let from = m.from().unwrap_or(m.to());
        let to = m.to();
        let moving_color = self.pos.turn();
        let final_role = m.promotion().unwrap_or(m.role());
        let is_capture = m.is_capture();

        let role_sym = Self::piece_symbol(moving_color, m.role());
        let capture_sym = if is_capture { "x" } else { "→" };
        let notation = format!(
            "{}. {} {}{}{}",
            self.move_history.len() / 2 + 1,
            role_sym,
            from,
            capture_sym,
            to
        );

        if let Ok(new_pos) = self.pos.clone().play(&m) {
            self.pos = new_pos;
            self.last_move = Some((from, to));
            // Tylko wysyłamy ruch do serwera, gdy to nasz ruch (nie ruch przeciwnika z sieci).
            // Obserwator nigdy nie wysyła ruchów.
            if !self.online_mode
                || (!self.spectator && moving_color == self.my_online_color)
            {
                self.outgoing_move = Some((from as u32, to as u32));
            }
            self.move_history.push(notation);
            self.selected_sq = None;
            self.dragging_sq = None;

            if let Some(outcome) = self.pos.outcome() {
                let human_won = match outcome {
                    Outcome::Decisive { winner } => {
                        self.online_mode || !self.vs_bot || winner != self.bot_color
                    }
                    Outcome::Draw => false,
                };
                if human_won
                    && !self.spectator
                    && (!self.online_mode || moving_color == self.my_online_color)
                {
                    self.newly_won = true;
                    audio.play(Sfx::Win);
                } else {
                    audio.play(Sfx::Lose);
                }
            } else if self.pos.is_check() {
                audio.play(Sfx::Check);
            } else if is_capture {
                audio.play(Sfx::ChessCap);
            } else {
                audio.play(Sfx::ChessMove);
            }

            if animate {
                self.active_anim = Some(MoveAnimation {
                    from,
                    to,
                    color: moving_color,
                    role: final_role,
                    start_time: Instant::now(),
                    duration: 0.28,
                });
            }

            if !self.online_mode
                && self.vs_bot
                && self.pos.outcome().is_none()
                && self.pos.turn() == self.bot_color
            {
                let delay_ms = if animate { 620 } else { 360 };
                self.bot_think_until =
                    Some(Instant::now() + std::time::Duration::from_millis(delay_ms));
            } else {
                self.bot_think_until = None;
            }

            self.update_status();
        }
    }

    fn make_bot_move(&mut self, audio: &SoundManager) {
        let mut legal: Vec<Move> = self.pos.legal_moves().into_iter().collect();
        if legal.is_empty() {
            return;
        }

        let mut rng = rand::thread_rng();
        legal.shuffle(&mut rng);

        let bot_color = self.pos.turn();
        let mut best_move = legal[0].clone();
        let mut best_score = i32::MIN;

        for m in legal {
            if let Ok(next_pos) = self.pos.clone().play(&m) {
                if next_pos.is_checkmate() {
                    best_move = m;
                    break;
                }

                let opp_moves = next_pos.legal_moves();
                let mut worst_reply_score = i32::MAX;

                if opp_moves.is_empty() {
                    worst_reply_score = Self::evaluate_board(&next_pos, bot_color);
                } else {
                    for opp_m in opp_moves {
                        if let Ok(after_opp) = next_pos.clone().play(&opp_m) {
                            let s = Self::evaluate_board(&after_opp, bot_color);
                            if s < worst_reply_score {
                                worst_reply_score = s;
                            }
                        }
                    }
                }

                if worst_reply_score > best_score {
                    best_score = worst_reply_score;
                    best_move = m;
                }
            }
        }

        self.apply_move(best_move, true, audio);
    }

    fn evaluate_board(pos: &Chess, perspective: Color) -> i32 {
        if let Some(outcome) = pos.outcome() {
            return match outcome {
                Outcome::Decisive { winner } => {
                    if winner == perspective {
                        100_000
                    } else {
                        -100_000
                    }
                }
                Outcome::Draw => 0,
            };
        }

        let board = pos.board();
        let mut score = 0;

        for sq in Square::ALL {
            if let Some(piece) = board.piece_at(sq) {
                let mut val = match piece.role {
                    Role::Pawn => 100,
                    Role::Knight => 320,
                    Role::Bishop => 330,
                    Role::Rook => 500,
                    Role::Queen => 900,
                    Role::King => 20_000,
                };

                let file = sq.file() as i32;
                let rank = sq.rank() as i32;
                let center_dist = (file - 3).abs().min((file - 4).abs())
                    + (rank - 3).abs().min((rank - 4).abs());
                if piece.role != Role::King {
                    val += (4 - center_dist) * 8;
                }

                if piece.color == perspective {
                    score += val;
                } else {
                    score -= val;
                }
            }
        }

        score
    }

    fn piece_symbol(_color: Color, role: Role) -> &'static str {
        match role {
            Role::King => "♚",
            Role::Queen => "♛",
            Role::Rook => "♜",
            Role::Bishop => "♝",
            Role::Knight => "♞",
            Role::Pawn => "♟",
        }
    }

    fn draw_piece(
        painter: &egui::Painter,
        center: egui::Pos2,
        sq_size: f32,
        color: Color,
        role: Role,
    ) {
        let font_size = (sq_size * 0.74).max(18.0);
        let sym = Self::piece_symbol(color, role);

        let (fill_color, shadow_color) = match color {
            Color::White => (
                egui::Color32::from_rgb(252, 250, 242),
                egui::Color32::from_rgb(25, 25, 30),
            ),
            Color::Black => (
                egui::Color32::from_rgb(32, 34, 40),
                egui::Color32::from_rgb(235, 235, 235),
            ),
        };

        let offset = (sq_size * 0.03).clamp(1.5, 3.0);
        for (dx, dy) in [
            (-offset, 0.0),
            (offset, 0.0),
            (0.0, -offset),
            (0.0, offset),
            (offset, offset),
        ] {
            painter.text(
                center + egui::vec2(dx, dy),
                egui::Align2::CENTER_CENTER,
                sym,
                egui::FontId::proportional(font_size),
                shadow_color,
            );
        }

        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            sym,
            egui::FontId::proportional(font_size),
            fill_color,
        );
    }

    fn visual_to_square(&self, col: usize, row: usize) -> Square {
        let file = if self.flipped { 7 - col } else { col };
        let rank = if self.flipped { row } else { 7 - row };
        Square::new((rank * 8 + file) as u32)
    }

    fn square_center(&self, sq: Square, board_rect: egui::Rect, sq_size: f32) -> egui::Pos2 {
        let file = sq.file() as usize;
        let rank = sq.rank() as usize;
        let col = if self.flipped { 7 - file } else { file };
        let row = if self.flipped { rank } else { 7 - rank };
        egui::pos2(
            board_rect.left() + (col as f32 + 0.5) * sq_size,
            board_rect.top() + (row as f32 + 0.5) * sq_size,
        )
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        let now = Instant::now();

        let mut anim_in_progress = false;
        if let Some(anim) = &self.active_anim {
            if now.duration_since(anim.start_time).as_secs_f32() < anim.duration {
                anim_in_progress = true;
                ui.ctx().request_repaint();
            } else {
                self.active_anim = None;
            }
        }

        if !self.online_mode
            && !anim_in_progress
            && self.vs_bot
            && self.pos.outcome().is_none()
            && self.pos.turn() == self.bot_color
        {
            if let Some(until) = self.bot_think_until {
                if now >= until {
                    self.bot_think_until = None;
                    self.make_bot_move(audio);
                    ui.ctx().request_repaint();
                } else {
                    ui.ctx()
                        .request_repaint_after(until.saturating_duration_since(now));
                }
            } else {
                self.bot_think_until = Some(now + std::time::Duration::from_millis(350));
                ui.ctx().request_repaint();
            }
        }

        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(28, 30, 36));

        let margin = (full_rect.width() * 0.02).clamp(12.0, 28.0);
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
                    .button(egui::RichText::new("🔄 Nowa partia").size(16.0))
                    .clicked()
                {
                    audio.play(Sfx::Click);
                    self.new_game();
                }
                ui.separator();

                let mode_label = if self.vs_bot {
                    "🤖 Tryb: Gracz vs Bot AI"
                } else {
                    "👥 Tryb: 2 Graczy (przy 1 ekranie)"
                };
                if ui
                    .button(egui::RichText::new(mode_label).size(16.0))
                    .clicked()
                {
                    audio.play(Sfx::Click);
                    self.vs_bot = !self.vs_bot;
                    self.bot_think_until = None;
                }

                if self.vs_bot {
                    let side_label = match self.bot_color {
                        Color::Black => "Grasz: ⚪ Białymi",
                        Color::White => "Grasz: ⚫ Czarnymi",
                    };
                    if ui
                        .button(egui::RichText::new(side_label).size(16.0))
                        .clicked()
                    {
                        audio.play(Sfx::Click);
                        self.bot_color = !self.bot_color;
                        self.flipped = self.bot_color == Color::White;
                        self.new_game();
                    }
                }
            } else {
                let (online_label, online_color) = if self.spectator {
                    ("👁 Tryb obserwatora — nie możesz wykonywać ruchów", egui::Color32::GOLD)
                } else {
                    match self.my_online_color {
                        Color::White => (
                            "🌐 Tryb Online: Grasz Białymi",
                            egui::Color32::from_rgb(80, 235, 120),
                        ),
                        Color::Black => (
                            "🌐 Tryb Online: Grasz Czarnymi",
                            egui::Color32::from_rgb(80, 235, 120),
                        ),
                    }
                };
                ui.label(
                    egui::RichText::new(online_label)
                        .size(16.0)
                        .strong()
                        .color(online_color),
                );
                if self.spectators > 0 {
                    ui.separator();
                    let text = if self.spectators == 1 {
                        "👁 1 osoba obserwuje grę".to_string()
                    } else {
                        format!("👁 {} osoby obserwują grę", self.spectators)
                    };
                    ui.label(
                        egui::RichText::new(text)
                            .size(15.0)
                            .strong()
                            .color(egui::Color32::GOLD),
                    );
                }
            }

            if ui
                .button(egui::RichText::new("🔃 Obróć planszę").size(16.0))
                .clicked()
            {
                audio.play(Sfx::Click);
                self.flipped = !self.flipped;
            }

            ui.separator();

            let display_status = if self.bot_think_until.is_some() {
                "🤖 Bot analizuje ruch..."
            } else {
                &self.status_text
            };

            let status_color = if self.pos.outcome().is_some() {
                egui::Color32::GOLD
            } else if self.pos.is_check() {
                egui::Color32::from_rgb(255, 100, 100)
            } else if self.bot_think_until.is_some() {
                egui::Color32::from_rgb(150, 210, 255)
            } else {
                egui::Color32::WHITE
            };
            ui.label(
                egui::RichText::new(display_status)
                    .size(19.0)
                    .strong()
                    .color(status_color),
            );
        });

        ui.add_space(12.0);

        let work_rect = ui.available_rect_before_wrap();
        let panel_w = (work_rect.width() * 0.24).clamp(180.0, 280.0);
        let max_board_w = (work_rect.width() - panel_w - 24.0).max(260.0);
        let board_px = max_board_w.min(work_rect.height() - 8.0).max(260.0);
        let sq_size = board_px / 8.0;

        let total_w = board_px + 20.0 + panel_w;
        let start_x = work_rect.left() + ((work_rect.width() - total_w) / 2.0).max(0.0);
        let start_y = work_rect.top() + ((work_rect.height() - board_px) / 2.0).max(0.0);

        let board_rect =
            egui::Rect::from_min_size(egui::pos2(start_x, start_y), egui::vec2(board_px, board_px));

        ui.painter().rect_filled(
            board_rect.expand(6.0),
            8.0,
            egui::Color32::from_rgb(65, 48, 35),
        );

        let active_sq = self.dragging_sq.or(self.selected_sq);
        let legal_targets: Vec<Square> = active_sq
            .map(|sq| {
                self.legal_moves_from(sq)
                    .into_iter()
                    .map(|m| m.to())
                    .collect()
            })
            .unwrap_or_default();

        let anim_target_sq = self.active_anim.as_ref().map(|a| a.to);

        for row in 0..8 {
            for col in 0..8 {
                let sq = self.visual_to_square(col, row);
                let sq_rect = egui::Rect::from_min_size(
                    egui::pos2(
                        board_rect.left() + col as f32 * sq_size,
                        board_rect.top() + row as f32 * sq_size,
                    ),
                    egui::vec2(sq_size, sq_size),
                );

                let is_light = (col + row) % 2 == 0;
                let mut bg_color = if is_light {
                    egui::Color32::from_rgb(238, 216, 184)
                } else {
                    egui::Color32::from_rgb(176, 126, 84)
                };

                if let Some((from, to)) = self.last_move {
                    if sq == from || sq == to {
                        bg_color = if is_light {
                            egui::Color32::from_rgb(245, 236, 130)
                        } else {
                            egui::Color32::from_rgb(205, 186, 75)
                        };
                    }
                }

                if active_sq == Some(sq) {
                    bg_color = egui::Color32::from_rgb(130, 195, 110);
                }

                if self.pos.is_check() {
                    if let Some(piece) = self.pos.board().piece_at(sq) {
                        if piece.role == Role::King && piece.color == self.pos.turn() {
                            bg_color = egui::Color32::from_rgb(225, 75, 75);
                        }
                    }
                }

                ui.painter().rect_filled(sq_rect, 0.0, bg_color);

                let coord_color = if is_light {
                    egui::Color32::from_rgb(140, 95, 60)
                } else {
                    egui::Color32::from_rgb(235, 215, 185)
                };
                if col == 0 {
                    let rank_ch = sq.rank().char();
                    ui.painter().text(
                        sq_rect.left_top() + egui::vec2(4.0, 2.0),
                        egui::Align2::LEFT_TOP,
                        rank_ch,
                        egui::FontId::proportional((sq_size * 0.18).max(10.0)),
                        coord_color,
                    );
                }
                if row == 7 {
                    let file_ch = sq.file().char();
                    ui.painter().text(
                        sq_rect.right_bottom() - egui::vec2(4.0, 2.0),
                        egui::Align2::RIGHT_BOTTOM,
                        file_ch,
                        egui::FontId::proportional((sq_size * 0.18).max(10.0)),
                        coord_color,
                    );
                }

                if legal_targets.contains(&sq) {
                    if self.pos.board().piece_at(sq).is_some() {
                        ui.painter().circle_stroke(
                            sq_rect.center(),
                            sq_size * 0.42,
                            egui::Stroke::new(
                                (sq_size * 0.07).max(3.0),
                                egui::Color32::from_rgba_unmultiplied(210, 55, 55, 190),
                            ),
                        );
                    } else {
                        ui.painter().circle_filled(
                            sq_rect.center(),
                            sq_size * 0.16,
                            egui::Color32::from_rgba_unmultiplied(40, 145, 55, 175),
                        );
                    }
                }

                if self.dragging_sq != Some(sq) && anim_target_sq != Some(sq) {
                    if let Some(piece) = self.pos.board().piece_at(sq) {
                        Self::draw_piece(
                            ui.painter(),
                            sq_rect.center(),
                            sq_size,
                            piece.color,
                            piece.role,
                        );
                    }
                }

                let resp = ui.interact(
                    sq_rect,
                    egui::Id::new(("chess_sq", sq as u32)),
                    egui::Sense::click_and_drag(),
                );

                let can_control = if self.spectator {
                    false
                } else if self.online_mode {
                    self.pos.turn() == self.my_online_color && self.active_anim.is_none()
                } else {
                    (!self.vs_bot || self.pos.turn() != self.bot_color)
                        && self.active_anim.is_none()
                };

                if can_control && self.pos.outcome().is_none() {
                    if resp.drag_started() {
                        if let Some(piece) = self.pos.board().piece_at(sq) {
                            if piece.color == self.pos.turn() {
                                audio.play(Sfx::Click);
                                self.dragging_sq = Some(sq);
                                self.selected_sq = Some(sq);
                            }
                        }
                    } else if resp.clicked() {
                        if let Some(sel) = self.selected_sq {
                            if sel == sq {
                                self.selected_sq = None;
                            } else if !self.try_make_move(sel, sq, true, audio) {
                                if let Some(piece) = self.pos.board().piece_at(sq) {
                                    if piece.color == self.pos.turn() {
                                        audio.play(Sfx::Click);
                                        self.selected_sq = Some(sq);
                                    } else {
                                        self.selected_sq = None;
                                    }
                                } else {
                                    self.selected_sq = None;
                                }
                            }
                        } else if let Some(piece) = self.pos.board().piece_at(sq) {
                            if piece.color == self.pos.turn() {
                                audio.play(Sfx::Click);
                                self.selected_sq = Some(sq);
                            }
                        }
                    }
                }
            }
        }

        if let Some(anim) = &self.active_anim {
            let elapsed = now.duration_since(anim.start_time).as_secs_f32();
            let raw_t = (elapsed / anim.duration).clamp(0.0, 1.0);
            let eased_t = 1.0 - (1.0 - raw_t).powi(3);

            let from_pos = self.square_center(anim.from, board_rect, sq_size);
            let to_pos = self.square_center(anim.to, board_rect, sq_size);
            let current_pos = from_pos.lerp(to_pos, eased_t);
            let lift_scale = 1.0 + 0.14 * (raw_t * std::f32::consts::PI).sin();

            Self::draw_piece(
                ui.painter(),
                current_pos,
                sq_size * lift_scale,
                anim.color,
                anim.role,
            );
        }

        if let Some(drag_sq) = self.dragging_sq {
            if let Some(ptr) = ui.ctx().pointer_latest_pos() {
                if let Some(piece) = self.pos.board().piece_at(drag_sq) {
                    let top_painter = ui.ctx().layer_painter(egui::LayerId::new(
                        egui::Order::Tooltip,
                        egui::Id::new("chess_drag_layer"),
                    ));
                    Self::draw_piece(
                        &top_painter,
                        ptr,
                        sq_size * 1.12,
                        piece.color,
                        piece.role,
                    );
                }
                ui.ctx().request_repaint();

                if ui.input(|i| i.pointer.any_released()) {
                    if board_rect.contains(ptr) {
                        let col = ((ptr.x - board_rect.left()) / sq_size).floor() as usize;
                        let row = ((ptr.y - board_rect.top()) / sq_size).floor() as usize;
                        if col < 8 && row < 8 && !self.spectator {
                            let target_sq = self.visual_to_square(col, row);
                            if target_sq != drag_sq {
                                self.try_make_move(drag_sq, target_sq, false, audio);
                            }
                        }
                    }
                    self.dragging_sq = None;
                }
            } else if ui.input(|i| i.pointer.any_released()) {
                self.dragging_sq = None;
            }
        }

        let panel_rect = egui::Rect::from_min_size(
            egui::pos2(board_rect.right() + 20.0, board_rect.top()),
            egui::vec2(panel_w, board_px),
        );

        let mut panel_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(panel_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(38, 42, 50))
            .rounding(10.0)
            .inner_margin(14.0)
            .show(&mut panel_ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.heading(
                    egui::RichText::new("📜 Historia ruchów")
                        .size(18.0)
                        .color(egui::Color32::WHITE),
                );
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.move_history.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "Brak wykonanych ruchów.\nZłap figurę lub kliknij pole!",
                                )
                                .color(egui::Color32::GRAY),
                            );
                        } else {
                            for pair in self.move_history.chunks(2) {
                                let line = if pair.len() == 2 {
                                    format!("{:<14} {}", pair[0], pair[1])
                                } else {
                                    pair[0].clone()
                                };
                                ui.label(
                                    egui::RichText::new(line)
                                        .monospace()
                                        .size(14.0)
                                        .color(egui::Color32::LIGHT_GRAY),
                                );
                            }
                        }
                    });
            });
    }
}