use crate::audio::{Sfx, SoundManager};
use crate::ludo::PlayerType;
use eframe::egui;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnoColor {
    Red,
    Green,
    Blue,
    Yellow,
    Wild,
}

impl UnoColor {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => UnoColor::Red,
            1 => UnoColor::Green,
            2 => UnoColor::Blue,
            3 => UnoColor::Yellow,
            _ => UnoColor::Wild,
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            UnoColor::Red => 0,
            UnoColor::Green => 1,
            UnoColor::Blue => 2,
            UnoColor::Yellow => 3,
            UnoColor::Wild => 4,
        }
    }

    pub fn egui_color(self) -> egui::Color32 {
        match self {
            UnoColor::Red => egui::Color32::from_rgb(225, 50, 50),
            UnoColor::Green => egui::Color32::from_rgb(40, 180, 75),
            UnoColor::Blue => egui::Color32::from_rgb(40, 125, 235),
            UnoColor::Yellow => egui::Color32::from_rgb(235, 185, 25),
            UnoColor::Wild => egui::Color32::from_rgb(38, 40, 52),
        }
    }

    pub fn egui_dark_color(self) -> egui::Color32 {
        match self {
            UnoColor::Red => egui::Color32::from_rgb(155, 28, 28),
            UnoColor::Green => egui::Color32::from_rgb(22, 115, 45),
            UnoColor::Blue => egui::Color32::from_rgb(22, 75, 155),
            UnoColor::Yellow => egui::Color32::from_rgb(165, 122, 10),
            UnoColor::Wild => egui::Color32::from_rgb(22, 24, 32),
        }
    }

    pub fn name_pl(self) -> &'static str {
        match self {
            UnoColor::Red => "Czerwony",
            UnoColor::Green => "Zielony",
            UnoColor::Blue => "Niebieski",
            UnoColor::Yellow => "Żółty",
            UnoColor::Wild => "Dowolny",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnoValue {
    Number(u8),
    Skip,
    Reverse,
    DrawTwo,
    Wild,
    WildDrawFour,
}

impl UnoValue {
    pub fn label(self) -> String {
        match self {
            UnoValue::Number(n) => n.to_string(),
            UnoValue::Skip => "Ø".to_string(),
            UnoValue::Reverse => "⇄".to_string(),
            UnoValue::DrawTwo => "+2".to_string(),
            UnoValue::Wild => "★".to_string(),
            UnoValue::WildDrawFour => "+4".to_string(),
        }
    }

    pub fn corner_label(self) -> String {
        match self {
            UnoValue::Number(n) => n.to_string(),
            UnoValue::Skip => "STOP".to_string(),
            UnoValue::Reverse => "⇄".to_string(),
            UnoValue::DrawTwo => "+2".to_string(),
            UnoValue::Wild => "KOLOR".to_string(),
            UnoValue::WildDrawFour => "+4".to_string(),
        }
    }

    pub fn sub_label(self) -> &'static str {
        match self {
            UnoValue::Number(_) => "",
            UnoValue::Skip => "STOP",
            UnoValue::Reverse => "ZWROT",
            UnoValue::DrawTwo => "DOBIERZ 2",
            UnoValue::Wild => "ZMIANA KOLORU",
            UnoValue::WildDrawFour => "DZIKA +4",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnoCard {
    pub color: UnoColor,
    pub value: UnoValue,
}

impl UnoCard {
    pub fn can_play_on(&self, top: &UnoCard, active_color: UnoColor) -> bool {
        if self.color == UnoColor::Wild {
            return true;
        }
        if self.color == active_color {
            return true;
        }
        self.value == top.value
    }
}

pub struct UnoGame {
    deck: Vec<UnoCard>,
    discard: Vec<UnoCard>,
    hands: [Vec<UnoCard>; 4],
    player_types: [PlayerType; 4],
    current_turn: usize,
    direction: i8,
    active_color: UnoColor,
    winner: Option<usize>,
    choosing_wild_card: Option<usize>,
    bot_wait_until: Option<Instant>,
    status_text: String,
    pub newly_won: bool,
    pub online_mode: bool,
    pub my_player_idx: usize,
    pub outgoing_start_seed: Option<u64>,
    pub outgoing_play: Option<(usize, u8)>,
    pub outgoing_draw: bool,
}

impl Default for UnoGame {
    fn default() -> Self {
        let mut g = Self {
            deck: Vec::new(),
            discard: Vec::new(),
            hands: Default::default(),
            player_types: [
                PlayerType::Human,
                PlayerType::Bot,
                PlayerType::Bot,
                PlayerType::Bot,
            ],
            current_turn: 0,
            direction: 1,
            active_color: UnoColor::Red,
            winner: None,
            choosing_wild_card: None,
            bot_wait_until: None,
            status_text: String::new(),
            newly_won: false,
            online_mode: false,
            my_player_idx: 0,
            outgoing_start_seed: None,
            outgoing_play: None,
            outgoing_draw: false,
        };
        g.new_game();
        g
    }
}

impl UnoGame {
    fn build_standard_deck() -> Vec<UnoCard> {
        let mut deck = Vec::with_capacity(108);
        for &col in &[
            UnoColor::Red,
            UnoColor::Green,
            UnoColor::Blue,
            UnoColor::Yellow,
        ] {
            deck.push(UnoCard {
                color: col,
                value: UnoValue::Number(0),
            });
            for n in 1..=9 {
                deck.push(UnoCard {
                    color: col,
                    value: UnoValue::Number(n),
                });
                deck.push(UnoCard {
                    color: col,
                    value: UnoValue::Number(n),
                });
            }
            for _ in 0..2 {
                deck.push(UnoCard {
                    color: col,
                    value: UnoValue::Skip,
                });
                deck.push(UnoCard {
                    color: col,
                    value: UnoValue::Reverse,
                });
                deck.push(UnoCard {
                    color: col,
                    value: UnoValue::DrawTwo,
                });
            }
        }
        for _ in 0..4 {
            deck.push(UnoCard {
                color: UnoColor::Wild,
                value: UnoValue::Wild,
            });
            deck.push(UnoCard {
                color: UnoColor::Wild,
                value: UnoValue::WildDrawFour,
            });
        }
        deck
    }

    pub fn new_game(&mut self) {
        let seed: u64 = rand::thread_rng().gen();
        self.init_from_seed(seed);
        if self.online_mode && self.my_player_idx == 0 {
            self.outgoing_start_seed = Some(seed);
        }
    }

    pub fn init_from_seed(&mut self, seed: u64) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut deck = Self::build_standard_deck();
        deck.shuffle(&mut rng);

        for h in &mut self.hands {
            h.clear();
        }
        self.discard.clear();

        for p in 0..4 {
            if self.player_types[p] != PlayerType::Disabled {
                for _ in 0..7 {
                    if let Some(c) = deck.pop() {
                        self.hands[p].push(c);
                    }
                }
            }
        }

        let first_card = loop {
            if let Some(c) = deck.pop() {
                if c.color != UnoColor::Wild {
                    break c;
                }
                deck.insert(0, c);
            } else {
                break UnoCard {
                    color: UnoColor::Red,
                    value: UnoValue::Number(1),
                };
            }
        };

        self.active_color = first_card.color;
        self.discard.push(first_card);
        self.deck = deck;
        self.current_turn = 0;
        if self.player_types[self.current_turn] == PlayerType::Disabled {
            self.current_turn = self.next_active_player(0, 1);
        }
        self.direction = 1;
        self.winner = None;
        self.choosing_wild_card = None;
        self.bot_wait_until = None;
        self.newly_won = false;
        self.outgoing_play = None;
        self.outgoing_draw = false;
        self.status_text = format!(
            "Rozdano karty! Zaczyna {} (Kolor: {})",
            Self::player_name(self.current_turn),
            self.active_color.name_pl()
        );
    }

    pub fn setup_online(&mut self, my_idx: usize, players_count: usize) {
        self.online_mode = true;
        self.my_player_idx = my_idx.min(3);
        let active_humans = players_count.clamp(2, 4);
        for i in 0..4 {
            self.player_types[i] = if i < active_humans {
                PlayerType::Human
            } else {
                PlayerType::Disabled
            };
        }
        if self.my_player_idx == 0 {
            self.new_game();
        } else {
            self.init_from_seed(12345);
            self.status_text = "Oczekiwanie na rozdanie kart przez hosta pokoju...".to_string();
        }
    }

    pub fn state_json(&self) -> Value {
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
            "deck": self.deck,
            "discard": self.discard,
            "hands": self.hands,
            "player_types": player_types,
            "current_turn": self.current_turn,
            "direction": self.direction,
            "active_color": self.active_color.to_u8(),
            "winner": self.winner,
            "status": self.status_text,
        })
    }

    pub fn apply_state_json(&mut self, state: &Value) -> bool {
        let Some(deck_val) = state.get("deck") else {
            return false;
        };
        let Ok(deck) = serde_json::from_value::<Vec<UnoCard>>(deck_val.clone()) else {
            return false;
        };

        let Some(discard_val) = state.get("discard") else {
            return false;
        };
        let Ok(discard) = serde_json::from_value::<Vec<UnoCard>>(discard_val.clone()) else {
            return false;
        };

        let Some(hands_val) = state.get("hands") else {
            return false;
        };
        let Ok(hands) = serde_json::from_value::<[Vec<UnoCard>; 4]>(hands_val.clone()) else {
            return false;
        };

        if let Some(types) = state.get("player_types").and_then(Value::as_array) {
            for (idx, kind) in types.iter().take(4).enumerate() {
                self.player_types[idx] = match kind.as_str().unwrap_or("human") {
                    "bot" => PlayerType::Bot,
                    "disabled" => PlayerType::Disabled,
                    _ => PlayerType::Human,
                };
            }
        }

        self.deck = deck;
        self.discard = discard;
        self.hands = hands;
        self.current_turn = state
            .get("current_turn")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(3) as usize;
        self.direction = state
            .get("direction")
            .and_then(Value::as_i64)
            .unwrap_or(1) as i8;
        self.active_color = UnoColor::from_u8(
            state
                .get("active_color")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u8,
        );
        self.winner = state
            .get("winner")
            .and_then(Value::as_u64)
            .map(|w| w as usize);
        self.status_text = state
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        self.choosing_wild_card = None;
        self.online_mode = true;
        true
    }

    fn player_name(p: usize) -> &'static str {
        match p {
            0 => "Gracz #1",
            1 => "Gracz #2",
            2 => "Gracz #3",
            _ => "Gracz #4",
        }
    }

    fn player_accent(p: usize) -> egui::Color32 {
        match p {
            0 => egui::Color32::from_rgb(235, 75, 75),
            1 => egui::Color32::from_rgb(65, 200, 95),
            2 => egui::Color32::from_rgb(245, 195, 45),
            _ => egui::Color32::from_rgb(75, 150, 245),
        }
    }

    fn active_players_count(&self) -> usize {
        self.player_types
            .iter()
            .filter(|&&t| t != PlayerType::Disabled)
            .count()
    }

    fn next_active_player(&self, from: usize, steps: usize) -> usize {
        let mut cur = from as i8;
        for _ in 0..steps {
            for _ in 0..4 {
                cur = (cur + self.direction).rem_euclid(4);
                if self.player_types[cur as usize] != PlayerType::Disabled {
                    break;
                }
            }
        }
        cur as usize
    }

    fn draw_one_card(&mut self, player: usize) {
        if self.deck.is_empty() && self.discard.len() > 1 {
            let top = self.discard.pop().unwrap();
            self.deck.append(&mut self.discard);
            self.discard.push(top);
        }
        if let Some(c) = self.deck.pop() {
            self.hands[player].push(c);
        }
    }

    pub fn apply_remote_play(&mut self, card_idx: usize, chosen_color: u8, audio: &SoundManager) {
        let p = self.current_turn;
        let _ = self.play_card(
            p,
            card_idx,
            Some(UnoColor::from_u8(chosen_color)),
            false,
            audio,
        );
    }

    pub fn apply_remote_draw(&mut self, audio: &SoundManager) {
        let p = self.current_turn;
        self.draw_action(p, false, audio);
    }

    fn draw_action(&mut self, player: usize, is_local: bool, audio: &SoundManager) {
        if self.winner.is_some() || player != self.current_turn {
            return;
        }
        audio.play(Sfx::CardSlide);
        self.draw_one_card(player);
        if is_local && self.online_mode && player == self.my_player_idx {
            self.outgoing_draw = true;
        }
        let next = self.next_active_player(player, 1);
        self.current_turn = next;
        self.status_text = format!(
            "{} dobrał kartę. Kolej na: {}",
            Self::player_name(player),
            Self::player_name(self.current_turn)
        );
        self.schedule_bot_if_needed();
    }

    fn play_card(
        &mut self,
        player: usize,
        card_idx: usize,
        chosen_wild_color: Option<UnoColor>,
        is_local: bool,
        audio: &SoundManager,
    ) -> bool {
        if self.winner.is_some() || player != self.current_turn {
            return false;
        }
        if card_idx >= self.hands[player].len() {
            return false;
        }
        let top = *self.discard.last().unwrap();
        let card = self.hands[player][card_idx];
        if !card.can_play_on(&top, self.active_color) {
            return false;
        }

        self.hands[player].remove(card_idx);
        self.discard.push(card);

        let new_color = if card.color == UnoColor::Wild {
            chosen_wild_color.unwrap_or(UnoColor::Red)
        } else {
            card.color
        };
        self.active_color = new_color;

        if is_local && self.online_mode && player == self.my_player_idx {
            self.outgoing_play = Some((card_idx, new_color.to_u8()));
        }

        if self.hands[player].is_empty() {
            self.winner = Some(player);
            if self.player_types[player] == PlayerType::Human
                && (!self.online_mode || player == self.my_player_idx)
            {
                self.newly_won = true;
                audio.play(Sfx::Win);
            } else {
                audio.play(Sfx::Lose);
            }
            self.status_text = format!(
                "🏆 WYGRYWA {}! Pozbył się wszystkich kart!",
                Self::player_name(player)
            );
            return true;
        }

        let mut steps = 1;
        let mut action_msg = String::new();

        match card.value {
            UnoValue::Number(_) => {
                audio.play(Sfx::CardPlace);
            }
            UnoValue::Skip => {
                audio.play(Sfx::Check);
                steps = 2;
                action_msg = " (STOP - następny gracz traci kolejkę!)".to_string();
            }
            UnoValue::Reverse => {
                audio.play(Sfx::CardSlide);
                if self.active_players_count() == 2 {
                    steps = 2;
                    action_msg = " (ZWROT przy 2 osobach działa jak STOP!)".to_string();
                } else {
                    self.direction = -self.direction;
                    action_msg = " (Zmiana kierunku gry!)".to_string();
                }
            }
            UnoValue::DrawTwo => {
                audio.play(Sfx::ChessCap);
                let victim = self.next_active_player(player, 1);
                self.draw_one_card(victim);
                self.draw_one_card(victim);
                steps = 2;
                action_msg = format!(" (+2 karty dla {}!)", Self::player_name(victim));
            }
            UnoValue::Wild => {
                audio.play(Sfx::Check);
                action_msg = format!(" (Nowy kolor: {})", new_color.name_pl());
            }
            UnoValue::WildDrawFour => {
                audio.play(Sfx::ChessCap);
                let victim = self.next_active_player(player, 1);
                for _ in 0..4 {
                    self.draw_one_card(victim);
                }
                steps = 2;
                action_msg = format!(
                    " (+4 karty dla {}! Kolor: {})",
                    Self::player_name(victim),
                    new_color.name_pl()
                );
            }
        }

        let uno_scream = if self.hands[player].len() == 1 {
            " 🔥 UNO!"
        } else {
            ""
        };

        self.current_turn = self.next_active_player(player, steps);
        self.status_text = format!(
            "{} zagrał {}{}{} — Kolej na: {}",
            Self::player_name(player),
            card.value.corner_label(),
            action_msg,
            uno_scream,
            Self::player_name(self.current_turn)
        );

        self.schedule_bot_if_needed();
        true
    }

    fn schedule_bot_if_needed(&mut self) {
        if !self.online_mode
            && self.winner.is_none()
            && self.player_types[self.current_turn] == PlayerType::Bot
        {
            self.bot_wait_until = Some(Instant::now() + std::time::Duration::from_millis(550));
        } else {
            self.bot_wait_until = None;
        }
    }

    fn execute_bot_turn(&mut self, audio: &SoundManager) {
        let p = self.current_turn;
        let top = *self.discard.last().unwrap();

        let playable: Vec<usize> = self.hands[p]
            .iter()
            .enumerate()
            .filter(|(_, c)| c.can_play_on(&top, self.active_color))
            .map(|(i, _)| i)
            .collect();

        if let Some(&chosen_idx) = playable.first() {
            let mut color_counts = [0usize; 4];
            for c in &self.hands[p] {
                let idx = c.color.to_u8() as usize;
                if idx < 4 {
                    color_counts[idx] += 1;
                }
            }
            let best_col_u8 = color_counts
                .iter()
                .enumerate()
                .max_by_key(|(_, &cnt)| cnt)
                .map(|(i, _)| i as u8)
                .unwrap_or(0);

            let _ = self.play_card(
                p,
                chosen_idx,
                Some(UnoColor::from_u8(best_col_u8)),
                false,
                audio,
            );
        } else {
            self.draw_action(p, false, audio);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        let now = Instant::now();

        if !self.online_mode
            && self.winner.is_none()
            && self.player_types[self.current_turn] == PlayerType::Bot
        {
            ui.ctx().request_repaint();
            if let Some(until) = self.bot_wait_until {
                if now >= until {
                    self.bot_wait_until = None;
                    self.execute_bot_turn(audio);
                }
            } else {
                self.bot_wait_until = Some(now + std::time::Duration::from_millis(550));
            }
        }

        let full_rect = ui.available_rect_before_wrap();
        // Głębokie, eleganckie tło stołu karcianego
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(18, 24, 34));

        // Dekoracyjna elipsa na środku stołu z poświatą aktualnego koloru UNO
        let felt_rect = full_rect.shrink2(egui::vec2(
            full_rect.width() * 0.08,
            full_rect.height() * 0.14,
        ));
        let glow_col = self.active_color.egui_color().gamma_multiply(0.14);
        ui.painter().rect(
            felt_rect,
            felt_rect.height() * 0.45,
            glow_col,
            egui::Stroke::new(
                2.0_f32,
                self.active_color.egui_color().gamma_multiply(0.38),
            ),
        );

        let margin = (full_rect.width() * 0.022).clamp(12.0, 28.0);
        let content_rect = full_rect.shrink(margin);
        let scale = (content_rect.width() / 1280.0)
            .min(content_rect.height() / 720.0)
            .clamp(0.75, 1.35);

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child_ui;

        // --- 1. GÓRNY PASEK KONTROLNY ---
        ui.horizontal_wrapped(|ui| {
            if !self.online_mode || self.my_player_idx == 0 {
                if ui
                    .button(egui::RichText::new("↻ Nowe rozdanie").size(15.5 * scale))
                    .clicked()
                {
                    audio.play(Sfx::CardSlide);
                    self.new_game();
                }
                ui.separator();
            }

            if !self.online_mode {
                for p in 1..4 {
                    let txt = match self.player_types[p] {
                        PlayerType::Bot => format!("Miejsce #{}: BOT", p + 1),
                        PlayerType::Human => format!("Miejsce #{}: GRACZ", p + 1),
                        PlayerType::Disabled => format!("Miejsce #{}: WYŁ.", p + 1),
                    };
                    if ui
                        .button(egui::RichText::new(txt).size(14.0 * scale))
                        .clicked()
                    {
                        audio.play(Sfx::Click);
                        self.player_types[p] = match self.player_types[p] {
                            PlayerType::Bot => PlayerType::Disabled,
                            PlayerType::Disabled => PlayerType::Bot,
                            PlayerType::Human => PlayerType::Bot,
                        };
                        self.new_game();
                    }
                }
                ui.separator();
            }

            ui.label(
                egui::RichText::new(&self.status_text)
                    .size(17.5 * scale)
                    .strong()
                    .color(egui::Color32::from_rgb(255, 225, 115)),
            );
        });

        ui.add_space(12.0 * scale);

        // --- 2. KARTY GRACZY PRZY STOLE ---
        let avail_w = ui.available_width();
        let gap = 12.0 * scale;
        let card_box_w = ((avail_w - gap * 3.0) / 4.0).max(140.0);
        let card_box_h = (68.0 * scale).clamp(56.0, 86.0);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for p in 0..4 {
                let is_turn = self.current_turn == p && self.winner.is_none();
                let is_disabled = self.player_types[p] == PlayerType::Disabled;
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(card_box_w, card_box_h),
                    egui::Sense::hover(),
                );

                let bg = if is_disabled {
                    egui::Color32::from_rgb(24, 27, 36)
                } else if is_turn {
                    egui::Color32::from_rgb(44, 62, 92)
                } else {
                    egui::Color32::from_rgb(30, 36, 48)
                };

                let stroke = if is_turn {
                    egui::Stroke::new(2.5_f32, egui::Color32::GOLD)
                } else {
                    egui::Stroke::new(1.2_f32, egui::Color32::from_rgb(65, 74, 92))
                };

                ui.painter().rect(rect, 10.0 * scale, bg, stroke);

                // Kolorowy znacznik gracza po lewej
                let dot_center =
                    egui::pos2(rect.left() + 18.0 * scale, rect.top() + 22.0 * scale);
                let accent = if is_disabled {
                    egui::Color32::DARK_GRAY
                } else {
                    Self::player_accent(p)
                };
                ui.painter().circle_filled(dot_center, 7.0 * scale, accent);
                ui.painter().circle_stroke(
                    dot_center,
                    7.0 * scale,
                    egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
                );

                let role_tag = if is_disabled {
                    "Wolne miejsce"
                } else if (self.online_mode && p == self.my_player_idx)
                    || (!self.online_mode && p == 0)
                {
                    "TY"
                } else if !self.online_mode {
                    "BOT"
                } else {
                    "ONLINE"
                };

                ui.painter().text(
                    egui::pos2(rect.left() + 32.0 * scale, rect.top() + 22.0 * scale),
                    egui::Align2::LEFT_CENTER,
                    format!("{} ({})", Self::player_name(p), role_tag),
                    egui::FontId::proportional(15.5 * scale),
                    if is_disabled {
                        egui::Color32::GRAY
                    } else {
                        egui::Color32::WHITE
                    },
                );

                if !is_disabled {
                    let cnt = self.hands[p].len();
                    let sub_txt = if cnt == 1 {
                        "🔥 UNO! (1 karta)".to_string()
                    } else {
                        format!("Kart w ręce: {}", cnt)
                    };
                    ui.painter().text(
                        egui::pos2(rect.left() + 32.0 * scale, rect.bottom() - 18.0 * scale),
                        egui::Align2::LEFT_CENTER,
                        sub_txt,
                        egui::FontId::proportional(13.5 * scale),
                        if cnt == 1 {
                            egui::Color32::GOLD
                        } else {
                            egui::Color32::from_rgb(175, 190, 210)
                        },
                    );

                    // Wizualny mini-wachlarz kart po prawej stronie kafelka
                    let preview_cards = cnt.min(6);
                    let mini_w = 13.0 * scale;
                    let mini_h = 20.0 * scale;
                    let base_x =
                        rect.right() - 18.0 * scale - (preview_cards as f32 * 6.0 * scale);
                    let base_y = rect.center().y - mini_h * 0.5;
                    for i in 0..preview_cards {
                        let mr = egui::Rect::from_min_size(
                            egui::pos2(base_x + i as f32 * 6.0 * scale, base_y),
                            egui::vec2(mini_w, mini_h),
                        );
                        ui.painter().rect(
                            mr,
                            3.0,
                            egui::Color32::from_rgb(200, 45, 45),
                            egui::Stroke::new(1.0_f32, egui::Color32::WHITE),
                        );
                    }
                }
            }
        });

        // --- 3. ŚRODKOWA ARENA STOŁU ---
        let my_seat = if self.online_mode {
            self.my_player_idx
        } else {
            0
        };
        let is_my_turn = self.winner.is_none()
            && self.current_turn == my_seat
            && self.player_types[my_seat] == PlayerType::Human;

        let remaining_h = ui.available_height();
        let wild_bar_h = if self.choosing_wild_card.is_some() {
            64.0 * scale
        } else {
            0.0
        };
        let hand_section_h = (remaining_h * 0.42).clamp(165.0, 260.0);
        let arena_h = (remaining_h - hand_section_h - wild_bar_h - 14.0 * scale).max(160.0);

        let (arena_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), arena_h),
            egui::Sense::hover(),
        );
        let center = arena_rect.center();

        let center_card_h = (arena_h * 0.78).clamp(135.0, 225.0);
        let center_card_w = center_card_h / 1.42;
        let center_card_size = egui::vec2(center_card_w, center_card_h);

        // Dekoracyjny pierścień na środku stołu
        let ring_radius = center_card_h * 0.62;
        ui.painter().circle_stroke(
            center,
            ring_radius,
            egui::Stroke::new(
                3.0_f32,
                self.active_color.egui_color().gamma_multiply(0.45),
            ),
        );

        // Stos dobierania (po lewej od środka)
        let deck_center = egui::pos2(center.x - center_card_w * 0.95, center.y);
        let deck_rect = egui::Rect::from_center_size(deck_center, center_card_size);

        // Efekt grubości talii (kilka kart pod spodem)
        for offset in (1..=3).rev() {
            let off = offset as f32 * 3.0 * scale;
            let under = deck_rect.translate(egui::vec2(-off, off));
            ui.painter().rect(
                under,
                10.0 * scale,
                egui::Color32::from_rgb(24, 26, 36),
                egui::Stroke::new(1.2_f32, egui::Color32::from_white_alpha(130)),
            );
        }

        let deck_resp = ui.interact(
            deck_rect,
            egui::Id::new("uno_draw_pile"),
            egui::Sense::click(),
        );
        let deck_hover = deck_resp.hovered() && is_my_turn && self.choosing_wild_card.is_none();
        let draw_deck_rect = if deck_hover {
            deck_rect.translate(egui::vec2(0.0, -5.0 * scale))
        } else {
            deck_rect
        };
        Self::draw_uno_back(
            ui.painter(),
            draw_deck_rect,
            is_my_turn && self.choosing_wild_card.is_none(),
            self.deck.len(),
        );
        if deck_resp.clicked() && is_my_turn && self.choosing_wild_card.is_none() {
            self.draw_action(my_seat, true, audio);
        }

        // Karta na wierzchu stosu zrzutu (po prawej od środka)
        let discard_center = egui::pos2(center.x + center_card_w * 0.95, center.y);
        let discard_rect = egui::Rect::from_center_size(discard_center, center_card_size);

        if self.discard.len() > 1 {
            let prev_card = &self.discard[self.discard.len() - 2];
            let under_discard = discard_rect.translate(egui::vec2(-6.0 * scale, 5.0 * scale));
            Self::draw_uno_card(ui.painter(), under_discard, prev_card, false, false);
        }
        if let Some(top_card) = self.discard.last() {
            Self::draw_uno_card(ui.painter(), discard_rect, top_card, false, false);
        }

        // Elegancki panel statusu koloru i kierunku po prawej stronie stołu
        let badge_w = (195.0 * scale).clamp(155.0, 235.0);
        let badge_h = (112.0 * scale).clamp(90.0, 135.0);
        let badge_rect = egui::Rect::from_center_size(
            egui::pos2(
                (center.x + center_card_w * 2.55)
                    .min(arena_rect.right() - badge_w * 0.55),
                center.y,
            ),
            egui::vec2(badge_w, badge_h),
        );

        ui.painter().rect(
            badge_rect,
            14.0 * scale,
            egui::Color32::from_rgb(28, 34, 46),
            egui::Stroke::new(2.2_f32, self.active_color.egui_color()),
        );

        let swatch_center =
            egui::pos2(badge_rect.left() + 34.0 * scale, badge_rect.center().y - 12.0 * scale);
        ui.painter().circle(
            swatch_center,
            18.0 * scale,
            self.active_color.egui_color(),
            egui::Stroke::new(2.5_f32, egui::Color32::WHITE),
        );

        ui.painter().text(
            egui::pos2(badge_rect.left() + 62.0 * scale, badge_rect.center().y - 22.0 * scale),
            egui::Align2::LEFT_CENTER,
            "AKTYWNY KOLOR:",
            egui::FontId::proportional(11.5 * scale),
            egui::Color32::from_rgb(160, 175, 195),
        );
        ui.painter().text(
            egui::pos2(badge_rect.left() + 62.0 * scale, badge_rect.center().y - 3.0 * scale),
            egui::Align2::LEFT_CENTER,
            self.active_color.name_pl(),
            egui::FontId::proportional(17.5 * scale),
            egui::Color32::WHITE,
        );

        let dir_txt = if self.direction > 0 {
            "Kierunek: ➡ Zgodnie"
        } else {
            "Kierunek: ⬅ Przeciwnie"
        };
        ui.painter().text(
            egui::pos2(badge_rect.center().x, badge_rect.bottom() - 20.0 * scale),
            egui::Align2::CENTER_CENTER,
            dir_txt,
            egui::FontId::proportional(14.0 * scale),
            egui::Color32::from_rgb(235, 215, 120),
        );

        // --- 4. PASEK WYBORU KOLORU DLA DZIKIEJ KARTY ---
        if let Some(pending_idx) = self.choosing_wild_card {
            ui.vertical_centered(|ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(36, 42, 58))
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(2.0_f32, egui::Color32::GOLD))
                    .inner_margin(egui::Margin::symmetric(18.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Wybierz nowy kolor obowiązujący na stole:")
                                    .size(16.0 * scale)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            );
                            ui.add_space(8.0);
                            for col in [
                                UnoColor::Red,
                                UnoColor::Green,
                                UnoColor::Blue,
                                UnoColor::Yellow,
                            ] {
                                let btn = egui::Button::new(
                                    egui::RichText::new(col.name_pl())
                                        .size(15.0 * scale)
                                        .color(egui::Color32::WHITE)
                                        .strong(),
                                )
                                .fill(col.egui_color())
                                .min_size(egui::vec2(105.0 * scale, 34.0 * scale));
                                if ui.add(btn).clicked() {
                                    self.choosing_wild_card = None;
                                    let _ = self.play_card(
                                        my_seat,
                                        pending_idx,
                                        Some(col),
                                        true,
                                        audio,
                                    );
                                }
                            }
                            if ui
                                .button(egui::RichText::new("Anuluj").size(14.0 * scale))
                                .clicked()
                            {
                                self.choosing_wild_card = None;
                            }
                        });
                    });
            });
            ui.add_space(6.0 * scale);
        }

        // --- 5. DOLNY PANEL: WYŚRODKOWANA RĘKA GRACZA (WACHLARZ) ---
        let hand_frame_rect = ui.available_rect_before_wrap();
        ui.painter().rect(
            hand_frame_rect,
            14.0 * scale,
            egui::Color32::from_rgb(24, 31, 44),
            egui::Stroke::new(
                1.5_f32,
                if is_my_turn {
                    egui::Color32::from_rgb(80, 220, 120)
                } else {
                    egui::Color32::from_rgb(55, 65, 82)
                },
            ),
        );

        let banner_text = if is_my_turn {
            "🟢 TWOJA TURA — Kliknij pasującą kartę (podświetloną na złoto) lub dobierz ze stosu"
        } else {
            "Twoje karty w ręce (czekaj na swoją kolej)"
        };
        ui.painter().text(
            egui::pos2(
                hand_frame_rect.center().x,
                hand_frame_rect.top() + 18.0 * scale,
            ),
            egui::Align2::CENTER_CENTER,
            banner_text,
            egui::FontId::proportional(15.5 * scale),
            if is_my_turn {
                egui::Color32::from_rgb(110, 245, 140)
            } else {
                egui::Color32::from_rgb(175, 185, 205)
            },
        );

        let cards_area = egui::Rect::from_min_max(
            egui::pos2(
                hand_frame_rect.left() + 18.0 * scale,
                hand_frame_rect.top() + 36.0 * scale,
            ),
            egui::pos2(
                hand_frame_rect.right() - 18.0 * scale,
                hand_frame_rect.bottom() - 12.0 * scale,
            ),
        );

        let top_card = *self.discard.last().unwrap();
        let my_cards = self.hands[my_seat].clone();
        let num_cards = my_cards.len();

        if num_cards > 0 {
            let hand_card_h = (cards_area.height() - 14.0 * scale).clamp(110.0, 205.0);
            let hand_card_w = hand_card_h / 1.42;
            let hand_card_size = egui::vec2(hand_card_w, hand_card_h);

            let ideal_step = hand_card_w + 10.0 * scale;
            let max_total_w = (cards_area.width() - hand_card_w).max(10.0);
            let step_x = if num_cards > 1 {
                ideal_step.min(max_total_w / (num_cards - 1) as f32)
            } else {
                0.0
            };

            let total_hand_w = hand_card_w + step_x * (num_cards.saturating_sub(1)) as f32;
            let start_x = cards_area.center().x - total_hand_w * 0.5;
            let base_y = cards_area.bottom() - hand_card_h;

            let mut hovered_idx: Option<usize> = None;
            let mut clicked_card_idx: Option<usize> = None;

            for (idx, card) in my_cards.iter().enumerate() {
                let playable = is_my_turn && card.can_play_on(&top_card, self.active_color);
                let x = start_x + idx as f32 * step_x;
                let hit_w = if idx + 1 == num_cards {
                    hand_card_w
                } else {
                    step_x.max(24.0)
                };
                let hit_rect = egui::Rect::from_min_size(
                    egui::pos2(x, base_y - 12.0 * scale),
                    egui::vec2(hit_w, hand_card_h + 12.0 * scale),
                );
                let resp = ui.interact(
                    hit_rect,
                    egui::Id::new(("uno_hand_card", idx)),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    hovered_idx = Some(idx);
                }
                if resp.clicked() && playable {
                    clicked_card_idx = Some(idx);
                }
            }

            for (idx, card) in my_cards.iter().enumerate() {
                if hovered_idx == Some(idx) {
                    continue;
                }
                let playable = is_my_turn && card.can_play_on(&top_card, self.active_color);
                let lift = if playable { -6.0 * scale } else { 0.0 };
                let x = start_x + idx as f32 * step_x;
                let rect = egui::Rect::from_min_size(egui::pos2(x, base_y + lift), hand_card_size);
                Self::draw_uno_card(
                    ui.painter(),
                    rect,
                    card,
                    playable,
                    is_my_turn && !playable,
                );
            }

            // Rysujemy najechaną kartę na samym wierzchu, żeby nie była przykryta przez sąsiednią
            if let Some(idx) = hovered_idx {
                let card = &my_cards[idx];
                let playable = is_my_turn && card.can_play_on(&top_card, self.active_color);
                let lift = if playable {
                    -18.0 * scale
                } else {
                    -6.0 * scale
                };
                let x = start_x + idx as f32 * step_x;
                let rect = egui::Rect::from_min_size(egui::pos2(x, base_y + lift), hand_card_size);
                Self::draw_uno_card(ui.painter(), rect, card, playable, false);
                ui.ctx().request_repaint();
            }

            if let Some(idx) = clicked_card_idx {
                let card = self.hands[my_seat][idx];
                if card.color == UnoColor::Wild {
                    audio.play(Sfx::Click);
                    self.choosing_wild_card = Some(idx);
                } else {
                    self.choosing_wild_card = None;
                    let _ = self.play_card(my_seat, idx, None, true, audio);
                }
            }
        }
    }

    fn draw_uno_card(
        painter: &egui::Painter,
        rect: egui::Rect,
        card: &UnoCard,
        highlighted: bool,
        dimmed: bool,
    ) {
        let scale = (rect.width() / 105.0).max(0.45);
        let rounding = 11.0 * scale;

        // Cień pod kartą
        painter.rect_filled(
            rect.translate(egui::vec2(3.5 * scale, 5.0 * scale)),
            rounding,
            egui::Color32::from_black_alpha(135),
        );

        // Zewnętrzna biała lub złota ramka karty
        let outer_bg = if highlighted {
            egui::Color32::from_rgb(255, 230, 95)
        } else {
            egui::Color32::from_rgb(248, 248, 245)
        };
        let outer_stroke = if highlighted {
            egui::Stroke::new(2.8 * scale, egui::Color32::GOLD)
        } else {
            egui::Stroke::new(1.5 * scale, egui::Color32::from_rgb(190, 195, 205))
        };
        painter.rect(rect, rounding, outer_bg, outer_stroke);

        // Kolorowe wnętrze karty
        let inner = rect.shrink(5.5 * scale);
        let inner_rounding = 8.0 * scale;
        painter.rect_filled(inner, inner_rounding, card.color.egui_color());

        // Ciemniejszy pas dekoracyjny wewnątrz
        let band_rect = egui::Rect::from_center_size(
            inner.center(),
            egui::vec2(inner.width() * 0.88, inner.height() * 0.62),
        );
        painter.rect_filled(
            band_rect,
            inner.width() * 0.35,
            card.color.egui_dark_color(),
        );

        let center = rect.center();
        let circle_r = rect.width() * 0.33;

        // Dla kart Wild rysujemy 4 kolorowe ćwiartki wokół środka
        if card.color == UnoColor::Wild {
            let off = circle_r * 0.42;
            let quads = [
                (egui::vec2(-off, -off), UnoColor::Red.egui_color()),
                (egui::vec2(off, -off), UnoColor::Blue.egui_color()),
                (egui::vec2(-off, off), UnoColor::Yellow.egui_color()),
                (egui::vec2(off, off), UnoColor::Green.egui_color()),
            ];
            for (d, col) in quads {
                painter.circle_filled(center + d, circle_r * 0.58, col);
            }
        }

        // Biały medalion na środku karty
        painter.circle(
            center,
            circle_r,
            egui::Color32::from_rgb(250, 250, 248),
            egui::Stroke::new(2.2 * scale, egui::Color32::WHITE),
        );

        let main_label = card.value.label();
        let text_col = if card.color == UnoColor::Wild {
            egui::Color32::from_rgb(25, 28, 38)
        } else {
            card.color.egui_color()
        };

        // Cień pod główną cyfrą/symbolem
        painter.text(
            center + egui::vec2(1.5 * scale, 2.0 * scale),
            egui::Align2::CENTER_CENTER,
            &main_label,
            egui::FontId::proportional((38.0 * scale).max(16.0)),
            egui::Color32::from_black_alpha(70),
        );
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            &main_label,
            egui::FontId::proportional((38.0 * scale).max(16.0)),
            text_col,
        );

        // Narożniki: lewy górny oraz prawy dolny
        let corner_txt = card.value.corner_label();
        let corner_font = if corner_txt.len() > 2 {
            (11.5 * scale).max(8.5)
        } else {
            (16.5 * scale).max(11.0)
        };

        painter.text(
            inner.left_top() + egui::vec2(6.0 * scale, 4.0 * scale),
            egui::Align2::LEFT_TOP,
            &corner_txt,
            egui::FontId::proportional(corner_font),
            egui::Color32::WHITE,
        );

        let sub = card.value.sub_label();
        if !sub.is_empty() {
            painter.text(
                egui::pos2(rect.center().x, inner.bottom() - 6.0 * scale),
                egui::Align2::CENTER_BOTTOM,
                sub,
                egui::FontId::proportional((10.5 * scale).max(8.0)),
                egui::Color32::WHITE,
            );
        } else {
            painter.text(
                inner.right_bottom() - egui::vec2(6.0 * scale, 4.0 * scale),
                egui::Align2::RIGHT_BOTTOM,
                &corner_txt,
                egui::FontId::proportional(corner_font),
                egui::Color32::WHITE,
            );
        }

        // Przyciemnienie niegrywalnych kart podczas własnej tury
        if dimmed {
            painter.rect_filled(rect, rounding, egui::Color32::from_black_alpha(95));
        }
    }

    fn draw_uno_back(
        painter: &egui::Painter,
        rect: egui::Rect,
        highlight: bool,
        remaining_count: usize,
    ) {
        let scale = (rect.width() / 105.0).max(0.45);
        let rounding = 11.0 * scale;

        painter.rect_filled(
            rect.translate(egui::vec2(3.5 * scale, 5.0 * scale)),
            rounding,
            egui::Color32::from_black_alpha(135),
        );

        let stroke = if highlight {
            egui::Stroke::new(3.2 * scale, egui::Color32::GOLD)
        } else {
            egui::Stroke::new(2.0 * scale, egui::Color32::WHITE)
        };

        painter.rect(
            rect,
            rounding,
            egui::Color32::from_rgb(26, 30, 42),
            stroke,
        );

        let inner = rect.shrink(6.0 * scale);
        painter.rect_stroke(
            inner,
            8.0 * scale,
            egui::Stroke::new(1.2 * scale, egui::Color32::from_white_alpha(70)),
        );

        painter.circle(
            rect.center(),
            rect.width() * 0.36,
            egui::Color32::from_rgb(215, 45, 45),
            egui::Stroke::new(2.5 * scale, egui::Color32::GOLD),
        );

        painter.text(
            rect.center() - egui::vec2(0.0, 6.0 * scale),
            egui::Align2::CENTER_CENTER,
            "UNO",
            egui::FontId::proportional((22.0 * scale).max(13.0)),
            egui::Color32::GOLD,
        );
        painter.text(
            rect.center() + egui::vec2(0.0, 12.0 * scale),
            egui::Align2::CENTER_CENTER,
            "+ DOBIERZ",
            egui::FontId::proportional((10.5 * scale).max(8.0)),
            egui::Color32::WHITE,
        );

        painter.text(
            egui::pos2(rect.center().x, inner.bottom() - 5.0 * scale),
            egui::Align2::CENTER_BOTTOM,
            format!("W talii: {}", remaining_count),
            egui::FontId::proportional((10.5 * scale).max(8.0)),
            egui::Color32::from_rgb(185, 195, 215),
        );
    }
}