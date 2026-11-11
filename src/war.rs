use crate::audio::{Sfx, SoundManager};
use crate::cards::{paint_card_at, Card, Deck};
use eframe::egui;
use rand::Rng;
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
    pub online_mode: bool,
    pub online_lobby: bool,
    pub my_seat: usize,
    pub my_ready: bool,
    pub opponent_ready: bool,
    pub enemy_present: bool,
    start_sent: bool,
    pub outgoing_ready: bool,
    pub outgoing_start_seed: Option<u64>,
    pub my_step_confirmed: bool,
    pub opponent_step_confirmed: bool,
    pub opponent_nick: Option<String>,
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
            online_mode: false,
            online_lobby: false,
            my_seat: 0,
            my_ready: false,
            opponent_ready: false,
            enemy_present: false,
            start_sent: false,
            outgoing_ready: false,
            outgoing_start_seed: None,
            my_step_confirmed: false,
            opponent_step_confirmed: false,
            opponent_nick: None,
        };
        g.new_game();
        g
    }
}

impl WarGame {
    pub fn opponent_display_name(&self) -> &str {
        if self.online_mode {
            if let Some(nick) = &self.opponent_nick {
                let trimmed = nick.trim();
                if !trimmed.is_empty() {
                    return trimmed;
                }
            }
            "Gracz 2"
        } else {
            "Bot"
        }
    }
    fn deal_from_deck(&mut self, deck: Deck, seat: usize) {
        let mut even: VecDeque<Card> = VecDeque::new();
        let mut odd: VecDeque<Card> = VecDeque::new();
        for (i, mut card) in deck.cards.into_iter().enumerate() {
            card.face_up = true;
            if i % 2 == 0 {
                even.push_back(card);
            } else {
                odd.push_back(card);
            }
        }
        if seat % 2 == 0 {
            self.player_deck = even;
            self.bot_deck = odd;
        } else {
            self.player_deck = odd;
            self.bot_deck = even;
        }
    }

    fn clear_table(&mut self) {
        self.player_deck.clear();
        self.bot_deck.clear();
        self.table_player.clear();
        self.table_bot.clear();
        self.in_war_state = false;
        self.anim = None;
        self.auto_play = false;
        self.winner = None;
        self.rounds = 0;
        self.newly_won = false;
        self.outgoing_step = false;
        self.my_step_confirmed = false;
        self.opponent_step_confirmed = false;
    }

    pub fn new_game(&mut self) {
        let seat = if self.online_mode { self.my_seat } else { 0 };
        let deck = Deck::new_shuffled();
        self.clear_table();
        self.deal_from_deck(deck, seat);
        self.status_text =
            "Kliknij 'Zagraj kartę' (lub Spację), aby rozpocząć pojedynek!".to_string();
    }

    pub fn setup_online(&mut self, seat: usize) {
        self.online_mode = true;
        self.online_lobby = true;
        self.my_seat = seat.min(1);
        self.my_ready = false;
        self.opponent_ready = false;
        self.enemy_present = false;
        self.opponent_nick = None;
        self.start_sent = false;
        self.outgoing_ready = false;
        self.outgoing_start_seed = None;
        self.my_step_confirmed = false;
        self.opponent_step_confirmed = false;
        self.clear_table();
        self.status_text = "🌐 Poczekalnia — potwierdź gotowość, aby zagrać.".to_string();
    }

    pub fn reset_online_waiting(&mut self) {
        let seat = self.my_seat;
        self.setup_online(seat);
    }

    pub fn set_enemy_present(&mut self, present: bool) {
        if self.enemy_present != present {
            self.enemy_present = present;
            if !present {
                self.opponent_ready = false;
                self.my_ready = false;
            }
            if self.online_mode && self.online_lobby {
                self.update_lobby_status();
            }
        }
    }

    fn update_lobby_status(&mut self) {
        if !self.enemy_present {
            self.status_text = "⏳ Czekam, aż drugi gracz kliknie w wojnę...".to_string();
        } else {
            let opp = self.opponent_display_name().to_string();
            if self.my_ready && self.opponent_ready {
                self.status_text = "✅ Obaj gotowi — rozdaję karty...".to_string();
            } else if self.my_ready {
                self.status_text = format!("⏳ Czekam, aż {} potwierdzi gotowość...", opp);
            } else if self.opponent_ready {
                self.status_text = format!("🔥 {} jest gotowy — potwierdź i Ty!", opp);
            } else {
                self.status_text = "Obaj gracze są przy stole — potwierdźcie gotowość.".to_string();
            }
        }
    }

    pub fn mark_ready(&mut self, audio: &SoundManager) {
        if !self.online_mode || !self.online_lobby || self.my_ready || !self.enemy_present {
            return;
        }
        audio.play(Sfx::Check);
        self.my_ready = true;
        self.outgoing_ready = true;
        self.update_lobby_status();
        self.try_host_start();
    }

    pub fn apply_remote_ready(&mut self, from_seat: usize, audio: &SoundManager) {
        if !self.online_mode || from_seat == self.my_seat {
            return;
        }
        if !self.opponent_ready {
            audio.play(Sfx::Check);
        }
        self.opponent_ready = true;
        self.update_lobby_status();
        self.try_host_start();
    }

    fn try_host_start(&mut self) {
        if self.my_seat != 0 || !self.my_ready || !self.opponent_ready || self.start_sent {
            return;
        }
        self.start_sent = true;
        let seed: u64 = rand::thread_rng().gen();
        self.begin_online(seed);
        self.outgoing_start_seed = Some(seed);
    }

    fn begin_online(&mut self, seed: u64) {
        self.online_lobby = false;
        self.clear_table();
        self.my_step_confirmed = false;
        self.opponent_step_confirmed = false;
        let deck = Deck::new_seeded(seed);
        self.deal_from_deck(deck, self.my_seat);
        let opp = self.opponent_display_name().to_string();
        self.status_text =
            format!("🃏 Karty rozdane! Zagraj kartę — ruch potwierdzają obaj gracze (Ty i {}).", opp);
    }

    pub fn apply_remote_start(&mut self, seed: u64) {
        if !self.online_mode {
            return;
        }
        self.begin_online(seed);
        self.outgoing_start_seed = None;
    }

    pub fn confirm_my_step(&mut self, audio: &SoundManager) {
        if !self.online_mode || self.my_step_confirmed || self.winner.is_some() || self.anim.is_some() {
            return;
        }
        audio.play(Sfx::CardSlide);
        self.my_step_confirmed = true;
        self.outgoing_step = true;

        if self.opponent_step_confirmed {
            self.my_step_confirmed = false;
            self.opponent_step_confirmed = false;
            self.play_step(audio);
        } else {
            let opp = self.opponent_display_name().to_string();
            self.status_text = format!("✅ Karta zagrana — czekasz na ruch gracza {}...", opp);
        }
    }

    pub fn apply_remote_step(&mut self, from_seat: usize, audio: &SoundManager) {
        if !self.online_mode || self.online_lobby || from_seat == self.my_seat {
            return;
        }
        self.opponent_step_confirmed = true;
        if self.my_step_confirmed && self.anim.is_none() {
            self.my_step_confirmed = false;
            self.opponent_step_confirmed = false;
            self.play_step(audio);
        } else {
            let opp = self.opponent_display_name().to_string();
            self.status_text = format!("⚡ {} zagrał kartę! Twój ruch!", opp);
        }
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

        let (Some(mut p_card), Some(mut b_card)) = (self.player_deck.pop_front(), self.bot_deck.pop_front()) else {
            self.finish_by_count(audio);
            return;
        };
        p_card.face_up = true;
        b_card.face_up = true;

        self.table_player.push(p_card);
        self.table_bot.push(b_card);
        self.rounds += 1;

        let p_val = p_card.rank.war_value();
        let b_val = b_card.rank.war_value();
        let pot_size = self.table_player.len() + self.table_bot.len();

        audio.play(Sfx::CardSlide);

        let opp = self.opponent_display_name().to_string();
        if p_val > b_val {
            self.in_war_state = false;
            self.status_text = format!(
                "✅ Twoja ({}{}) bije kartę {} ({}{})! Zgarniasz {} kart!",
                p_card.rank.label(),
                p_card.suit.symbol(),
                opp,
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
                "❌ Tracisz! {} ({}{}) bije Twoją ({}{}) i zabiera {} kart.",
                opp,
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
                "🔥 WOJNA! Obie strony mają {}! Dłóż karty!",
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
        let msg = if self.player_deck.len() > self.bot_deck.len() {
            self.newly_won = true;
            audio.play(Sfx::Win);
            "🏆 ZWYCIĘSTWO! Zdobyłeś wszystkie karty!"
        } else {
            audio.play(Sfx::Lose);
            "💀 PORAŻKA! Straciłeś swoje karty."
        };
        self.winner = Some(msg);
        self.status_text = msg.to_string();
    }

    fn render_online_lobby(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(20, 52, 42));

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(full_rect.shrink(18.0))
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        let ui = &mut child_ui;

        ui.add_space(24.0);
        ui.label(
            egui::RichText::new("⚔ Wojna — Stolik online")
                .size(26.0)
                .strong()
                .color(egui::Color32::WHITE),
        );
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Karty zostaną rozdane, gdy obaj gracze potwierdzą gotowość.")
                .size(15.0)
                .color(egui::Color32::from_rgb(200, 220, 210)),
        );
        ui.add_space(20.0);

        let my_label = if self.my_ready { "✅ Gotowy" } else { "⏳ Niegotowy" };
        let opp_name = self.opponent_display_name();
        let opp_label = if !self.enemy_present {
            "❌ Brak gracza (czeka na dołączenie)"
        } else if self.opponent_ready {
            "✅ Gotowy"
        } else {
            "⏳ Niegotowy"
        };
        let opp_display = if self.enemy_present {
            format!("🌐 {}: {}", opp_name, opp_label)
        } else {
            "🌐 Przeciwnik: ❌ Brak gracza (czeka na dołączenie)".to_string()
        };

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(28, 62, 52))
            .rounding(10.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_width((full_rect.width() - 100.0).clamp(200.0, 380.0));
                ui.label(
                    egui::RichText::new(format!("🧑 Ty (miejsce #{}): {}", self.my_seat + 1, my_label))
                        .size(17.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
            });
        ui.add_space(8.0);
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(28, 62, 52))
            .rounding(10.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_width((full_rect.width() - 100.0).clamp(200.0, 380.0));
                ui.label(
                    egui::RichText::new(opp_display)
                        .size(17.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
            });

        ui.add_space(18.0);
        ui.label(
            egui::RichText::new(&self.status_text)
                .size(16.0)
                .color(egui::Color32::from_rgb(245, 235, 165)),
        );

        ui.add_space(16.0);
        if self.my_ready {
            ui.label(
                egui::RichText::new("Czekam na start...")
                    .size(16.0)
                    .color(egui::Color32::GOLD),
            );
        } else {
            let can_ready = self.enemy_present;
            let btn_text = if can_ready {
                "✅ Jestem gotowy"
            } else {
                "⏳ Czekam na drugiego gracza..."
            };
            if ui
                .add_enabled(
                    can_ready,
                    egui::Button::new(
                        egui::RichText::new(btn_text).size(19.0).strong(),
                    )
                    .min_size(egui::vec2(260.0, 48.0)),
                )
                .clicked()
            {
                self.mark_ready(audio);
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        let now = Instant::now();

        if self.online_mode && self.online_lobby {
            self.render_online_lobby(ui, audio);
            return;
        }

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

        let can_press = if self.online_mode {
            self.enemy_present
                && !self.my_step_confirmed
                && self.anim.is_none()
                && self.winner.is_none()
        } else {
            self.anim.is_none() && self.winner.is_none()
        };

        if self.online_mode
            && self.my_step_confirmed
            && self.opponent_step_confirmed
            && self.anim.is_none()
            && self.winner.is_none()
        {
            self.my_step_confirmed = false;
            self.opponent_step_confirmed = false;
            self.play_step(audio);
        }

        if self.auto_play && !self.online_mode && self.winner.is_none() && self.anim.is_none() {
            if now.duration_since(self.last_auto_step).as_millis() >= 180 {
                self.last_auto_step = now;
                self.play_step(audio);
            }
            ui.ctx().request_repaint();
        }

        if can_press
            && ui.input(|i| i.key_pressed(egui::Key::Space))
            && self.winner.is_none()
            && self.anim.is_none()
        {
            if self.online_mode {
                self.confirm_my_step(audio);
            } else {
                self.play_step(audio);
            }
        }

        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(20, 52, 42));

        let margin = (full_rect.width() * 0.025).clamp(12.0, 28.0);
        let content_rect = full_rect.shrink(margin);
        let is_portrait = content_rect.height() > content_rect.width() || content_rect.width() < 640.0;

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child_ui;

        ui.horizontal_wrapped(|ui| {
            if self.online_mode {
                let opp_name = self.opponent_display_name();
                let status_label = if self.my_step_confirmed && self.opponent_step_confirmed {
                    "🌐 Stół online — odsłaniam karty...".to_string()
                } else if self.my_step_confirmed {
                    format!("🌐 Stół online — czekasz na ruch {}", opp_name)
                } else if self.opponent_step_confirmed {
                    format!("🌐 Stół online — {} czeka na Twój ruch!", opp_name)
                } else {
                    format!("🌐 Stół online — pojedynek z {}", opp_name)
                };
                ui.label(
                    egui::RichText::new(status_label)
                        .size(16.0)
                        .strong()
                        .color(egui::Color32::GOLD),
                );
            } else {
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
            }

            ui.separator();
            ui.label(
                egui::RichText::new(format!("Starcia: {}", self.rounds))
                    .size(16.0)
                    .color(egui::Color32::LIGHT_GRAY),
            );
        });

        ui.add_space(8.0);

        let p_cnt = self.player_deck.len();
        let b_cnt = self.bot_deck.len();
        let total = (p_cnt + b_cnt).max(1) as f32;
        let p_ratio = p_cnt as f32 / total;

        let bar_w = ui.available_width();
        let (bar_rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, 20.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(bar_rect, 6.0, egui::Color32::from_rgb(195, 55, 55));

        let p_bar = egui::Rect::from_min_size(bar_rect.min, egui::vec2(bar_w * p_ratio, 20.0));
        ui.painter()
            .rect_filled(p_bar, 6.0, egui::Color32::from_rgb(50, 135, 225));

        ui.painter().text(
            bar_rect.center(),
            egui::Align2::CENTER_CENTER,
            format!(
                "🧑 Ty: {}     |     {} {}: {}",
                p_cnt,
                if self.online_mode { "🌐" } else { "🤖" },
                self.opponent_display_name(),
                b_cnt
            ),
            egui::FontId::proportional(14.0),
            egui::Color32::WHITE,
        );

        ui.add_space(10.0);

        let arena_rect = ui.available_rect_before_wrap();
        
        let card_h = if is_portrait {
            (arena_rect.height() * 0.22).clamp(100.0, 175.0)
        } else {
            (arena_rect.height() * 0.36).clamp(115.0, 210.0)
        };
        let card_w = card_h / 1.42;
        let card_size = egui::vec2(card_w, card_h);

        let center = arena_rect.center();

        let (p_deck_pos, b_deck_pos, p_target_base, b_target_base) = if is_portrait {
            (
                egui::pos2(center.x - card_w * 0.5, arena_rect.bottom() - card_h - 60.0),
                egui::pos2(center.x - card_w * 0.5, arena_rect.top() + 40.0),
                egui::pos2(center.x - card_w * 0.5, center.y + card_h * 0.1),
                egui::pos2(center.x - card_w * 0.5, center.y - card_h * 1.1),
            )
        } else {
            (
                egui::pos2(arena_rect.left() + card_w * 0.35, center.y - card_h * 0.5),
                egui::pos2(arena_rect.right() - card_w * 1.35, center.y - card_h * 0.5),
                egui::pos2(center.x - card_w * 1.25, center.y - card_h * 0.5),
                egui::pos2(center.x + card_w * 0.25, center.y - card_h * 0.5),
            )
        };
        
        let stack_offset = if is_portrait {
            egui::vec2(card_w * 0.25, 0.0)
        } else {
            egui::vec2(card_w * 0.20, 0.0)
        };

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
            let opp_name = self.opponent_display_name();
            let label = if self.online_mode {
                if opp_name.chars().count() > 8 {
                    let truncated: String = opp_name.chars().take(7).collect();
                    format!("{}..", truncated)
                } else {
                    opp_name.to_uppercase()
                }
            } else {
                "BOT".to_string()
            };
            ui.painter().text(
                b_deck_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}\n({})", label, b_cnt),
                egui::FontId::proportional((card_w * 0.20).clamp(12.0, 18.0)),
                egui::Color32::WHITE,
            );
        }

        match self.anim {
            Some(AnimPhase::Dealing { start, .. }) => {
                let el = now.duration_since(start).as_secs_f32();
                let raw_t = (el / 0.25).clamp(0.0, 1.0);
                let deal_t = 1.0 - (1.0 - raw_t).powi(3);

                for (i, card) in self.table_player.iter().enumerate() {
                    let target = p_target_base + stack_offset * (i as f32);
                    let pos = if i + 1 == self.table_player.len() {
                        p_deck_pos.lerp(target, deal_t)
                    } else {
                        target
                    };
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, self.in_war_state && card.face_up);
                }

                for (i, card) in self.table_bot.iter().enumerate() {
                    let target = b_target_base + stack_offset * (i as f32);
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
                    let start_pos = p_target_base + stack_offset * (i as f32);
                    let pos = start_pos.lerp(dest_pos, collect_t) - egui::vec2(arc_lift, arc_lift);
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, false);
                    ui.painter().rect_stroke(
                        r.expand(2.0),
                        8.0,
                        egui::Stroke::new(2.5_f32, outline_col),
                    );
                }

                for (i, card) in self.table_bot.iter().enumerate() {
                    let start_pos = b_target_base + stack_offset * (i as f32);
                    let pos = start_pos.lerp(dest_pos, collect_t) + egui::vec2(arc_lift, arc_lift);
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
                    let pos = p_target_base + stack_offset * (i as f32);
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, self.in_war_state && card.face_up);
                }
                for (i, card) in self.table_bot.iter().enumerate() {
                    let pos = b_target_base + stack_offset * (i as f32);
                    let r = egui::Rect::from_min_size(pos, card_size);
                    paint_card_at(ui.painter(), r, card, self.in_war_state && card.face_up);
                }
            }
        }

        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            if self.in_war_state { "🔥 WOJNA! 🔥" } else { "⚔" },
            egui::FontId::proportional((card_w * 0.32).max(20.0)),
            if self.in_war_state { egui::Color32::GOLD } else { egui::Color32::from_white_alpha(120) },
        );

        let status_y = if is_portrait {
            arena_rect.top() + 14.0
        } else {
            arena_rect.top() + 22.0
        };
        ui.painter().text(
            egui::pos2(center.x, status_y),
            egui::Align2::CENTER_CENTER,
            &self.status_text,
            egui::FontId::proportional((arena_rect.height() * 0.045).clamp(14.0, 24.0)),
            if self.in_war_state { egui::Color32::GOLD } else { egui::Color32::from_rgb(245, 235, 165) },
        );

        let btn_size = egui::vec2(260.0, 48.0);
        let btn_rect = egui::Rect::from_center_size(
            egui::pos2(center.x, arena_rect.bottom() - 30.0),
            btn_size,
        );

        let mut btn_ui = ui.new_child(egui::UiBuilder::new().max_rect(btn_rect));
        let btn_label = if self.online_mode {
            if !self.enemy_present {
                "⏳ Czekam na przeciwnika"
            } else if self.my_step_confirmed {
                "⏳ Czekaj na ruch przeciwnika..."
            } else if self.opponent_step_confirmed {
                if self.in_war_state {
                    "🔥 ROZSTRZYGNIJ WOJNĘ! (przeciwnik czeka)"
                } else {
                    "🃏 Zagraj kartę (przeciwnik czeka!)"
                }
            } else if self.in_war_state {
                "🔥 ROZSTRZYGNIJ WOJNĘ!"
            } else {
                "🃏 Zagraj kartę"
            }
        } else if self.in_war_state {
            "🔥 ROZSTRZYGNIJ WOJNĘ!"
        } else {
            "🃏 Zagraj kartę"
        };

        if btn_ui
            .add_enabled(
                can_press,
                egui::Button::new(egui::RichText::new(btn_label).size(18.0).strong())
                    .min_size(btn_size),
            )
            .clicked()
        {
            if self.online_mode {
                self.confirm_my_step(audio);
            } else {
                self.play_step(audio);
            }
        }
    }
}