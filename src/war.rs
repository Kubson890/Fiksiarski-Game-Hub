use crate::audio::{Sfx, SoundManager};
use crate::cards::{paint_card_at, Card, Deck};
use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RoundWinner {
    Player,
    Bot,
}

#[derive(Clone, Copy)]
enum AnimPhase {
    Dealing {
        start: Instant,
        outcome: Option<RoundWinner>,
    },
    Collecting {
        start: Instant,
        winner: RoundWinner,
    },
}

pub struct WarGame {
    player_deck: VecDeque<Card>,
    bot_deck: VecDeque<Card>,
    table_player: Vec<Card>,
    table_bot: Vec<Card>,
    in_war_state: bool,
    status_text: String,
    anim: Option<AnimPhase>,
    auto_play: bool,
    last_auto_step: Instant,
    winner: Option<&'static str>,
    rounds: u32,
    pub newly_won: bool,
    pub outgoing_step: bool,
}

impl Default for WarGame {
    fn default() -> Self {
        let mut g = Self {
            player_deck: VecDeque::new(),
            bot_deck: VecDeque::new(),
            table_player: Vec::new(),
            table_bot: Vec::new(),
            in_war_state: false,
            status_text: String::new(),
            anim: None,
            auto_play: false,
            last_auto_step: Instant::now(),
            winner: None,
            rounds: 0,
            newly_won: false,
            outgoing_step: false,
        };
        g.new_game();
        g
    }
}

impl WarGame {
    pub fn new_game(&mut self) {
        let deck = Deck::new_shuffled();
        self.player_deck.clear();
        self.bot_deck.clear();
        self.table_player.clear();
        self.table_bot.clear();

        for (i, mut card) in deck.cards.into_iter().enumerate() {
            card.face_up = true;
            if i % 2 == 0 {
                self.player_deck.push_back(card);
            } else {
                self.bot_deck.push_back(card);
            }
        }

        self.in_war_state = false;
        self.anim = None;
        self.auto_play = false;
        self.winner = None;
        self.rounds = 0;
        self.newly_won = false;
        self.outgoing_step = false;
        self.status_text =
            "Kliknij 'Zagraj kartę' (lub Spację), aby rozpocząć pojedynek!".to_string();
    }

    fn flush_pending_collection(&mut self, audio: &SoundManager) {
        if let Some(anim_phase) = self.anim.take() {
            match anim_phase {
                AnimPhase::Dealing {
                    outcome: Some(w), ..
                }
                | AnimPhase::Collecting { winner: w, .. } => {
                    self.transfer_table_cards_to(w, audio);
                }
                AnimPhase::Dealing { outcome: None, .. } => {}
            }
        }
    }

    fn transfer_table_cards_to(&mut self, winner: RoundWinner, audio: &SoundManager) {
        match winner {
            RoundWinner::Player => {
                for mut c in self
                    .table_player
                    .drain(..)
                    .chain(self.table_bot.drain(..))
                {
                    c.face_up = true;
                    self.player_deck.push_back(c);
                }
            }
            RoundWinner::Bot => {
                for mut c in self
                    .table_bot
                    .drain(..)
                    .chain(self.table_player.drain(..))
                {
                    c.face_up = true;
                    self.bot_deck.push_back(c);
                }
            }
        }

        if self.player_deck.is_empty() || self.bot_deck.is_empty() {
            self.finish_by_count(audio);
        }
    }

    pub fn play_step(&mut self, audio: &SoundManager) {
        if self.winner.is_some() {
            return;
        }

        self.flush_pending_collection(audio);
        if self.winner.is_some() {
            return;
        }

        if self.player_deck.is_empty() || self.bot_deck.is_empty() {
            self.finish_by_count(audio);
            return;
        }

        if self.in_war_state {
            if let Some(mut cp) = self.player_deck.pop_front() {
                cp.face_up = false;
                self.table_player.push(cp);
            }
            if let Some(mut cb) = self.bot_deck.pop_front() {
                cb.face_up = false;
                self.table_bot.push(cb);
            }

            if self.player_deck.is_empty() || self.bot_deck.is_empty() {
                self.finish_by_count(audio);
                return;
            }
        }

        let mut p_card = self.player_deck.pop_front().unwrap();
        let mut b_card = self.bot_deck.pop_front().unwrap();
        p_card.face_up = true;
        b_card.face_up = true;

        self.table_player.push(p_card);
        self.table_bot.push(b_card);
        self.rounds += 1;

        let p_val = p_card.rank.war_value();
        let b_val = b_card.rank.war_value();
        let pot_size = self.table_player.len() + self.table_bot.len();

        audio.play(Sfx::CardSlide);

        if p_val > b_val {
            self.in_war_state = false;
            self.status_text = format!(
                "✅ Twoja karta ({}{}) bije kartę wroga ({}{})! Zgarniasz {} kart!",
                p_card.rank.label(),
                p_card.suit.symbol(),
                b_card.rank.label(),
                b_card.suit.symbol(),
                pot_size
            );
            self.anim = Some(AnimPhase::Dealing {
                start: Instant::now(),
                outcome: Some(RoundWinner::Player),
            });
        } else if b_val > p_val {
            self.in_war_state = false;
            self.status_text = format!(
                "❌ Tracisz kartę! Wróg ({}{}) bije Twoją ({}{}) i zabiera {} kart.",
                b_card.rank.label(),
                b_card.suit.symbol(),
                p_card.rank.label(),
                p_card.suit.symbol(),
                pot_size
            );
            self.anim = Some(AnimPhase::Dealing {
                start: Instant::now(),
                outcome: Some(RoundWinner::Bot),
            });
        } else {
            self.in_war_state = true;
            audio.play(Sfx::Check);
            self.status_text = format!(
                "🔥 WOJNA! Obie strony wyrzuciły {}! Kliknij ponownie, aby dołożyć karty!",
                p_card.rank.label()
            );
            self.anim = Some(AnimPhase::Dealing {
                start: Instant::now(),
                outcome: None,
            });
        }
    }

    fn finish_by_count(&mut self, audio: &SoundManager) {
        self.auto_play = false;
        if self.player_deck.len() > self.bot_deck.len() {
            self.winner = Some("🏆 ZWYCIĘSTWO! Zdobyłeś wszystkie karty przeciwnika!");
            self.status_text = self.winner.unwrap().to_string();
            self.newly_won = true;
            audio.play(Sfx::Win);
        } else {
            self.winner = Some("💀 PORAŻKA! Straciłeś wszystkie swoje karty.");
            self.status_text = self.winner.unwrap().to_string();
            audio.play(Sfx::Lose);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        let now = Instant::now();

        if let Some(phase) = self.anim {
            match phase {
                AnimPhase::Dealing { start, outcome } => {
                    let el = now.duration_since(start).as_secs_f32();
                    if el < 0.63 {
                        ui.ctx().request_repaint();
                    } else if let Some(w) = outcome {
                        match w {
                            RoundWinner::Player => audio.play(Sfx::CardPlace),
                            RoundWinner::Bot => audio.play(Sfx::CardSlide),
                        }
                        self.anim = Some(AnimPhase::Collecting {
                            start: now,
                            winner: w,
                        });
                        ui.ctx().request_repaint();
                    } else {
                        self.anim = None;
                    }
                }
                AnimPhase::Collecting { start, winner } => {
                    let el = now.duration_since(start).as_secs_f32();
                    if el < 0.38 {
                        ui.ctx().request_repaint();
                    } else {
                        self.anim = None;
                        self.transfer_table_cards_to(winner, audio);
                        ui.ctx().request_repaint();
                    }
                }
            }
        }

        if self.auto_play && self.winner.is_none() && self.anim.is_none() {
            if now.duration_since(self.last_auto_step).as_millis() >= 180 {
                self.last_auto_step = now;
                self.play_step(audio);
            }
            ui.ctx().request_repaint();
        }

        if ui.input(|i| i.key_pressed(egui::Key::Space)) && self.winner.is_none() {
            self.outgoing_step = true;
            self.play_step(audio);
        }

        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(20, 52, 42));

        let margin = (full_rect.width() * 0.025).clamp(12.0, 28.0);
        let content_rect = full_rect.shrink(margin);

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child_ui;

        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("🔄 Nowa bitwa").size(16.0))
                .clicked()
            {
                audio.play(Sfx::Click);
                self.new_game();
            }
            ui.separator();

            let auto_label = if self.auto_play {
                "⏸ Zatrzymaj Auto-Grę"
            } else {
                "⏩ Szybka Auto-Gra"
            };
            if ui
                .button(egui::RichText::new(auto_label).size(16.0))
                .clicked()
            {
                audio.play(Sfx::Click);
                self.auto_play = !self.auto_play;
                self.last_auto_step = Instant::now();
            }

            ui.separator();
            ui.label(
                egui::RichText::new(format!("Starcia: {}", self.rounds))
                    .size(17.0)
                    .color(egui::Color32::LIGHT_GRAY),
            );
        });

        ui.add_space(12.0);

        let p_cnt = self.player_deck.len();
        let b_cnt = self.bot_deck.len();
        let total = (p_cnt + b_cnt).max(1) as f32;
        let p_ratio = p_cnt as f32 / total;

        let bar_w = ui.available_width();
        let (bar_rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, 26.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(bar_rect, 8.0, egui::Color32::from_rgb(195, 55, 55));

        let p_bar = egui::Rect::from_min_size(bar_rect.min, egui::vec2(bar_w * p_ratio, 26.0));
        ui.painter()
            .rect_filled(p_bar, 8.0, egui::Color32::from_rgb(50, 135, 225));

        ui.painter().text(
            bar_rect.center(),
            egui::Align2::CENTER_CENTER,
            format!(
                "🧑 Twoje karty: {}     vs     🤖 Przeciwnik: {}",
                p_cnt, b_cnt
            ),
            egui::FontId::proportional(15.0),
            egui::Color32::WHITE,
        );

        ui.add_space(16.0);

        let arena_rect = ui.available_rect_before_wrap();
        let card_h = (arena_rect.height() * 0.36).clamp(115.0, 210.0);
        let card_w = card_h / 1.42;
        let card_size = egui::vec2(card_w, card_h);

        let center = arena_rect.center();

        let p_deck_pos = egui::pos2(arena_rect.left() + card_w * 0.35, center.y - card_h * 0.5);
        let b_deck_pos = egui::pos2(arena_rect.right() - card_w * 1.35, center.y - card_h * 0.5);

        let dummy_back = Card::new(crate::cards::Suit::Spades, crate::cards::Rank::Ace, false);

        let collecting_winner = match self.anim {
            Some(AnimPhase::Collecting { winner, .. }) => Some(winner),
            _ => None,
        };

        let p_deck_rect = egui::Rect::from_min_size(p_deck_pos, card_size);
        if !self.player_deck.is_empty() || collecting_winner == Some(RoundWinner::Player) {
            if collecting_winner == Some(RoundWinner::Player) {
                ui.painter().rect_stroke(
                    p_deck_rect.expand(6.0),
                    10.0,
                    egui::Stroke::new(3.5_f32, egui::Color32::from_rgb(80, 235, 120)),
                );
            }
            paint_card_at(ui.painter(), p_deck_rect, &dummy_back, false);
            ui.painter().text(
                p_deck_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("TY\n({})", p_cnt),
                egui::FontId::proportional((card_w * 0.22).max(14.0)),
                egui::Color32::WHITE,
            );
        }

        let b_deck_rect = egui::Rect::from_min_size(b_deck_pos, card_size);
        if !self.bot_deck.is_empty() || collecting_winner == Some(RoundWinner::Bot) {
            if collecting_winner == Some(RoundWinner::Bot) {
                ui.painter().rect_stroke(
                    b_deck_rect.expand(6.0),
                    10.0,
                    egui::Stroke::new(3.5_f32, egui::Color32::from_rgb(245, 80, 80)),
                );
            }
            paint_card_at(ui.painter(), b_deck_rect, &dummy_back, false);
            ui.painter().text(
                b_deck_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("BOT\n({})", b_cnt),
                egui::FontId::proportional((card_w * 0.22).max(14.0)),
                egui::Color32::WHITE,
            );
        }

        let p_target_base = egui::pos2(center.x - card_w * 1.25, center.y - card_h * 0.5);
        let b_target_base = egui::pos2(center.x + card_w * 0.25, center.y - card_h * 0.5);
        let stack_offset = (card_w * 0.20).clamp(14.0, 26.0);

        match self.anim {
            Some(AnimPhase::Dealing { start, .. }) => {
                let el = now.duration_since(start).as_secs_f32();
                let raw_t = (el / 0.25).clamp(0.0, 1.0);
                let deal_t = 1.0 - (1.0 - raw_t).powi(3);

                for (i, card) in self.table_player.iter().enumerate() {
                    let target = p_target_base + egui::vec2(i as f32 * stack_offset, 0.0);
                    let pos = if i + 1 == self.table_player.len() {
                        p_deck_pos.lerp(target, deal_t)
                    } else {
                        target
                    };
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, self.in_war_state && card.face_up);
                }

                for (i, card) in self.table_bot.iter().enumerate() {
                    let target = b_target_base + egui::vec2(i as f32 * stack_offset, 0.0);
                    let pos = if i + 1 == self.table_bot.len() {
                        b_deck_pos.lerp(target, deal_t)
                    } else {
                        target
                    };
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, self.in_war_state && card.face_up);
                }
            }
            Some(AnimPhase::Collecting { start, winner }) => {
                let el = now.duration_since(start).as_secs_f32();
                let raw_t = (el / 0.38).clamp(0.0, 1.0);
                let collect_t = raw_t * raw_t * (3.0 - 2.0 * raw_t);

                let dest_pos = match winner {
                    RoundWinner::Player => p_deck_pos,
                    RoundWinner::Bot => b_deck_pos,
                };

                let outline_col = match winner {
                    RoundWinner::Player => egui::Color32::from_rgb(75, 235, 115),
                    RoundWinner::Bot => egui::Color32::from_rgb(245, 75, 75),
                };

                let arc_lift = (raw_t * std::f32::consts::PI).sin() * 28.0;

                for (i, card) in self.table_player.iter().enumerate() {
                    let start_pos = p_target_base + egui::vec2(i as f32 * stack_offset, 0.0);
                    let pos = start_pos.lerp(dest_pos, collect_t) - egui::vec2(0.0, arc_lift);
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, false);
                    ui.painter().rect_stroke(
                        r.expand(2.0),
                        8.0,
                        egui::Stroke::new(2.5_f32, outline_col),
                    );
                }

                for (i, card) in self.table_bot.iter().enumerate() {
                    let start_pos = b_target_base + egui::vec2(i as f32 * stack_offset, 0.0);
                    let pos = start_pos.lerp(dest_pos, collect_t) + egui::vec2(0.0, arc_lift);
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, false);
                    ui.painter().rect_stroke(
                        r.expand(2.0),
                        8.0,
                        egui::Stroke::new(2.5_f32, outline_col),
                    );
                }

                let pot_total = self.table_player.len() + self.table_bot.len();
                let badge_text = match winner {
                    RoundWinner::Player => format!("+{} kart! 🎉", pot_total),
                    RoundWinner::Bot => "Strata karty! 💀".to_string(),
                };
                ui.painter().text(
                    dest_pos + egui::vec2(card_w * 0.5, -18.0 - raw_t * 16.0),
                    egui::Align2::CENTER_BOTTOM,
                    badge_text,
                    egui::FontId::proportional(20.0),
                    outline_col,
                );
            }
            None => {
                for (i, card) in self.table_player.iter().enumerate() {
                    let pos = p_target_base + egui::vec2(i as f32 * stack_offset, 0.0);
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, self.in_war_state && card.face_up);
                }
                for (i, card) in self.table_bot.iter().enumerate() {
                    let pos = b_target_base + egui::vec2(i as f32 * stack_offset, 0.0);
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, self.in_war_state && card.face_up);
                }
            }
        }

        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            if self.in_war_state {
                "🔥 WOJNA! 🔥"
            } else {
                "⚔"
            },
            egui::FontId::proportional((card_w * 0.32).max(20.0)),
            if self.in_war_state {
                egui::Color32::GOLD
            } else {
                egui::Color32::from_white_alpha(120)
            },
        );

        ui.painter().text(
            egui::pos2(center.x, arena_rect.top() + 22.0),
            egui::Align2::CENTER_CENTER,
            &self.status_text,
            egui::FontId::proportional((arena_rect.height() * 0.045).clamp(16.0, 24.0)),
            if self.in_war_state {
                egui::Color32::GOLD
            } else {
                egui::Color32::from_rgb(245, 235, 165)
            },
        );

        let btn_size = egui::vec2(260.0, 52.0);
        let btn_rect = egui::Rect::from_center_size(
            egui::pos2(center.x, arena_rect.bottom() - 36.0),
            btn_size,
        );

        let mut btn_ui = ui.new_child(egui::UiBuilder::new().max_rect(btn_rect));
        let btn_label = if self.in_war_state {
            "🔥 ROZSTRZYGNIJ WOJNĘ!"
        } else {
            "🃏 Zagraj kartę (Spacja)"
        };

        if btn_ui
            .add_enabled(
                self.winner.is_none(),
                egui::Button::new(egui::RichText::new(btn_label).size(20.0).strong())
                    .min_size(btn_size),
            )
            .clicked()
        {
            self.outgoing_step = true;
            self.play_step(audio);
        }
    }
}