use crate::audio::{Sfx, SoundManager};
use crate::cards::{paint_card_at, Card, Deck, Rank, Suit};
use eframe::egui;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DragSource {
    Waste,
    Foundation(usize),
    Tableau { col: usize, idx: usize },
}

#[derive(Clone)]
struct DragState {
    source: DragSource,
    cards: Vec<Card>,
    grab_offset: egui::Vec2,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnimTarget {
    Waste,
    Foundation(usize),
    Tableau(usize),
}

struct CardFlightAnim {
    card: Card,
    from_pos: egui::Pos2,
    target: AnimTarget,
    start_time: Instant,
    duration: f32,
}

pub struct SolitaireGame {
    stock: Vec<Card>,
    waste: Vec<Card>,
    foundations: [Vec<Card>; 4],
    tableau: [Vec<Card>; 7],
    dragging: Option<DragState>,
    flight_anim: Option<CardFlightAnim>,
    moves: u32,
    won: bool,
    pub newly_won: bool,
}

impl Default for SolitaireGame {
    fn default() -> Self {
        let mut game = Self {
            stock: Vec::new(),
            waste: Vec::new(),
            foundations: Default::default(),
            tableau: Default::default(),
            dragging: None,
            flight_anim: None,
            moves: 0,
            won: false,
            newly_won: false,
        };
        game.new_game();
        game
    }
}

impl SolitaireGame {
    pub fn new_game(&mut self) {
        let mut deck = Deck::new_shuffled();
        self.stock.clear();
        self.waste.clear();
        for f in &mut self.foundations {
            f.clear();
        }
        for col in &mut self.tableau {
            col.clear();
        }

        for col_idx in 0..7 {
            for row_idx in 0..=col_idx {
                let face_up = row_idx == col_idx;
                if let Some(card) = deck.draw(face_up) {
                    self.tableau[col_idx].push(card);
                }
            }
        }

        while let Some(card) = deck.draw(false) {
            self.stock.push(card);
        }

        self.dragging = None;
        self.flight_anim = None;
        self.moves = 0;
        self.won = false;
        self.newly_won = false;
    }

    fn can_move_to_foundation(&self, moving: Card, f_idx: usize) -> bool {
        let expected_suits = [Suit::Hearts, Suit::Diamonds, Suit::Clubs, Suit::Spades];
        if moving.suit != expected_suits[f_idx] {
            return false;
        }

        let pile = &self.foundations[f_idx];
        match pile.last() {
            None => moving.rank == Rank::Ace,
            Some(top) => moving.rank.value() == top.rank.value() + 1,
        }
    }

    fn can_move_to_tableau(&self, moving: Card, col_idx: usize) -> bool {
        let pile = &self.tableau[col_idx];
        match pile.last() {
            None => moving.rank == Rank::King,
            Some(top) => {
                top.face_up
                    && top.suit.is_red() != moving.suit.is_red()
                    && top.rank.value() == moving.rank.value() + 1
            }
        }
    }

    fn flip_top_tableau(&mut self, col: usize) {
        if let Some(last) = self.tableau[col].last_mut() {
            if !last.face_up {
                last.face_up = true;
            }
        }
    }

    fn check_win(&mut self, audio: &SoundManager) {
        let was_won = self.won;
        self.won = self.foundations.iter().all(|f| f.len() == 13);
        if self.won && !was_won {
            self.newly_won = true;
            audio.play(Sfx::Win);
        }
    }

    fn execute_move_to_foundation(
        &mut self,
        source: DragSource,
        target_f: usize,
        audio: &SoundManager,
    ) -> Option<Card> {
        match source {
            DragSource::Waste => {
                if let Some(&card) = self.waste.last() {
                    if self.can_move_to_foundation(card, target_f) {
                        self.waste.pop();
                        self.foundations[target_f].push(card);
                        self.moves += 1;
                        audio.play(Sfx::CardPlace);
                        self.check_win(audio);
                        return Some(card);
                    }
                }
            }
            DragSource::Tableau { col, idx } => {
                if idx + 1 == self.tableau[col].len() {
                    let card = self.tableau[col][idx];
                    if self.can_move_to_foundation(card, target_f) {
                        self.tableau[col].pop();
                        self.flip_top_tableau(col);
                        self.foundations[target_f].push(card);
                        self.moves += 1;
                        audio.play(Sfx::CardPlace);
                        self.check_win(audio);
                        return Some(card);
                    }
                }
            }
            DragSource::Foundation(_) => {}
        }
        None
    }

    fn execute_move_to_tableau(
        &mut self,
        source: DragSource,
        target_col: usize,
        audio: &SoundManager,
    ) -> Option<Card> {
        match source {
            DragSource::Waste => {
                if let Some(&card) = self.waste.last() {
                    if self.can_move_to_tableau(card, target_col) {
                        self.waste.pop();
                        self.tableau[target_col].push(card);
                        self.moves += 1;
                        audio.play(Sfx::CardPlace);
                        return Some(card);
                    }
                }
            }
            DragSource::Foundation(f_idx) => {
                if let Some(&card) = self.foundations[f_idx].last() {
                    if self.can_move_to_tableau(card, target_col) {
                        self.foundations[f_idx].pop();
                        self.tableau[target_col].push(card);
                        self.moves += 1;
                        audio.play(Sfx::CardPlace);
                        return Some(card);
                    }
                }
            }
            DragSource::Tableau { col, idx } => {
                if col == target_col {
                    return None;
                }
                if idx < self.tableau[col].len() {
                    let first_card = self.tableau[col][idx];
                    if self.can_move_to_tableau(first_card, target_col) {
                        let moving_stack: Vec<Card> = self.tableau[col].drain(idx..).collect();
                        self.tableau[target_col].extend(moving_stack);
                        self.flip_top_tableau(col);
                        self.moves += 1;
                        audio.play(Sfx::CardPlace);
                        return Some(first_card);
                    }
                }
            }
        }
        None
    }

    fn auto_move(&mut self, source: DragSource, from_pos: egui::Pos2, audio: &SoundManager) {
        for f_idx in 0..4 {
            if let Some(card) = self.execute_move_to_foundation(source, f_idx, audio) {
                self.flight_anim = Some(CardFlightAnim {
                    card,
                    from_pos,
                    target: AnimTarget::Foundation(f_idx),
                    start_time: Instant::now(),
                    duration: 0.22,
                });
                return;
            }
        }
        for col_idx in 0..7 {
            if let Some(card) = self.execute_move_to_tableau(source, col_idx, audio) {
                self.flight_anim = Some(CardFlightAnim {
                    card,
                    from_pos,
                    target: AnimTarget::Tableau(col_idx),
                    start_time: Instant::now(),
                    duration: 0.22,
                });
                return;
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        let now = Instant::now();
        if let Some(anim) = &self.flight_anim {
            if now.duration_since(anim.start_time).as_secs_f32() < anim.duration {
                ui.ctx().request_repaint();
            } else {
                self.flight_anim = None;
            }
        }

        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(18, 62, 38));

        let margin = (full_rect.width() * 0.02).clamp(10.0, 28.0);
        let content_rect = full_rect.shrink(margin);

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child_ui;

        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("🔄 Nowe rozdanie").size(16.0))
                .clicked()
            {
                audio.play(Sfx::CardSlide);
                self.new_game();
            }
            ui.separator();
            ui.label(
                egui::RichText::new(format!("Ruchy: {}", self.moves))
                    .size(18.0)
                    .strong()
                    .color(egui::Color32::WHITE),
            );
            ui.separator();
            ui.label(
                egui::RichText::new(
                    "🖐 Przeciągnij i upuść karty (LPM)   |   ⚡ Szybki ruch z animacją (PPM / Dwuklik)",
                )
                .size(14.0)
                .color(egui::Color32::LIGHT_GRAY),
            );

            if self.won {
                ui.separator();
                ui.label(
                    egui::RichText::new("🏆 WYGRANA!")
                        .size(20.0)
                        .strong()
                        .color(egui::Color32::GOLD),
                );
            }
        });

        ui.add_space(10.0);

        let board_rect = ui.available_rect_before_wrap();
        let col_spacing = (board_rect.width() * 0.018).clamp(8.0, 28.0);

        let width_from_w = (board_rect.width() - col_spacing * 6.0) / 7.0;
        let width_from_h = (board_rect.height() / 3.4) / 1.42;
        let card_w = width_from_w.min(width_from_h).max(55.0);
        let card_h = card_w * 1.42;
        let card_size = egui::vec2(card_w, card_h);

        let total_board_w = card_w * 7.0 + col_spacing * 6.0;
        let start_x = board_rect.left() + ((board_rect.width() - total_board_w) / 2.0).max(0.0);
        let top_row_y = board_rect.top();

        let col_x = |col: usize| -> f32 { start_x + col as f32 * (card_w + col_spacing) };

        // --- 1. STOCK ---
        let stock_rect = egui::Rect::from_min_size(egui::pos2(col_x(0), top_row_y), card_size);
        let stock_resp = ui.interact(
            stock_rect,
            egui::Id::new("sol_stock"),
            egui::Sense::click(),
        );
        if let Some(top_stock) = self.stock.last() {
            paint_card_at(ui.painter(), stock_rect, top_stock, false);
            ui.painter().text(
                stock_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}", self.stock.len()),
                egui::FontId::proportional((card_w * 0.24).max(14.0)),
                egui::Color32::WHITE,
            );
        } else {
            Self::draw_empty_slot(ui.painter(), stock_rect, "↻", card_w);
        }

        if stock_resp.clicked() {
            audio.play(Sfx::CardSlide);
            if let Some(mut card) = self.stock.pop() {
                card.face_up = true;
                self.waste.push(card);
                self.moves += 1;
                self.flight_anim = Some(CardFlightAnim {
                    card,
                    from_pos: stock_rect.min,
                    target: AnimTarget::Waste,
                    start_time: Instant::now(),
                    duration: 0.16,
                });
            } else {
                while let Some(mut card) = self.waste.pop() {
                    card.face_up = false;
                    self.stock.push(card);
                }
            }
        }

        // --- 2. WASTE ---
        let waste_rect = egui::Rect::from_min_size(egui::pos2(col_x(1), top_row_y), card_size);
        let waste_resp = ui.interact(
            waste_rect,
            egui::Id::new("sol_waste"),
            egui::Sense::click_and_drag(),
        );

        let is_dragging_waste = matches!(
            self.dragging,
            Some(DragState {
                source: DragSource::Waste,
                ..
            })
        );
        let is_animating_to_waste = self
            .flight_anim
            .as_ref()
            .map_or(false, |a| a.target == AnimTarget::Waste);

        if is_dragging_waste || is_animating_to_waste {
            if self.waste.len() >= 2 {
                paint_card_at(
                    ui.painter(),
                    waste_rect,
                    &self.waste[self.waste.len() - 2],
                    false,
                );
            } else {
                Self::draw_empty_slot(ui.painter(), waste_rect, "", card_w);
            }
        } else if let Some(top_waste) = self.waste.last() {
            paint_card_at(ui.painter(), waste_rect, top_waste, false);
        } else {
            Self::draw_empty_slot(ui.painter(), waste_rect, "", card_w);
        }

        if waste_resp.drag_started() {
            if let Some(&card) = self.waste.last() {
                audio.play(Sfx::CardSlide);
                let ptr = ui
                    .ctx()
                    .pointer_interact_pos()
                    .unwrap_or(waste_rect.center());
                self.dragging = Some(DragState {
                    source: DragSource::Waste,
                    cards: vec![card],
                    grab_offset: ptr - waste_rect.min,
                });
            }
        } else if waste_resp.secondary_clicked() || waste_resp.double_clicked() {
            if !self.waste.is_empty() {
                self.auto_move(DragSource::Waste, waste_rect.min, audio);
            }
        }

        // --- 3. FOUNDATIONS ---
        let foundation_hints = [
            Suit::Hearts.symbol(),
            Suit::Diamonds.symbol(),
            Suit::Clubs.symbol(),
            Suit::Spades.symbol(),
        ];
        let mut foundation_rects = [egui::Rect::NOTHING; 4];

        for f_idx in 0..4 {
            let f_rect =
                egui::Rect::from_min_size(egui::pos2(col_x(3 + f_idx), top_row_y), card_size);
            foundation_rects[f_idx] = f_rect;

            let f_resp = ui.interact(
                f_rect,
                egui::Id::new(("sol_found", f_idx)),
                egui::Sense::click_and_drag(),
            );

            let is_dragging_this = matches!(
                self.dragging,
                Some(DragState { source: DragSource::Foundation(idx), .. }) if idx == f_idx
            );
            let is_anim_this = self
                .flight_anim
                .as_ref()
                .map_or(false, |a| a.target == AnimTarget::Foundation(f_idx));

            let pile = &self.foundations[f_idx];
            if is_dragging_this || is_anim_this {
                if pile.len() >= 2 {
                    paint_card_at(ui.painter(), f_rect, &pile[pile.len() - 2], false);
                } else {
                    Self::draw_empty_slot(ui.painter(), f_rect, foundation_hints[f_idx], card_w);
                }
            } else if let Some(top_card) = pile.last() {
                paint_card_at(ui.painter(), f_rect, top_card, false);
            } else {
                Self::draw_empty_slot(ui.painter(), f_rect, foundation_hints[f_idx], card_w);
            }

            if f_resp.drag_started() {
                if let Some(&card) = self.foundations[f_idx].last() {
                    audio.play(Sfx::CardSlide);
                    let ptr = ui.ctx().pointer_interact_pos().unwrap_or(f_rect.center());
                    self.dragging = Some(DragState {
                        source: DragSource::Foundation(f_idx),
                        cards: vec![card],
                        grab_offset: ptr - f_rect.min,
                    });
                }
            }
        }

        // --- 4. TABLEAU ---
        let tableau_y = top_row_y + card_h + (board_rect.height() * 0.035).clamp(14.0, 32.0);
        let available_tableau_h = (board_rect.bottom() - tableau_y - 8.0).max(card_h + 20.0);

        let mut tableau_drop_rects = [egui::Rect::NOTHING; 7];
        let mut tableau_last_card_pos = [egui::pos2(0.0, tableau_y); 7];

        for col_idx in 0..7 {
            let x = col_x(col_idx);
            let col_len = self.tableau[col_idx].len();

            let default_face_up_step = (card_h * 0.22).clamp(20.0, 36.0);
            let default_face_down_step = (card_h * 0.13).clamp(12.0, 22.0);

            let total_default_steps: f32 = self.tableau[col_idx]
                .iter()
                .take(col_len.saturating_sub(1))
                .map(|c| {
                    if c.face_up {
                        default_face_up_step
                    } else {
                        default_face_down_step
                    }
                })
                .sum();

            let max_steps_h = (available_tableau_h - card_h).max(20.0);
            let squeeze = if total_default_steps > max_steps_h && total_default_steps > 0.0 {
                max_steps_h / total_default_steps
            } else {
                1.0
            };

            tableau_drop_rects[col_idx] = egui::Rect::from_min_max(
                egui::pos2(x, tableau_y),
                egui::pos2(x + card_w, board_rect.bottom()),
            );

            if col_len == 0 {
                let empty_rect = egui::Rect::from_min_size(egui::pos2(x, tableau_y), card_size);
                tableau_last_card_pos[col_idx] = empty_rect.min;
                Self::draw_empty_slot(ui.painter(), empty_rect, "K", card_w);
            } else {
                let mut current_y = tableau_y;

                for card_idx in 0..col_len {
                    let card = self.tableau[col_idx][card_idx];
                    let step_y = if card.face_up {
                        default_face_up_step * squeeze
                    } else {
                        default_face_down_step * squeeze
                    };

                    let is_hidden_by_drag = matches!(
                        self.dragging,
                        Some(DragState { source: DragSource::Tableau { col, idx }, .. })
                        if col == col_idx && card_idx >= idx
                    );

                    let is_hidden_by_anim = card_idx + 1 == col_len
                        && self
                            .flight_anim
                            .as_ref()
                            .map_or(false, |a| a.target == AnimTarget::Tableau(col_idx));

                    let card_rect = egui::Rect::from_min_size(egui::pos2(x, current_y), card_size);
                    if card_idx + 1 == col_len {
                        tableau_last_card_pos[col_idx] = card_rect.min;
                    }

                    if !is_hidden_by_drag && !is_hidden_by_anim {
                        paint_card_at(ui.painter(), card_rect, &card, false);
                    } else if card_idx == 0 {
                        Self::draw_empty_slot(ui.painter(), card_rect, "K", card_w);
                    }

                    let hit_h = if card_idx + 1 == col_len {
                        card_h
                    } else {
                        step_y
                    };
                    let hit_rect = egui::Rect::from_min_size(
                        egui::pos2(x, current_y),
                        egui::vec2(card_w, hit_h),
                    );

                    let resp = ui.interact(
                        hit_rect,
                        egui::Id::new(("tab_card", col_idx, card_idx)),
                        egui::Sense::click_and_drag(),
                    );

                    if card.face_up && !is_hidden_by_drag {
                        if resp.drag_started() {
                            audio.play(Sfx::CardSlide);
                            let ptr = ui.ctx().pointer_interact_pos().unwrap_or(card_rect.center());
                            let moving_cards = self.tableau[col_idx][card_idx..].to_vec();
                            self.dragging = Some(DragState {
                                source: DragSource::Tableau {
                                    col: col_idx,
                                    idx: card_idx,
                                },
                                cards: moving_cards,
                                grab_offset: ptr - card_rect.min,
                            });
                        } else if resp.secondary_clicked() || resp.double_clicked() {
                            self.auto_move(
                                DragSource::Tableau {
                                    col: col_idx,
                                    idx: card_idx,
                                },
                                card_rect.min,
                                audio,
                            );
                            break;
                        }
                    }

                    current_y += step_y;
                }
            }
        }

        // --- 5. RYSOWANIE PŁYNNEGO LOTU KARTY ---
        if let Some(anim) = &self.flight_anim {
            let elapsed = now.duration_since(anim.start_time).as_secs_f32();
            let raw_t = (elapsed / anim.duration).clamp(0.0, 1.0);
            let eased_t = 1.0 - (1.0 - raw_t).powi(3);

            let to_pos = match anim.target {
                AnimTarget::Waste => waste_rect.min,
                AnimTarget::Foundation(f_idx) => foundation_rects[f_idx].min,
                AnimTarget::Tableau(c_idx) => tableau_last_card_pos[c_idx],
            };

            let current_pos = anim.from_pos.lerp(to_pos, eased_t);
            let top_painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("solitaire_flight_layer"),
            ));
            let r = egui::Rect::from_min_size(current_pos, card_size);
            paint_card_at(&top_painter, r, &anim.card, true);
        }

        // --- 6. RYSOWANIE PRZECIĄGANYCH KART POD KURSOREM ---
        if let Some(drag) = self.dragging.clone() {
            if let Some(ptr) = ui.ctx().pointer_latest_pos() {
                let top_painter = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("solitaire_drag_layer"),
                ));
                let base_pos = ptr - drag.grab_offset;
                let drag_step = (card_h * 0.22).clamp(20.0, 34.0);

                for (i, card) in drag.cards.iter().enumerate() {
                    let r = egui::Rect::from_min_size(
                        base_pos + egui::vec2(0.0, i as f32 * drag_step),
                        card_size,
                    );
                    paint_card_at(&top_painter, r, card, true);
                }

                ui.ctx().request_repaint();

                if ui.input(|i| i.pointer.any_released()) {
                    let dragged_rect = egui::Rect::from_min_size(base_pos, card_size);
                    let drop_point = dragged_rect.center();
                    let mut moved = false;

                    if drag.cards.len() == 1 {
                        for f_idx in 0..4 {
                            if foundation_rects[f_idx].expand(12.0).contains(drop_point)
                                || foundation_rects[f_idx].intersects(dragged_rect)
                            {
                                if self
                                    .execute_move_to_foundation(drag.source, f_idx, audio)
                                    .is_some()
                                {
                                    moved = true;
                                    break;
                                }
                            }
                        }
                    }

                    if !moved {
                        for col_idx in 0..7 {
                            if tableau_drop_rects[col_idx].expand(8.0).contains(drop_point)
                                || tableau_drop_rects[col_idx].intersects(dragged_rect)
                            {
                                if self
                                    .execute_move_to_tableau(drag.source, col_idx, audio)
                                    .is_some()
                                {
                                    break;
                                }
                            }
                        }
                    }

                    self.dragging = None;
                }
            } else if ui.input(|i| i.pointer.any_released()) {
                self.dragging = None;
            }
        }
    }

    fn draw_empty_slot(painter: &egui::Painter, rect: egui::Rect, hint: &str, card_w: f32) {
        let scale = (card_w / 95.0).max(0.5);
        painter.rect(
            rect,
            8.0 * scale,
            egui::Color32::from_black_alpha(45),
            egui::Stroke::new(1.5 * scale, egui::Color32::from_white_alpha(60)),
        );
        if !hint.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                hint,
                egui::FontId::proportional((28.0 * scale).max(14.0)),
                egui::Color32::from_white_alpha(55),
            );
        }
    }
}