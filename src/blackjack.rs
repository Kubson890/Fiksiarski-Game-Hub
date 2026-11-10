use crate::audio::{Sfx, SoundManager};
use crate::cards::{draw_card_sized, Card, Deck, Rank};
use eframe::egui;

#[derive(PartialEq, Clone, Copy)]
enum GameState {
    Betting,
    PlayerTurn,
    Finished,
}

pub struct BlackjackGame {
    deck: Deck,
    player_hand: Vec<Card>,
    dealer_hand: Vec<Card>,
    state: GameState,
    chips: i32,
    current_bet: i32,
    status_message: String,
    pub is_online: bool,
}

impl Default for BlackjackGame {
    fn default() -> Self {
        Self {
            deck: Deck::new_shuffled(),
            player_hand: Vec::new(),
            dealer_hand: Vec::new(),
            state: GameState::Betting,
            chips: 1000,
            current_bet: 50,
            status_message: "Postaw zakład i kliknij 'Rozdaj karty'!".to_string(),
            is_online: false,
        }
    }
}

impl BlackjackGame {
    pub fn chips(&self) -> i32 {
        self.chips
    }

    pub fn set_chips(&mut self, amount: i32) {
        self.chips = amount;
        if self.current_bet > self.chips && self.chips > 0 {
            self.current_bet = self.chips.min(50);
        }
    }

    fn calculate_score(hand: &[Card]) -> u32 {
        let mut total: u32 = 0;
        let mut aces: u32 = 0;

        for card in hand.iter().filter(|c| c.face_up) {
            total += card.rank.blackjack_value() as u32;
            if card.rank == Rank::Ace {
                aces += 1;
            }
        }

        while total > 21 && aces > 0 {
            total -= 10;
            aces -= 1;
        }

        total
    }

    fn start_round(&mut self, audio: &SoundManager) {
        if self.current_bet > self.chips {
            self.current_bet = self.chips;
        }
        if self.current_bet <= 0 {
            return;
        }

        audio.play(Sfx::CardSlide);
        self.chips -= self.current_bet;
        self.deck = Deck::new_shuffled();
        self.player_hand.clear();
        self.dealer_hand.clear();

        self.player_hand.push(self.deck.draw(true).unwrap());
        self.dealer_hand.push(self.deck.draw(true).unwrap());
        self.player_hand.push(self.deck.draw(true).unwrap());
        self.dealer_hand.push(self.deck.draw(false).unwrap());

        let player_score = Self::calculate_score(&self.player_hand);
        if player_score == 21 {
            self.reveal_and_resolve_dealer(true, audio);
        } else {
            self.state = GameState::PlayerTurn;
            self.status_message = "Twoja tura: Dobierz (Hit) lub Pasuj (Stand).".to_string();
        }
    }

    fn hit(&mut self, audio: &SoundManager) {
        audio.play(Sfx::CardSlide);
        if let Some(card) = self.deck.draw(true) {
            self.player_hand.push(card);
        }

        let score = Self::calculate_score(&self.player_hand);
        if score > 21 {
            for c in &mut self.dealer_hand {
                c.face_up = true;
            }
            self.state = GameState::Finished;
            self.status_message = format!("Fura ({} pkt)! Przegrywasz zakład.", score);
            audio.play(Sfx::Lose);
        } else if score == 21 {
            self.reveal_and_resolve_dealer(false, audio);
        }
    }

    fn double_down(&mut self, audio: &SoundManager) {
        if self.chips >= self.current_bet && self.player_hand.len() == 2 {
            audio.play(Sfx::ChipBet);
            self.chips -= self.current_bet;
            self.current_bet *= 2;
            if let Some(card) = self.deck.draw(true) {
                self.player_hand.push(card);
            }
            let score = Self::calculate_score(&self.player_hand);
            if score > 21 {
                for c in &mut self.dealer_hand {
                    c.face_up = true;
                }
                self.state = GameState::Finished;
                self.status_message = format!("Fura po podwojeniu ({} pkt)! Przegrywasz.", score);
                audio.play(Sfx::Lose);
            } else {
                self.reveal_and_resolve_dealer(false, audio);
            }
        }
    }

    fn reveal_and_resolve_dealer(&mut self, natural_blackjack: bool, audio: &SoundManager) {
        for c in &mut self.dealer_hand {
            c.face_up = true;
        }

        while Self::calculate_score(&self.dealer_hand) < 17 {
            if let Some(card) = self.deck.draw(true) {
                self.dealer_hand.push(card);
            } else {
                break;
            }
        }

        let p_score = Self::calculate_score(&self.player_hand);
        let d_score = Self::calculate_score(&self.dealer_hand);

        self.state = GameState::Finished;

        if natural_blackjack && (d_score != 21 || self.dealer_hand.len() > 2) {
            let payout = (self.current_bet as f32 * 2.5) as i32;
            self.chips += payout;
            self.status_message = format!("🔥 BLACKJACK! Wygrywasz {} żetonów!", payout);
            audio.play(Sfx::Win);
        } else if d_score > 21 {
            let payout = self.current_bet * 2;
            self.chips += payout;
            self.status_message =
                format!("Krupier ma furę ({} pkt)! Wygrywasz {}!", d_score, payout);
            audio.play(Sfx::Win);
        } else if p_score > d_score {
            let payout = self.current_bet * 2;
            self.chips += payout;
            self.status_message =
                format!("Wygrywasz! ({} vs {}) +{} żetonów.", p_score, d_score, payout);
            audio.play(Sfx::Win);
        } else if p_score < d_score {
            self.status_message = format!("Krupier wygrywa ({} vs {}).", d_score, p_score);
            audio.play(Sfx::Lose);
        } else {
            self.chips += self.current_bet;
            self.status_message = format!("Remis (Push) po {} pkt! Zwrot zakładu.", p_score);
            audio.play(Sfx::CardPlace);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        let avail = ui.available_size();
        let is_portrait = avail.y > avail.x || avail.x < 680.0;

        let card_h = if is_portrait {
            (avail.y * 0.22).clamp(110.0, 180.0)
        } else {
            (avail.y * 0.28).clamp(140.0, 240.0)
        };
        let card_w = card_h / 1.42;
        let card_size = egui::vec2(card_w, card_h);
        let v_gap = (avail.y * 0.045).clamp(12.0, 55.0);

        let table_frame = egui::Frame::none()
            .fill(egui::Color32::from_rgb(22, 68, 42))
            .rounding(16.0)
            .inner_margin(if is_portrait { 12.0 } else { 24.0 });

        table_frame.show(ui, |ui| {
            ui.set_min_size(ui.available_size());

            // Top bar: chips + bet
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(format!("🪙 Twoje żetony: {}", self.chips))
                        .size(18.0)
                        .strong()
                        .color(egui::Color32::GOLD),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("Aktualna stawka: {}", self.current_bet))
                        .size(16.0)
                        .color(egui::Color32::WHITE),
                );

                if self.chips == 0 && self.state != GameState::PlayerTurn && !self.is_online {
                    if ui.button("🎁 Odbierz darmowe 500 żetonów").clicked() {
                        audio.play(Sfx::ChipBet);
                        self.chips = 500;
                        self.current_bet = 50;
                    }
                }
            });

            // Calculate remaining space and add top padding to center content
            let used_top = 36.0; // approx top bar height
            let content_h = card_h * 2.0 + 24.0 + v_gap * 4.0 + 60.0;
            let remaining = (avail.y - used_top - content_h - 48.0).max(0.0);
            ui.add_space(remaining / 2.0);

            ui.add_space(v_gap * 0.5);

            // Dealer section
            ui.vertical_centered(|ui| {
                let d_score = Self::calculate_score(&self.dealer_hand);
                ui.label(
                    egui::RichText::new(format!("🎩 Krupier (Punkty: {})", d_score))
                        .size(18.0)
                        .color(egui::Color32::LIGHT_GRAY),
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    let count = self.dealer_hand.len().max(1) as f32;
                    let width = count * (card_w + 10.0);
                    let pad = ((ui.available_width() - width) / 2.0).max(0.0);
                    ui.add_space(pad);
                    for card in &self.dealer_hand {
                        draw_card_sized(ui, card, card_size);
                    }
                });
            });

            ui.add_space(v_gap);

            // Status message
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(&self.status_message)
                        .size(if is_portrait { 18.0 } else { 22.0 })
                        .strong()
                        .color(egui::Color32::from_rgb(255, 235, 140)),
                );
            });

            ui.add_space(v_gap);

            // Player section
            ui.vertical_centered(|ui| {
                let p_score = Self::calculate_score(&self.player_hand);
                ui.label(
                    egui::RichText::new(format!("🧑 Twoje karty (Punkty: {})", p_score))
                        .size(18.0)
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    let count = self.player_hand.len().max(1) as f32;
                    let width = count * (card_w + 10.0);
                    let pad = ((ui.available_width() - width) / 2.0).max(0.0);
                    ui.add_space(pad);
                    for card in &self.player_hand {
                        draw_card_sized(ui, card, card_size);
                    }
                });
            });

            ui.add_space(v_gap);

            // Buttons
            ui.vertical_centered(|ui| {
                match self.state {
                    GameState::Betting | GameState::Finished => {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.spacing_mut().item_spacing.y = 8.0;
                            for &bet in &[10, 50, 100, 250] {
                                let enabled = self.chips >= bet;
                                if ui
                                    .add_enabled(
                                        enabled,
                                        egui::Button::new(format!("Stawka {}", bet))
                                            .min_size(egui::vec2(80.0, 36.0)),
                                    )
                                    .clicked()
                                {
                                    audio.play(Sfx::ChipBet);
                                    self.current_bet = bet;
                                }
                            }
                        });

                        ui.add_space(12.0);

                        if ui
                            .add_enabled(
                                self.chips > 0,
                                egui::Button::new(
                                    egui::RichText::new("🃏 Rozdaj karty").size(18.0).strong(),
                                )
                                .min_size(egui::vec2(200.0, 44.0)),
                            )
                            .clicked()
                        {
                            self.start_round(audio);
                        }
                    }
                    GameState::PlayerTurn => {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.spacing_mut().item_spacing.y = 8.0;
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("➕ Dobierz (Hit)").size(16.0),
                                    )
                                    .min_size(egui::vec2(120.0, 40.0)),
                                )
                                .clicked()
                            {
                                self.hit(audio);
                            }

                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("✋ Pasuj (Stand)").size(16.0),
                                    )
                                    .min_size(egui::vec2(120.0, 40.0)),
                                )
                                .clicked()
                            {
                                audio.play(Sfx::CardPlace);
                                self.reveal_and_resolve_dealer(false, audio);
                            }

                            let can_double =
                                self.player_hand.len() == 2 && self.chips >= self.current_bet;
                            if ui
                                .add_enabled(
                                    can_double,
                                    egui::Button::new(
                                        egui::RichText::new("✖2 Podwój (Double)").size(16.0),
                                    )
                                    .min_size(egui::vec2(120.0, 40.0)),
                                )
                                .clicked()
                            {
                                self.double_down(audio);
                            }
                        });
                    }
                }
            });
        });
    }
}