use eframe::egui;
use rand::seq::SliceRandom;
use rand::thread_rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suit {
    Hearts,   // Kier ♥
    Diamonds, // Karo ♦
    Clubs,    // Trefl ♣
    Spades,   // Pik ♠
}

impl Suit {
    pub fn symbol(&self) -> &'static str {
        match self {
            Suit::Hearts => "♥",
            Suit::Diamonds => "♦",
            Suit::Clubs => "♣",
            Suit::Spades => "♠",
        }
    }

    pub fn is_red(&self) -> bool {
        matches!(self, Suit::Hearts | Suit::Diamonds)
    }

    pub fn color(&self) -> egui::Color32 {
        if self.is_red() {
            egui::Color32::from_rgb(220, 45, 45)
        } else {
            egui::Color32::from_rgb(30, 30, 35)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Ace = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Jack = 11,
    Queen = 12,
    King = 13,
}

impl Rank {
    pub fn label(&self) -> &'static str {
        match self {
            Rank::Ace => "A",
            Rank::Two => "2",
            Rank::Three => "3",
            Rank::Four => "4",
            Rank::Five => "5",
            Rank::Six => "6",
            Rank::Seven => "7",
            Rank::Eight => "8",
            Rank::Nine => "9",
            Rank::Ten => "10",
            Rank::Jack => "J",
            Rank::Queen => "Q",
            Rank::King => "K",
        }
    }

    pub fn value(&self) -> u8 {
        *self as u8
    }
    pub fn war_value(&self) -> u8 {
        match self {
            Rank::Ace => 14,
            other => *other as u8,
        }
    }
    pub fn blackjack_value(&self) -> u8 {
        match self {
            Rank::Two => 2,
            Rank::Three => 3,
            Rank::Four => 4,
            Rank::Five => 5,
            Rank::Six => 6,
            Rank::Seven => 7,
            Rank::Eight => 8,
            Rank::Nine => 9,
            Rank::Ten | Rank::Jack | Rank::Queen | Rank::King => 10,
            Rank::Ace => 11,
        }
    }
    
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub suit: Suit,
    pub rank: Rank,
    pub face_up: bool,
}

impl Card {
    pub fn new(suit: Suit, rank: Rank, face_up: bool) -> Self {
        Self { suit, rank, face_up }
    }
}

pub struct Deck {
    pub cards: Vec<Card>,
}

impl Deck {
    pub fn new_shuffled() -> Self {
        let suits = [Suit::Hearts, Suit::Diamonds, Suit::Clubs, Suit::Spades];
        let ranks = [
            Rank::Ace, Rank::Two, Rank::Three, Rank::Four, Rank::Five, Rank::Six, Rank::Seven,
            Rank::Eight, Rank::Nine, Rank::Ten, Rank::Jack, Rank::Queen, Rank::King,
        ];

        let mut cards = Vec::with_capacity(52);
        for &suit in &suits {
            for &rank in &ranks {
                cards.push(Card::new(suit, rank, true));
            }
        }

        let mut rng = thread_rng();
        cards.shuffle(&mut rng);

        Self { cards }
    }

    pub fn draw(&mut self, face_up: bool) -> Option<Card> {
        self.cards.pop().map(|mut c| {
            c.face_up = face_up;
            c
        })
    }
}

/// Rysuje kartę wewnątrz podanego prostokąta `rect` – proporcjonalnie skalując wszystkie elementy!
pub fn paint_card_at(painter: &egui::Painter, rect: egui::Rect, card: &Card, highlighted: bool) {
    let scale = (rect.width() / 95.0).max(0.4);
    let rounding = 8.0 * scale;

    // Cień pod kartą
    let shadow_offset = egui::vec2(3.0 * scale, 4.0 * scale);
    painter.rect_filled(
        rect.translate(shadow_offset),
        rounding,
        egui::Color32::from_black_alpha(110),
    );

    if card.face_up {
        let stroke = if highlighted {
            egui::Stroke::new(3.0 * scale, egui::Color32::GOLD)
        } else {
            egui::Stroke::new(1.5 * scale, egui::Color32::from_rgb(180, 180, 180))
        };

        let bg = if highlighted {
            egui::Color32::from_rgb(255, 252, 225)
        } else {
            egui::Color32::from_rgb(248, 248, 242)
        };

        painter.rect(rect, rounding, bg, stroke);

        let color = card.suit.color();
        let pad = egui::vec2(7.0 * scale, 5.0 * scale);
        let corner_font = (16.0 * scale).max(10.0);
        let center_font = (40.0 * scale).max(18.0);

        // Wartość + mały symbol w górnym lewym rogu
        painter.text(
            rect.left_top() + pad,
            egui::Align2::LEFT_TOP,
            format!("{} {}", card.rank.label(), card.suit.symbol()),
            egui::FontId::proportional(corner_font),
            color,
        );

        // Duży symbol na środku
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            card.suit.symbol(),
            egui::FontId::proportional(center_font),
            color,
        );

        // Wartość w prawym dolnym rogu
        painter.text(
            rect.right_bottom() - pad,
            egui::Align2::RIGHT_BOTTOM,
            card.rank.label(),
            egui::FontId::proportional(corner_font),
            color,
        );
    } else {
        // Rewers (zakryta karta)
        painter.rect(
            rect,
            rounding,
            egui::Color32::from_rgb(35, 75, 140),
            egui::Stroke::new(1.5 * scale, egui::Color32::WHITE),
        );

        let inner = rect.shrink(6.0 * scale);
        painter.rect_stroke(
            inner,
            5.0 * scale,
            egui::Stroke::new(1.0 * scale, egui::Color32::from_rgb(120, 165, 230)),
        );
    }
}

/// Rysuje pojedynczą kartę o podanym rozmiarze jako widget (dla Blackjacka)
pub fn draw_card_sized(ui: &mut egui::Ui, card: &Card, size: egui::Vec2) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let offset_y = if response.hovered() { -4.0 } else { 0.0 };
        let draw_rect = rect.translate(egui::vec2(0.0, offset_y));
        paint_card_at(ui.painter(), draw_rect, card, false);

        if response.hovered() {
            ui.ctx().request_repaint();
        }
    }

    response
}