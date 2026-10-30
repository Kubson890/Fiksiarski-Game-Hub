use crate::audio::{Sfx, SoundManager};
use eframe::egui;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

pub const CATEGORY_NAMES: [&str; 5] = ["Państwo", "Miasto", "Zwierzę", "Roślina", "Rzecz"];
const ALLOWED_LETTERS: &[char] = &[
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'R', 'S', 'T',
    'U', 'W', 'Z',
];
const MAX_ROUNDS: u32 = 5;
const COUNTDOWN_SECS: f32 = 10.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CatPhase {
    Lobby,
    Writing,
    Voting,
    RoundSummary,
    GameOver,
}

pub struct CategoriesGame {
    pub phase: CatPhase,
    pub letter: char,
    pub round: u32,
    pub active_players: usize,
    pub my_player_idx: usize,
    pub my_inputs: [String; 5],
    pub submitted: [bool; 4],
    pub all_answers: [[String; 5]; 4],
    pub votes: [[[bool; 5]; 4]; 4],
    pub voted: [bool; 4],
    pub round_points: [[i32; 5]; 4],
    pub total_scores: [i32; 4],
    pub stopper_player: Option<usize>,
    pub countdown_start: Option<Instant>,
    pub my_submitted: bool,
    pub my_voted: bool,
    pub status_text: String,
    pub newly_won: bool,
    pub outgoing_start: Option<(char, u32)>,
    pub outgoing_stop: bool,
    pub outgoing_submit: Option<[String; 5]>,
    pub outgoing_vote: Option<[[bool; 5]; 4]>,
    pub force_broadcast_state: bool,
    last_host_sync: Instant,
}

impl Default for CategoriesGame {
    fn default() -> Self {
        Self {
            phase: CatPhase::Lobby,
            letter: 'A',
            round: 1,
            active_players: 2,
            my_player_idx: 0,
            my_inputs: Default::default(),
            submitted: [false; 4],
            all_answers: Default::default(),
            votes: [[[true; 5]; 4]; 4],
            voted: [false; 4],
            round_points: [[0; 5]; 4],
            total_scores: [0; 4],
            stopper_player: None,
            countdown_start: None,
            my_submitted: false,
            my_voted: false,
            status_text: "Oczekiwanie na rozpoczęcie rundy...".to_string(),
            newly_won: false,
            outgoing_start: None,
            outgoing_stop: false,
            outgoing_submit: None,
            outgoing_vote: None,
            force_broadcast_state: false,
            last_host_sync: Instant::now(),
        }
    }
}

impl CategoriesGame {
    pub fn setup_online(&mut self, my_idx: usize, players_count: usize) {
        self.my_player_idx = my_idx.min(3);
        self.active_players = players_count.clamp(1, 4);
        self.phase = CatPhase::Lobby;
        self.round = 1;
        self.total_scores = [0; 4];
        self.newly_won = false;
        self.clear_round_buffers();
        self.update_lobby_status();
    }

    pub fn update_roster(&mut self, my_idx: usize, players_count: usize) {
        self.my_player_idx = my_idx.min(3);
        self.active_players = players_count.clamp(1, 4);
        if self.phase == CatPhase::Lobby {
            self.update_lobby_status();
        }
    }

    fn update_lobby_status(&mut self) {
        if self.active_players < 2 {
            self.status_text =
                "Czekam na przynajmniej 2 graczy w pokoju, aby rozpocząć...".to_string();
        } else if self.my_player_idx == 0 {
            self.status_text =
                "Jesteś gospodarzem pokoju — kliknij 'Wylosuj literę i rozpocznij Rundę #1!'"
                    .to_string();
        } else {
            self.status_text =
                "Czekam, aż Gracz #1 wylosuje literę i rozpocznie rundę...".to_string();
        }
    }

    fn clear_round_buffers(&mut self) {
        self.my_inputs = Default::default();
        self.submitted = [false; 4];
        self.all_answers = Default::default();
        self.votes = [[[true; 5]; 4]; 4];
        self.voted = [false; 4];
        self.round_points = [[0; 5]; 4];
        self.stopper_player = None;
        self.countdown_start = None;
        self.my_submitted = false;
        self.my_voted = false;
    }

    pub fn host_start_round(&mut self, next_round: u32, audio: &SoundManager) {
        let mut rng = rand::thread_rng();
        let ch = *ALLOWED_LETTERS.choose(&mut rng).unwrap_or(&'A');
        self.apply_start_round(ch, next_round, audio);
        self.outgoing_start = Some((ch, next_round));
        self.force_broadcast_state = true;
        self.last_host_sync = Instant::now();
    }

    pub fn apply_start_round(&mut self, letter: char, round: u32, audio: &SoundManager) {
        audio.play(Sfx::CardSlide);
        if round == 1 {
            self.total_scores = [0; 4];
            self.newly_won = false;
        }
        self.letter = letter.to_ascii_uppercase();
        self.round = round;
        self.phase = CatPhase::Writing;
        self.clear_round_buffers();
        self.status_text = format!(
            "Runda {}/{} — Wpisz słowa na literę '{}'!",
            self.round, MAX_ROUNDS, self.letter
        );
    }

    pub fn apply_remote_stop(&mut self, from_seat: usize, audio: &SoundManager) {
        if self.phase != CatPhase::Writing || self.countdown_start.is_some() {
            return;
        }
        audio.play(Sfx::Check);
        self.stopper_player = Some(from_seat.min(3));
        self.countdown_start = Some(Instant::now());
        self.status_text = format!(
            "Gracz #{} wcisnął STOP! Pozostało 10 sekund na dokończenie!",
            from_seat + 1
        );
    }

    pub fn submit_my_answers(&mut self, audio: &SoundManager) {
        if self.my_submitted || self.my_player_idx >= 4 {
            return;
        }
        audio.play(Sfx::CardPlace);
        let cleaned: [String; 5] = std::array::from_fn(|i| self.my_inputs[i].trim().to_string());
        self.my_submitted = true;
        self.outgoing_submit = Some(cleaned.clone());
        self.apply_remote_submit(self.my_player_idx, cleaned, audio);
        self.force_broadcast_state = true;
    }

    pub fn apply_remote_submit(
        &mut self,
        from_seat: usize,
        answers: [String; 5],
        audio: &SoundManager,
    ) {
        if from_seat >= 4 {
            return;
        }
        self.all_answers[from_seat] = answers;
        self.submitted[from_seat] = true;

        let active = self.active_players.clamp(1, 4);
        if (0..active).all(|p| self.submitted[p]) && self.phase == CatPhase::Writing {
            self.enter_voting_phase(audio);
        }
    }

    fn is_valid_prefix(word: &str, letter: char) -> bool {
        let Some(first) = word.trim().chars().next() else {
            return false;
        };
        first.to_uppercase().next() == letter.to_uppercase().next()
    }

    fn enter_voting_phase(&mut self, audio: &SoundManager) {
        audio.play(Sfx::CardSlide);
        self.phase = CatPhase::Voting;
        self.countdown_start = None;
        self.voted = [false; 4];
        self.my_voted = false;

        for v in 0..4 {
            for target in 0..4 {
                for c in 0..5 {
                    let w = self.all_answers[target][c].trim();
                    let ok = w.chars().count() >= 2 && Self::is_valid_prefix(w, self.letter);
                    self.votes[v][target][c] = ok;
                }
            }
        }

        self.status_text =
            "Faza głosowania! Kliknij na błędne słowa, aby je odrzucić (wymagane min. 3/4 głosów na TAK)."
                .to_string();
    }

    pub fn submit_my_votes(&mut self, audio: &SoundManager) {
        if self.my_voted || self.my_player_idx >= 4 {
            return;
        }
        audio.play(Sfx::Click);
        self.my_voted = true;
        let my_matrix = self.votes[self.my_player_idx];
        self.outgoing_vote = Some(my_matrix);
        self.apply_remote_vote(self.my_player_idx, my_matrix, audio);
        self.force_broadcast_state = true;
    }

    pub fn apply_remote_vote(
        &mut self,
        from_seat: usize,
        matrix: [[bool; 5]; 4],
        audio: &SoundManager,
    ) {
        if from_seat >= 4 {
            return;
        }
        self.votes[from_seat] = matrix;
        self.voted[from_seat] = true;

        let active = self.active_players.clamp(1, 4);
        if (0..active).all(|p| self.voted[p]) && self.phase == CatPhase::Voting {
            self.calculate_scores_and_finish_round(audio);
        }
    }

    pub fn required_yes_votes(&self) -> usize {
        let active = self.active_players.clamp(2, 4);
        match active {
            4 => 3,
            3 => 2,
            _ => 2,
        }
    }

    pub fn yes_votes_for(&self, target_player: usize, cat_idx: usize) -> usize {
        let active = self.active_players.clamp(1, 4);
        (0..active)
            .filter(|&voter| self.votes[voter][target_player][cat_idx])
            .count()
    }

    pub fn is_answer_approved(&self, target_player: usize, cat_idx: usize) -> bool {
        let w = self.all_answers[target_player][cat_idx].trim();
        if w.chars().count() < 2 || !Self::is_valid_prefix(w, self.letter) {
            return false;
        }
        self.yes_votes_for(target_player, cat_idx) >= self.required_yes_votes()
    }

    fn calculate_scores_and_finish_round(&mut self, audio: &SoundManager) {
        let active = self.active_players.clamp(1, 4);
        self.round_points = [[0; 5]; 4];

        for c in 0..5 {
            let mut approved = [false; 4];
            let mut norm_words: [String; 4] = Default::default();
            for p in 0..active {
                if self.is_answer_approved(p, c) {
                    approved[p] = true;
                    norm_words[p] = self.all_answers[p][c].trim().to_lowercase();
                }
            }

            let total_approved = (0..active).filter(|&p| approved[p]).count();
            for p in 0..active {
                if !approved[p] {
                    self.round_points[p][c] = 0;
                } else if total_approved == 1 {
                    self.round_points[p][c] = 15;
                } else {
                    let same_count = (0..active)
                        .filter(|&other| approved[other] && norm_words[other] == norm_words[p])
                        .count();
                    self.round_points[p][c] = if same_count > 1 { 5 } else { 10 };
                }
            }
        }

        for p in 0..active {
            let sum: i32 = self.round_points[p].iter().sum();
            self.total_scores[p] += sum;
        }

        if self.round >= MAX_ROUNDS {
            self.phase = CatPhase::GameOver;
            let max_score = (0..active)
                .map(|p| self.total_scores[p])
                .max()
                .unwrap_or(0);
            let i_won = self.my_player_idx < active
                && self.total_scores[self.my_player_idx] == max_score
                && max_score > 0;
            if i_won {
                self.newly_won = true;
                audio.play(Sfx::Win);
            } else {
                audio.play(Sfx::CardPlace);
            }
            self.status_text = format!(
                "Koniec gry po {} rundach! Najlepszy wynik: {} pkt.",
                MAX_ROUNDS, max_score
            );
        } else {
            self.phase = CatPhase::RoundSummary;
            audio.play(Sfx::Win);
            self.status_text = format!(
                "Podsumowanie rundy {}/{}. Sprawdź przyznane punkty!",
                self.round, MAX_ROUNDS
            );
        }
    }

    pub fn state_json(&self) -> Value {
        json!({
            "cat_sync": true,
            "phase": self.phase,
            "letter": self.letter.to_string(),
            "round": self.round,
            "active_players": self.active_players,
            "submitted": self.submitted,
            "all_answers": self.all_answers,
            "votes": self.votes,
            "voted": self.voted,
            "round_points": self.round_points,
            "total_scores": self.total_scores,
            "stopper_player": self.stopper_player,
            "status": self.status_text,
        })
    }

    pub fn apply_state_json(&mut self, state: &Value, audio: &SoundManager) -> bool {
        let Some(phase_val) = state.get("phase") else {
            return false;
        };
        let Ok(remote_phase) = serde_json::from_value::<CatPhase>(phase_val.clone()) else {
            return false;
        };
        let remote_round = state.get("round").and_then(Value::as_u64).unwrap_or(1) as u32;
        let remote_letter = state
            .get("letter")
            .and_then(Value::as_str)
            .and_then(|s| s.chars().next())
            .unwrap_or('A');

        if (self.phase == CatPhase::Lobby && remote_phase == CatPhase::Writing)
            || (remote_round != self.round && remote_phase == CatPhase::Writing)
        {
            self.apply_start_round(remote_letter, remote_round, audio);
        }

        if let Some(stopper) = state.get("stopper_player").and_then(Value::as_u64) {
            if self.phase == CatPhase::Writing && self.countdown_start.is_none() {
                self.apply_remote_stop(stopper as usize, audio);
            }
        }

        if let Some(sub_val) = state.get("submitted") {
            if let Ok(rem_sub) = serde_json::from_value::<[bool; 4]>(sub_val.clone()) {
                if let Some(ans_val) = state.get("all_answers") {
                    if let Ok(rem_ans) =
                        serde_json::from_value::<[[String; 5]; 4]>(ans_val.clone())
                    {
                        for p in 0..4 {
                            if rem_sub[p] && !self.submitted[p] {
                                self.apply_remote_submit(p, rem_ans[p].clone(), audio);
                            }
                        }
                    }
                }
            }
        }

        if let Some(vd_val) = state.get("voted") {
            if let Ok(rem_voted) = serde_json::from_value::<[bool; 4]>(vd_val.clone()) {
                if let Some(vt_val) = state.get("votes") {
                    if let Ok(rem_votes) =
                        serde_json::from_value::<[[[bool; 5]; 4]; 4]>(vt_val.clone())
                    {
                        for p in 0..4 {
                            if rem_voted[p] && !self.voted[p] {
                                self.apply_remote_vote(p, rem_votes[p], audio);
                            }
                        }
                    }
                }
            }
        }

        true
    }

    fn player_color(p: usize) -> egui::Color32 {
        match p {
            0 => egui::Color32::from_rgb(235, 80, 80),
            1 => egui::Color32::from_rgb(65, 205, 100),
            2 => egui::Color32::from_rgb(245, 195, 50),
            _ => egui::Color32::from_rgb(80, 155, 245),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        let now = Instant::now();

        if self.my_player_idx == 0
            && self.phase != CatPhase::Lobby
            && now.duration_since(self.last_host_sync).as_millis() >= 1200
        {
            self.last_host_sync = now;
            self.force_broadcast_state = true;
        }

        if self.phase == CatPhase::Writing {
            if let Some(start) = self.countdown_start {
                let elapsed = now.duration_since(start).as_secs_f32();
                if elapsed >= COUNTDOWN_SECS {
                    if !self.my_submitted {
                        self.submit_my_answers(audio);
                    }
                } else {
                    ui.ctx().request_repaint();
                }
            }
        }

        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(19, 24, 34));

        let margin = (full_rect.width() * 0.022).clamp(12.0, 26.0);
        let content_rect = full_rect.shrink(margin);
        let scale = (content_rect.width() / 1240.0)
            .min(content_rect.height() / 720.0)
            .clamp(0.75, 1.25);

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child_ui;

        ui.horizontal(|ui| {
            let badge_size = 72.0 * scale;
            let (badge_rect, _) = ui.allocate_exact_size(
                egui::vec2(badge_size, badge_size),
                egui::Sense::hover(),
            );
            ui.painter().circle(
                badge_rect.center(),
                badge_size * 0.46,
                egui::Color32::from_rgb(42, 52, 74),
                egui::Stroke::new(3.0_f32, egui::Color32::GOLD),
            );
            let shown_letter = if self.phase == CatPhase::Lobby {
                '?'
            } else {
                self.letter
            };
            ui.painter().text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                shown_letter.to_string(),
                egui::FontId::proportional(38.0 * scale),
                egui::Color32::GOLD,
            );

            ui.add_space(12.0 * scale);

            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "PAŃSTWA - MIASTA   |   Runda {}/{}   |   Weryfikacja: min. {}/{} głosów na TAK",
                        self.round,
                        MAX_ROUNDS,
                        self.required_yes_votes(),
                        self.active_players.clamp(2, 4)
                    ))
                    .size(18.0 * scale)
                    .strong()
                    .color(egui::Color32::WHITE),
                );
                ui.add_space(4.0 * scale);
                ui.label(
                    egui::RichText::new(&self.status_text)
                        .size(15.0 * scale)
                        .color(egui::Color32::from_rgb(255, 220, 115)),
                );
            });
        });

        ui.add_space(10.0 * scale);

        let avail_w = ui.available_width();
        let gap = 12.0 * scale;
        let box_w = ((avail_w - gap * 3.0) / 4.0).max(120.0);
        let box_h = 54.0 * scale;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for p in 0..4 {
                let active = p < self.active_players;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(box_w, box_h), egui::Sense::hover());
                let bg = if !active {
                    egui::Color32::from_rgb(24, 27, 35)
                } else if p == self.my_player_idx {
                    egui::Color32::from_rgb(38, 52, 76)
                } else {
                    egui::Color32::from_rgb(28, 34, 46)
                };
                let stroke = if p == self.my_player_idx && active {
                    egui::Stroke::new(2.0_f32, Self::player_color(p))
                } else {
                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(60, 68, 86))
                };
                ui.painter().rect(rect, 10.0 * scale, bg, stroke);

                let dot_pos = egui::pos2(rect.left() + 16.0 * scale, rect.center().y);
                ui.painter().circle_filled(
                    dot_pos,
                    6.0 * scale,
                    if active {
                        Self::player_color(p)
                    } else {
                        egui::Color32::DARK_GRAY
                    },
                );

                let title = if !active {
                    format!("Gracz #{} (Wolne)", p + 1)
                } else if p == self.my_player_idx {
                    format!("Gracz #{} (TY)", p + 1)
                } else {
                    format!("Gracz #{}", p + 1)
                };
                ui.painter().text(
                    egui::pos2(rect.left() + 30.0 * scale, rect.center().y - 9.0 * scale),
                    egui::Align2::LEFT_CENTER,
                    title,
                    egui::FontId::proportional(14.5 * scale),
                    if active {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::GRAY
                    },
                );

                if active {
                    let state_badge = match self.phase {
                        CatPhase::Writing => {
                            if self.submitted[p] {
                                "[Gotowy]"
                            } else {
                                "[Pisze...]"
                            }
                        }
                        CatPhase::Voting => {
                            if self.voted[p] {
                                "[Zagłosował]"
                            } else {
                                "[Ocenia...]"
                            }
                        }
                        _ => "",
                    };
                    ui.painter().text(
                        egui::pos2(rect.left() + 30.0 * scale, rect.center().y + 10.0 * scale),
                        egui::Align2::LEFT_CENTER,
                        format!("Punkty: {}   {}", self.total_scores[p], state_badge),
                        egui::FontId::proportional(13.0 * scale),
                        egui::Color32::GOLD,
                    );
                }
            }
        });

        ui.add_space(12.0 * scale);

        match self.phase {
            CatPhase::Lobby => self.render_lobby(ui, scale, audio),
            CatPhase::Writing => self.render_writing(ui, scale, audio),
            CatPhase::Voting => self.render_voting(ui, scale, audio),
            CatPhase::RoundSummary | CatPhase::GameOver => self.render_summary(ui, scale, audio),
        }
    }

    fn render_lobby(&mut self, ui: &mut egui::Ui, scale: f32, audio: &SoundManager) {
        let avail = ui.available_size();
        ui.vertical_centered(|ui| {
            ui.add_space((avail.y * 0.14).max(16.0));
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(28, 34, 48))
                .rounding(16.0 * scale)
                .stroke(egui::Stroke::new(
                    1.5_f32,
                    egui::Color32::from_rgb(75, 90, 120),
                ))
                .inner_margin(28.0 * scale)
                .show(ui, |ui| {
                    ui.set_width((540.0 * scale).min(avail.x * 0.92));
                    ui.heading(
                        egui::RichText::new("Zasady gry Państwa-Miasta Online")
                            .size(22.0 * scale)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                    ui.add_space(10.0 * scale);
                    ui.label(
                        egui::RichText::new(
                            "1. Gospodarz pokoju losuje literę dla wszystkich graczy.\n\
                             2. Kto pierwszy wpisze wszystkie 5 kategorii, może wcisnąć przycisk STOP (uruchamia 10 sekund odliczania dla reszty).\n\
                             3. Brak sztywnego słownika — gracze sami głosują nad poprawnością wpisanych słów (wymagane min. 3/4 głosów na TAK).\n\
                             4. Punktacja: 15 pkt (jedyna odpowiedź w kategorii), 10 pkt (unikalne słowo), 5 pkt (powtórzone słowo).",
                        )
                        .size(14.5 * scale)
                        .color(egui::Color32::LIGHT_GRAY),
                    );
                    ui.add_space(20.0 * scale);

                    let can_start = self.active_players >= 2 && self.my_player_idx == 0;
                    if ui
                        .add_enabled(
                            can_start,
                            egui::Button::new(
                                egui::RichText::new("★ Wylosuj literę i rozpocznij Rundę #1!")
                                    .size(18.0 * scale)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(45, 145, 85))
                            .min_size(egui::vec2(340.0 * scale, 48.0 * scale)),
                        )
                        .clicked()
                    {
                        self.host_start_round(1, audio);
                    }
                });
        });
    }

    fn render_writing(&mut self, ui: &mut egui::Ui, scale: f32, audio: &SoundManager) {
        if let Some(start) = self.countdown_start {
            let elapsed = Instant::now().duration_since(start).as_secs_f32();
            let left = (COUNTDOWN_SECS - elapsed).max(0.0);
            let ratio = (left / COUNTDOWN_SECS).clamp(0.0, 1.0);

            let bar_w = ui.available_width();
            let (bar_rect, _) =
                ui.allocate_exact_size(egui::vec2(bar_w, 26.0 * scale), egui::Sense::hover());
            ui.painter()
                .rect_filled(bar_rect, 8.0, egui::Color32::from_rgb(60, 25, 25));
            let fill_rect = egui::Rect::from_min_size(
                bar_rect.min,
                egui::vec2(bar_w * ratio, 26.0 * scale),
            );
            ui.painter()
                .rect_filled(fill_rect, 8.0, egui::Color32::from_rgb(225, 60, 60));
            ui.painter().text(
                bar_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("⏱ ALARM STOP! Pozostało {:.1} s na wpisanie słów!", left),
                egui::FontId::proportional(14.5 * scale),
                egui::Color32::WHITE,
            );
            ui.add_space(10.0 * scale);
        }

        let outer_w = ui.available_width();
        let pad = 20.0 * scale;

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(27, 33, 46))
            .rounding(14.0 * scale)
            .stroke(egui::Stroke::new(
                1.5_f32,
                egui::Color32::from_rgb(65, 78, 102),
            ))
            .inner_margin(pad)
            .show(ui, |ui| {
                ui.set_max_width((outer_w - pad * 2.0).max(320.0));
                let mut valid_count = 0usize;

                for (idx, &cat_name) in CATEGORY_NAMES.iter().enumerate() {
                    let is_ok = self.my_inputs[idx].trim().chars().count() >= 2
                        && Self::is_valid_prefix(&self.my_inputs[idx], self.letter);
                    if is_ok {
                        valid_count += 1;
                    }

                    ui.horizontal(|ui| {
                        let (dot_rect, _) = ui.allocate_exact_size(
                            egui::vec2(22.0 * scale, 28.0 * scale),
                            egui::Sense::hover(),
                        );
                        ui.painter().circle_filled(
                            dot_rect.center(),
                            6.5 * scale,
                            if is_ok {
                                egui::Color32::from_rgb(75, 225, 115)
                            } else {
                                egui::Color32::from_rgb(85, 95, 115)
                            },
                        );

                        ui.add_sized(
                            [130.0 * scale, 32.0 * scale],
                            egui::Label::new(
                                egui::RichText::new(format!("{}:", cat_name))
                                    .size(16.5 * scale)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            ),
                        );

                        let input_w = (ui.available_width() - 8.0).max(140.0);
                        let edit = egui::TextEdit::singleline(&mut self.my_inputs[idx])
                            .hint_text(format!("Na literę {}...", self.letter))
                            .font(egui::FontId::proportional(16.5 * scale))
                            .desired_width(input_w);
                        ui.add_enabled(!self.my_submitted, edit);
                    });
                    ui.add_space(8.0 * scale);
                }

                ui.add_space(10.0 * scale);

                ui.horizontal_wrapped(|ui| {
                    let all_filled = valid_count == 5;
                    let can_stop =
                        all_filled && !self.my_submitted && self.countdown_start.is_none();

                    if ui
                        .add_enabled(
                            can_stop,
                            egui::Button::new(
                                egui::RichText::new("⏱ STOP! Mam wszystkie 5 (odliczaj 10s)")
                                    .size(16.0 * scale)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(205, 55, 55))
                            .min_size(egui::vec2(300.0 * scale, 42.0 * scale)),
                        )
                        .clicked()
                    {
                        self.outgoing_stop = true;
                        self.apply_remote_stop(self.my_player_idx, audio);
                        self.submit_my_answers(audio);
                    }

                    ui.add_space(10.0 * scale);

                    if ui
                        .add_enabled(
                            !self.my_submitted,
                            egui::Button::new(
                                egui::RichText::new(if self.my_submitted {
                                    "Odpowiedzi wysłane (czekam na resztę...)"
                                } else {
                                    "Wyślij swoje odpowiedzi"
                                })
                                .size(15.5 * scale)
                                .strong(),
                            )
                            .min_size(egui::vec2(250.0 * scale, 42.0 * scale)),
                        )
                        .clicked()
                    {
                        self.submit_my_answers(audio);
                    }

                    ui.add_space(10.0 * scale);
                    ui.label(
                        egui::RichText::new(format!("Poprawnie wypełnione: {}/5", valid_count))
                            .size(15.0 * scale)
                            .color(if all_filled {
                                egui::Color32::from_rgb(90, 235, 125)
                            } else {
                                egui::Color32::LIGHT_GRAY
                            }),
                    );
                });
            });
    }

    fn render_voting(&mut self, ui: &mut egui::Ui, scale: f32, audio: &SoundManager) {
        let active = self.active_players.clamp(2, 4);
        let req_votes = self.required_yes_votes();
        let outer_w = ui.available_width();
        let pad = 16.0 * scale;

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(26, 32, 44))
            .rounding(14.0 * scale)
            .stroke(egui::Stroke::new(
                1.5_f32,
                egui::Color32::from_rgb(70, 84, 110),
            ))
            .inner_margin(pad)
            .show(ui, |ui| {
                let inner_w = (outer_w - pad * 2.0 - 8.0).max(320.0);
                ui.set_max_width(inner_w);

                ui.label(
                    egui::RichText::new(format!(
                        "Kliknij na odpowiedź, aby zmienić swój głos ([TAK] / [NIE]). Słowo zostaje uznane przy min. {}/{} głosach na TAK:",
                        req_votes, active
                    ))
                    .size(14.5 * scale)
                    .color(egui::Color32::LIGHT_GRAY),
                );
                ui.add_space(10.0 * scale);

                let cat_col_w = (125.0 * scale).clamp(95.0, 155.0);
                let col_gap = 12.0 * scale;
                let total_gaps = col_gap * active as f32;
                let player_col_w =
                    ((inner_w - cat_col_w - total_gaps) / active as f32).max(105.0);

                egui::Grid::new("categories_voting_grid")
                    .striped(true)
                    .spacing([col_gap, 10.0 * scale])
                    .show(ui, |ui| {
                        ui.add_sized(
                            [cat_col_w, 26.0 * scale],
                            egui::Label::new(
                                egui::RichText::new("Kategoria")
                                    .size(15.5 * scale)
                                    .strong()
                                    .color(egui::Color32::GOLD),
                            ),
                        );
                        for p in 0..active {
                            ui.add_sized(
                                [player_col_w, 26.0 * scale],
                                egui::Label::new(
                                    egui::RichText::new(if p == self.my_player_idx {
                                        format!("Gracz #{} (TY)", p + 1)
                                    } else {
                                        format!("Gracz #{}", p + 1)
                                    })
                                    .size(15.5 * scale)
                                    .strong()
                                    .color(Self::player_color(p)),
                                ),
                            );
                        }
                        ui.end_row();

                        for (c_idx, &cat_name) in CATEGORY_NAMES.iter().enumerate() {
                            ui.add_sized(
                                [cat_col_w, 34.0 * scale],
                                egui::Label::new(
                                    egui::RichText::new(cat_name)
                                        .size(15.5 * scale)
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                ),
                            );

                            for p in 0..active {
                                let word = self.all_answers[p][c_idx].trim();
                                let valid_letter = word.chars().count() >= 2
                                    && Self::is_valid_prefix(word, self.letter);

                                if !valid_letter {
                                    ui.add_sized(
                                        [player_col_w, 34.0 * scale],
                                        egui::Label::new(
                                            egui::RichText::new(if word.is_empty() {
                                                "— (brak) —".to_string()
                                            } else {
                                                format!("{} (zła litera)", word)
                                            })
                                            .size(14.0 * scale)
                                            .color(egui::Color32::GRAY),
                                        ),
                                    );
                                } else {
                                    let my_vote = self.votes[self.my_player_idx][p][c_idx];
                                    let btn_label = if my_vote {
                                        format!("[TAK] {}", word)
                                    } else {
                                        format!("[NIE] {}", word)
                                    };
                                    let btn_fill = if my_vote {
                                        egui::Color32::from_rgb(35, 115, 62)
                                    } else {
                                        egui::Color32::from_rgb(135, 42, 42)
                                    };

                                    let btn = egui::Button::new(
                                        egui::RichText::new(btn_label)
                                            .size(14.5 * scale)
                                            .color(egui::Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(btn_fill)
                                    .min_size(egui::vec2(player_col_w, 34.0 * scale));

                                    if ui.add_enabled(!self.my_voted, btn).clicked() {
                                        audio.play(Sfx::Click);
                                        self.votes[self.my_player_idx][p][c_idx] = !my_vote;
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });

                ui.add_space(16.0 * scale);

                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.my_voted,
                            egui::Button::new(
                                egui::RichText::new(if self.my_voted {
                                    "Głosy zatwierdzone — czekam na pozostałych graczy..."
                                } else {
                                    "Zatwierdź moje głosy i podlicz punkty"
                                })
                                .size(16.0 * scale)
                                .strong()
                                .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(50, 135, 220))
                            .min_size(egui::vec2(340.0 * scale, 42.0 * scale)),
                        )
                        .clicked()
                    {
                        self.submit_my_votes(audio);
                    }
                });
            });
    }

    fn render_summary(&mut self, ui: &mut egui::Ui, scale: f32, audio: &SoundManager) {
        let active = self.active_players.clamp(2, 4);
        let req_votes = self.required_yes_votes();
        let outer_w = ui.available_width();
        let pad = 16.0 * scale;

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(26, 32, 44))
            .rounding(14.0 * scale)
            .stroke(egui::Stroke::new(1.5_f32, egui::Color32::GOLD))
            .inner_margin(pad)
            .show(ui, |ui| {
                let inner_w = (outer_w - pad * 2.0 - 8.0).max(320.0);
                ui.set_max_width(inner_w);

                let cat_col_w = (125.0 * scale).clamp(95.0, 155.0);
                let col_gap = 12.0 * scale;
                let total_gaps = col_gap * active as f32;
                let player_col_w =
                    ((inner_w - cat_col_w - total_gaps) / active as f32).max(105.0);

                egui::Grid::new("categories_summary_grid")
                    .striped(true)
                    .spacing([col_gap, 10.0 * scale])
                    .show(ui, |ui| {
                        ui.add_sized(
                            [cat_col_w, 26.0 * scale],
                            egui::Label::new(
                                egui::RichText::new("Kategoria")
                                    .size(15.5 * scale)
                                    .strong()
                                    .color(egui::Color32::GOLD),
                            ),
                        );
                        for p in 0..active {
                            ui.add_sized(
                                [player_col_w, 26.0 * scale],
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "Gracz #{} ({} pkt)",
                                        p + 1,
                                        self.total_scores[p]
                                    ))
                                    .size(15.5 * scale)
                                    .strong()
                                    .color(Self::player_color(p)),
                                ),
                            );
                        }
                        ui.end_row();

                        for (c_idx, &cat_name) in CATEGORY_NAMES.iter().enumerate() {
                            ui.add_sized(
                                [cat_col_w, 30.0 * scale],
                                egui::Label::new(
                                    egui::RichText::new(cat_name)
                                        .size(15.5 * scale)
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                ),
                            );

                            for p in 0..active {
                                let word = self.all_answers[p][c_idx].trim();
                                let pts = self.round_points[p][c_idx];
                                let yes_cnt = self.yes_votes_for(p, c_idx);

                                let txt = if word.is_empty() {
                                    "— (0 pkt)".to_string()
                                } else {
                                    format!("{} (+{} pkt | {}/{})", word, pts, yes_cnt, active)
                                };
                                let col = if pts >= 10 {
                                    egui::Color32::from_rgb(90, 240, 130)
                                } else if pts == 5 {
                                    egui::Color32::GOLD
                                } else if yes_cnt < req_votes {
                                    egui::Color32::from_rgb(235, 95, 95)
                                } else {
                                    egui::Color32::GRAY
                                };

                                ui.add_sized(
                                    [player_col_w, 30.0 * scale],
                                    egui::Label::new(
                                        egui::RichText::new(txt).size(14.5 * scale).color(col),
                                    ),
                                );
                            }
                            ui.end_row();
                        }
                    });

                ui.add_space(16.0 * scale);

                if self.my_player_idx == 0 {
                    let next_round = if self.phase == CatPhase::GameOver {
                        1
                    } else {
                        self.round + 1
                    };
                    let btn_label = if self.phase == CatPhase::GameOver {
                        "↻ Zagraj nowy mecz (od Rundy #1)".to_string()
                    } else {
                        format!(
                            "➡ Wylosuj literę i rozpocznij Rundę #{}/{}",
                            next_round, MAX_ROUNDS
                        )
                    };

                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(btn_label)
                                    .size(16.5 * scale)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(45, 145, 85))
                            .min_size(egui::vec2(340.0 * scale, 42.0 * scale)),
                        )
                        .clicked()
                    {
                        self.host_start_round(next_round, audio);
                    }
                } else {
                    ui.label(
                        egui::RichText::new("Czekam aż Gracz #1 rozpocznie kolejną rundę...")
                            .size(15.0 * scale)
                            .color(egui::Color32::LIGHT_GRAY),
                    );
                }
            });
    }
}