use crate::audio::{Sfx, SoundManager};
use eframe::egui;
use rand::Rng;
use serde_json::{json, Value};
use std::time::Instant;

const FLEET_SIZES: [(usize, &str); 5] = [
    (5, "Lotniskowiec (5)"),
    (4, "Pancernik (4)"),
    (3, "Krążownik (3)"),
    (3, "Okręt podwodny (3)"),
    (2, "Niszczyciel (2)"),
];

#[derive(Clone, Copy)]
pub struct Ship {
    pub x: usize,
    pub y: usize,
    pub len: usize,
    pub horizontal: bool,
}

impl Ship {
    pub fn cells(&self) -> Vec<(usize, usize)> {
        (0..self.len)
            .map(|i| {
                if self.horizontal {
                    (self.x + i, self.y)
                } else {
                    (self.x, self.y + i)
                }
            })
            .collect()
    }

    pub fn is_sunk(&self, shots: &[[bool; 10]; 10]) -> bool {
        self.cells().into_iter().all(|(cx, cy)| shots[cx][cy])
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Phase {
    Placement,
    Battle,
    GameOver { player_won: bool },
}

struct BlastAnim {
    is_enemy_board: bool,
    gx: usize,
    gy: usize,
    is_hit: bool,
    start: Instant,
}

pub struct BattleshipGame {
    phase: Phase,
    player_ships: Vec<Ship>,
    bot_ships: Vec<Ship>,
    player_shots: [[bool; 10]; 10],
    bot_shots: [[bool; 10]; 10],
    placing_idx: usize,
    place_horizontal: bool,
    bot_targets: Vec<(usize, usize)>,
    blast: Option<BlastAnim>,
    status_text: String,
    pub newly_won: bool,
    pub online_mode: bool,
    pub my_turn_online: bool,
    pub outgoing_fleet: Option<Vec<(usize, usize, usize, bool)>>,
    pub outgoing_shot: Option<(usize, usize)>,
    /// Miejsce w kolejce pokoju (0/1 gra, 2/3 obserwuje).
    pub my_seat: usize,
    /// Gracz z miejsca 3/4 tylko obserwuje bitwę.
    pub spectator: bool,
    /// Czeka na drugiego gracza ze swojego stolika — plansza jest zablokowana.
    pub waiting: bool,
    /// Ilu obserwatorów aktualnie patrzy na bitwę (do ikony 👁).
    pub spectators: usize,
    /// W trybie obserwatora: czyja kolej (0 = lewy gracz, 1 = prawy gracz).
    spectator_turn: usize,
    /// Czy obserwator dostał już pełny stan plansz.
    state_ready: bool,
}

impl Default for BattleshipGame {
    fn default() -> Self {
        let mut g = Self {
            phase: Phase::Placement,
            player_ships: Vec::new(),
            bot_ships: Vec::new(),
            player_shots: [[false; 10]; 10],
            bot_shots: [[false; 10]; 10],
            placing_idx: 0,
            place_horizontal: true,
            bot_targets: Vec::new(),
            blast: None,
            status_text: String::new(),
            newly_won: false,
            online_mode: false,
            my_turn_online: true,
            outgoing_fleet: None,
            outgoing_shot: None,
            my_seat: 0,
            spectator: false,
            waiting: false,
            spectators: 0,
            spectator_turn: 0,
            state_ready: true,
        };
        g.new_game();
        g
    }
}

impl BattleshipGame {
    pub fn new_game(&mut self) {
        self.phase = Phase::Placement;
        self.player_ships.clear();
        if !self.online_mode {
            self.bot_ships = Self::generate_random_fleet();
        } else {
            self.bot_ships.clear();
        }
        self.player_shots = [[false; 10]; 10];
        self.bot_shots = [[false; 10]; 10];
        self.placing_idx = 0;
        self.place_horizontal = true;
        self.bot_targets.clear();
        self.blast = None;
        self.status_text = format!(
            "Ustaw swój okręt: {} (PPM lub klawisz R obraca)",
            FLEET_SIZES[0].1
        );
        self.newly_won = false;
        self.outgoing_fleet = None;
        self.outgoing_shot = None;
    }

    /// Powrót do gry lokalnej — czyści tryb obserwatora.
    pub fn leave_online(&mut self) {
        self.online_mode = false;
        self.spectator = false;
        self.waiting = false;
        self.state_ready = true;
    }

    pub fn setup_online(&mut self, player_idx: usize) {
        self.online_mode = true;
        // `player_idx` to miejsce w obrębie stolika (0/1).
        self.my_seat = player_idx.min(1);
        self.spectator = false;
        self.waiting = false;
        self.my_turn_online = self.my_seat == 0;
        self.spectator_turn = 0;
        self.state_ready = true;
        self.new_game();
    }

    /// Ustawia rolę przy stole: gracz / obserwator / oczekujący na partnera.
    pub fn set_online_role(&mut self, spectator: bool, waiting: bool) {
        let changed = spectator != self.spectator || waiting != self.waiting;
        self.spectator = spectator;
        self.waiting = waiting;
        if spectator || waiting {
            self.outgoing_fleet = None;
            self.outgoing_shot = None;
        }
        if changed {
            if waiting {
                self.status_text =
                    "⏳ Czekam na drugiego gracza ze stolika...".to_string();
            } else if spectator {
                self.status_text =
                    "👁 Obserwujesz bitwę — czekam na synchronizację plansz...".to_string();
            } else {
                self.status_text = match self.phase {
                    Phase::Placement => "🚢 Rozmieszczanie flot...".to_string(),
                    Phase::Battle => {
                        if self.my_turn_online {
                            "🎯 TWOJA KOLEJ na strzał!".to_string()
                        } else {
                            "⏳ Czekaj na ruch przeciwnika...".to_string()
                        }
                    }
                    Phase::GameOver { player_won: true } => {
                        "🏆 ZWYCIĘSTWO!".to_string()
                    }
                    Phase::GameOver { player_won: false } => {
                        "💀 PORAŻKA!".to_string()
                    }
                };
            }
        }
    }

    pub fn receive_enemy_fleet(
        &mut self,
        ships_data: Vec<(usize, usize, usize, bool)>,
        from_seat: usize,
    ) {
        let ships: Vec<Ship> = ships_data
            .into_iter()
            .map(|(x, y, len, horizontal)| Ship {
                x,
                y,
                len,
                horizontal,
            })
            .collect();

        if self.spectator {
            match from_seat {
                0 => self.player_ships = ships,
                1 => self.bot_ships = ships,
                _ => {}
            }
            // Gdy obie floty są już znane, rozmieszczanie się skończyło.
            if self.phase == Phase::Placement
                && !self.player_ships.is_empty()
                && !self.bot_ships.is_empty()
            {
                self.phase = Phase::Battle;
                self.state_ready = true;
                self.status_text = "🎯 Obie floty gotowe — bitwa się rozpoczęła!".to_string();
            }
        } else {
            self.bot_ships = ships;
            if self.online_mode && self.phase == Phase::Battle {
                self.status_text = if self.my_turn_online {
                    "🎯 Flota przeciwnika gotowa — TWOJA KOLEJ na strzał!".to_string()
                } else {
                    "🎯 Flota przeciwnika gotowa — czekaj na swój ruch.".to_string()
                };
            }
        }
    }

    pub fn receive_enemy_shot(
        &mut self,
        x: usize,
        y: usize,
        from_seat: usize,
        audio: &SoundManager,
    ) {
        if x >= 10 || y >= 10 {
            return;
        }
        if self.spectator {
            self.apply_spectator_shot(from_seat, x, y, audio);
            return;
        }
        self.bot_shots[x][y] = true;
        let is_hit = Self::has_ship_at(&self.player_ships, x, y);
        self.blast = Some(BlastAnim {
            is_enemy_board: false,
            gx: x,
            gy: y,
            is_hit,
            start: Instant::now(),
        });

        if is_hit {
            audio.play(Sfx::ChessCap);
            for ship in &self.player_ships {
                if ship.cells().contains(&(x, y)) && ship.is_sunk(&self.bot_shots) {
                    Self::mark_halo_around_sunk(ship, &mut self.bot_shots);
                }
            }
            if self
                .player_ships
                .iter()
                .all(|s| s.is_sunk(&self.bot_shots))
            {
                audio.play(Sfx::Lose);
                self.phase = Phase::GameOver { player_won: false };
                self.status_text = "💀 PORAŻKA! Przeciwnik zatopił Twoją flotę!".to_string();
            } else {
                self.status_text =
                    "💥 Przeciwnik trafił Twój okręt i strzela ponownie!".to_string();
            }
        } else {
            audio.play(Sfx::CardPlace);
            self.my_turn_online = true;
            self.status_text = "💧 Przeciwnik spudłował! TWOJA KOLEJ na strzał!".to_string();
        }
    }

    fn apply_spectator_shot(
        &mut self,
        from_seat: usize,
        x: usize,
        y: usize,
        audio: &SoundManager,
    ) {
        if from_seat > 1 || matches!(self.phase, Phase::GameOver { .. }) {
            return;
        }
        // Obserwator mógł wejść w trakcie rozmieszczania flot — pierwszy
        // strzał oznacza, że bitwa już się zaczęła.
        if self.phase == Phase::Placement {
            self.phase = Phase::Battle;
            self.state_ready = true;
        }
        let (target_ships, target_shots) = if from_seat == 0 {
            (&self.bot_ships, &mut self.player_shots)
        } else {
            (&self.player_ships, &mut self.bot_shots)
        };
        if target_shots[x][y] {
            return;
        }
        target_shots[x][y] = true;
        let is_hit = Self::has_ship_at(target_ships, x, y);

        if is_hit {
            audio.play(Sfx::ChessCap);
            let mut sunk_now = false;
            for ship in target_ships {
                if ship.cells().contains(&(x, y)) && ship.is_sunk(target_shots) {
                    Self::mark_halo_around_sunk(ship, target_shots);
                    sunk_now = true;
                }
            }
            if target_ships.iter().all(|s| s.is_sunk(target_shots)) {
                audio.play(Sfx::Win);
                self.phase = Phase::GameOver {
                    player_won: from_seat == 0,
                };
                self.status_text = format!(
                    "🏆 KONIEC! Gracz {} zatopił całą flotę przeciwnika!",
                    from_seat + 1
                );
            } else if sunk_now {
                audio.play(Sfx::Check);
                self.status_text = format!("🔥 Gracz {} zatopił okręt!", from_seat + 1);
            } else {
                self.status_text = format!("💥 Gracz {} trafił i strzela dalej!", from_seat + 1);
            }
            self.spectator_turn = from_seat;
        } else {
            audio.play(Sfx::CardPlace);
            self.status_text = format!("💧 Gracz {} spudłował.", from_seat + 1);
            self.spectator_turn = 1 - from_seat;
        }
    }

    /// Pełny stan obu plansz — wysyłany obserwatorowi lub graczowi,
    /// który dopiero wchodzi na ekran Statków.
    pub fn state_json(&self, from_seat: usize) -> Value {
        let swapped = from_seat == 1;
        let (seat0_ships, seat1_ships, shots_by_0, shots_by_1) = if swapped {
            (&self.bot_ships, &self.player_ships, &self.bot_shots, &self.player_shots)
        } else {
            (&self.player_ships, &self.bot_ships, &self.player_shots, &self.bot_shots)
        };
        let turn = if swapped {
            if self.my_turn_online {
                1
            } else {
                0
            }
        } else if self.my_turn_online {
            0
        } else {
            1
        };
        let phase = match self.phase {
            Phase::Placement => "placement",
            Phase::Battle => "battle",
            Phase::GameOver { .. } => "over",
        };
        let winner = match self.phase {
            Phase::GameOver { player_won } => Some(if swapped {
                if player_won {
                    1
                } else {
                    0
                }
            } else if player_won {
                0
            } else {
                1
            }),
            _ => None,
        };
        // Status neutralny wobec perspektywy gracza — stan wysyłany jest
        // zarówno drugiemu graczowi, jak i obserwatorom.
        let status = match (phase, winner) {
            ("over", Some(winner)) => format!("🏆 KONIEC! Wygrywa Gracz {}!", winner + 1),
            ("battle", _) => format!("🎯 Tura: Gracz {}", turn + 1),
            ("placement", _) => "🚢 Rozmieszczanie flot...".to_string(),
            _ => self.status_text.clone(),
        };
        let ships = |fleet: &[Ship]| {
            fleet
                .iter()
                .map(|s| json!([s.x, s.y, s.len, s.horizontal]))
                .collect::<Vec<_>>()
        };
        json!({
            "phase": phase,
            "winner": winner,
            "seat0_ships": ships(seat0_ships),
            "seat1_ships": ships(seat1_ships),
            "shots_by_0": shots_by_0,
            "shots_by_1": shots_by_1,
            "turn": turn,
            "status": status,
        })
    }

    pub fn apply_state_json(&mut self, state: &Value) -> bool {
        let parse_ships = |key: &str| -> Vec<Ship> {
            state
                .get(key)
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| {
                            let arr = row.as_array()?;
                            Some(Ship {
                                x: arr.first()?.as_u64()? as usize,
                                y: arr.get(1)?.as_u64()? as usize,
                                len: arr.get(2)?.as_u64()? as usize,
                                horizontal: arr.get(3)?.as_bool()?,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let parse_shots = |key: &str| -> [[bool; 10]; 10] {
            let mut shots = [[false; 10]; 10];
            if let Some(rows) = state.get(key).and_then(Value::as_array) {
                for (x, row) in rows.iter().take(10).enumerate() {
                    if let Some(cols) = row.as_array() {
                        for (y, cell) in cols.iter().take(10).enumerate() {
                            shots[x][y] = cell.as_bool().unwrap_or(false);
                        }
                    }
                }
            }
            shots
        };

        let seat0_ships = parse_ships("seat0_ships");
        let seat1_ships = parse_ships("seat1_ships");
        let shots_by_0 = parse_shots("shots_by_0");
        let shots_by_1 = parse_shots("shots_by_1");
        let turn = state.get("turn").and_then(Value::as_u64).unwrap_or(0) as usize;

        if self.spectator {
            self.player_ships = seat0_ships;
            self.bot_ships = seat1_ships;
            self.player_shots = shots_by_0;
            self.bot_shots = shots_by_1;
            self.spectator_turn = turn;
        } else if self.my_seat == 1 {
            self.player_ships = seat1_ships;
            self.bot_ships = seat0_ships;
            self.player_shots = shots_by_1;
            self.bot_shots = shots_by_0;
            self.my_turn_online = turn == 1;
        } else {
            self.player_ships = seat0_ships;
            self.bot_ships = seat1_ships;
            self.player_shots = shots_by_0;
            self.bot_shots = shots_by_1;
            self.my_turn_online = turn == 0;
        }

        let was_battle = self.phase == Phase::Battle;
        self.phase = match state.get("phase").and_then(Value::as_str).unwrap_or("battle") {
            "placement" if self.spectator && was_battle => Phase::Battle,
            "placement" => Phase::Placement,
            "over" => {
                let winner = state.get("winner").and_then(Value::as_u64).map(|w| w as usize);
                let local_won = match winner {
                    Some(winner) if self.spectator => winner == 0,
                    Some(winner) => winner == self.my_seat,
                    None => false,
                };
                Phase::GameOver {
                    player_won: local_won,
                }
            }
            _ => Phase::Battle,
        };
        self.status_text = state
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        self.blast = None;
        self.bot_targets.clear();
        self.outgoing_fleet = None;
        self.outgoing_shot = None;
        self.state_ready = true;
        if self.spectator {
            self.status_text = format!("👁 Obserwujesz | {}", self.status_text);
        }
        true
    }

    fn can_place_ship(existing: &[Ship], candidate: Ship) -> bool {
        for (cx, cy) in candidate.cells() {
            if cx >= 10 || cy >= 10 {
                return false;
            }
            for ship in existing {
                for (sx, sy) in ship.cells() {
                    if (cx as i32 - sx as i32).abs() <= 1 && (cy as i32 - sy as i32).abs() <= 1 {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn generate_random_fleet() -> Vec<Ship> {
        let mut rng = rand::thread_rng();
        let mut fleet = Vec::with_capacity(5);

        for &(len, _) in &FLEET_SIZES {
            loop {
                let horizontal = rng.gen_bool(0.5);
                let x = rng.gen_range(0..10);
                let y = rng.gen_range(0..10);
                let cand = Ship {
                    x,
                    y,
                    len,
                    horizontal,
                };
                if Self::can_place_ship(&fleet, cand) {
                    fleet.push(cand);
                    break;
                }
            }
        }
        fleet
    }

    fn has_ship_at(ships: &[Ship], x: usize, y: usize) -> bool {
        ships
            .iter()
            .any(|s| s.cells().into_iter().any(|(sx, sy)| sx == x && sy == y))
    }

    fn mark_halo_around_sunk(ship: &Ship, shots: &mut [[bool; 10]; 10]) {
        for (cx, cy) in ship.cells() {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let nx = cx as i32 + dx;
                    let ny = cy as i32 + dy;
                    if (0..10).contains(&nx) && (0..10).contains(&ny) {
                        shots[nx as usize][ny as usize] = true;
                    }
                }
            }
        }
    }

    fn fire_at_bot(&mut self, x: usize, y: usize, audio: &SoundManager) {
        if self.spectator
            || self.waiting
            || self.phase != Phase::Battle
            // Nie strzelamy, zanim nie dotrze flota przeciwnika — inaczej
            // pierwsze strzały byłyby fałszywymi pudłami.
            || (self.online_mode && self.bot_ships.is_empty())
            || self.player_shots[x][y]
            || (self.online_mode && !self.my_turn_online)
        {
            return;
        }

        if self.online_mode {
            self.outgoing_shot = Some((x, y));
        }

        self.player_shots[x][y] = true;
        let is_hit = Self::has_ship_at(&self.bot_ships, x, y);

        self.blast = Some(BlastAnim {
            is_enemy_board: true,
            gx: x,
            gy: y,
            is_hit,
            start: Instant::now(),
        });

        if is_hit {
            let mut sunk_now = false;
            for ship in &self.bot_ships {
                if ship.cells().contains(&(x, y)) && ship.is_sunk(&self.player_shots) {
                    Self::mark_halo_around_sunk(ship, &mut self.player_shots);
                    sunk_now = true;
                }
            }

            if self
                .bot_ships
                .iter()
                .all(|s| s.is_sunk(&self.player_shots))
            {
                audio.play(Sfx::Win);
                self.phase = Phase::GameOver { player_won: true };
                self.status_text =
                    "🏆 ZWYCIĘSTWO! Zatopiłeś całą flotę przeciwnika!".to_string();
                self.newly_won = true;
                return;
            }

            if sunk_now {
                audio.play(Sfx::Check);
                self.status_text =
                    "🔥 TRAFIONY ZATOPIONY! Masz kolejny strzał!".to_string();
            } else {
                audio.play(Sfx::ChessCap);
                self.status_text = "💥 TRAFIONY! Strzelaj jeszcze raz!".to_string();
            }
        } else {
            audio.play(Sfx::CardPlace);
            if self.online_mode {
                self.my_turn_online = false;
                self.status_text = "💧 Pudło! Czekaj na strzał przeciwnika...".to_string();
            } else {
                self.bot_turn(audio);
            }
        }
    }

    fn bot_turn(&mut self, audio: &SoundManager) {
        let mut rng = rand::thread_rng();

        loop {
            let target = loop {
                if let Some((tx, ty)) = self.bot_targets.pop() {
                    if !self.bot_shots[tx][ty] {
                        break (tx, ty);
                    }
                } else {
                    let rx = rng.gen_range(0..10);
                    let ry = rng.gen_range(0..10);
                    if !self.bot_shots[rx][ry] {
                        break (rx, ry);
                    }
                }
            };

            let (bx, by) = target;
            self.bot_shots[bx][by] = true;
            let is_hit = Self::has_ship_at(&self.player_ships, bx, by);

            if is_hit {
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let nx = bx as i32 + dx;
                    let ny = by as i32 + dy;
                    if (0..10).contains(&nx) && (0..10).contains(&ny) {
                        let ux = nx as usize;
                        let uy = ny as usize;
                        if !self.bot_shots[ux][uy] {
                            self.bot_targets.push((ux, uy));
                        }
                    }
                }

                for ship in &self.player_ships {
                    if ship.cells().contains(&(bx, by)) && ship.is_sunk(&self.bot_shots) {
                        Self::mark_halo_around_sunk(ship, &mut self.bot_shots);
                        self.bot_targets.clear();
                    }
                }

                if self
                    .player_ships
                    .iter()
                    .all(|s| s.is_sunk(&self.bot_shots))
                {
                    audio.play(Sfx::Lose);
                    self.phase = Phase::GameOver { player_won: false };
                    self.status_text =
                        "💀 PORAŻKA! Wróg zatopił wszystkie Twoje okręty!".to_string();
                    return;
                }
            } else {
                self.status_text =
                    "💧 Pudło! Wróg odpowiedział ogniem — Twoja kolej na strzał.".to_string();
                break;
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, audio: &SoundManager) {
        if self.spectator {
            self.spectator_ui(ui);
            return;
        }

        let now = Instant::now();

        if self.phase == Phase::Placement && ui.input(|i| i.key_pressed(egui::Key::R)) {
            audio.play(Sfx::Click);
            self.place_horizontal = !self.place_horizontal;
        }

        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(16, 28, 42));

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
                    .button(egui::RichText::new("🔄 Nowa bitwa morska").size(16.0))
                    .clicked()
                {
                    audio.play(Sfx::Click);
                    self.new_game();
                }
                ui.separator();
            }

            if self.phase == Phase::Placement {
                if ui
                    .button(egui::RichText::new("🔃 Obróć okręt (R / PPM)").size(16.0))
                    .clicked()
                {
                    audio.play(Sfx::Click);
                    self.place_horizontal = !self.place_horizontal;
                }

                let can_random = (!self.online_mode || self.bot_ships.is_empty())
                    && !self.waiting;
                let rand_btn = ui.add_enabled(
                    can_random,
                    egui::Button::new(egui::RichText::new("🎲 Losuj układ floty i graj!").size(16.0)),
                );
                if rand_btn.clicked() {
                    audio.play(Sfx::CardSlide);
                    self.player_ships = Self::generate_random_fleet();
                    self.phase = Phase::Battle;
                    if self.online_mode {
                        self.outgoing_fleet = Some(
                            self.player_ships
                                .iter()
                                .map(|s| (s.x, s.y, s.len, s.horizontal))
                                .collect(),
                        );
                        self.status_text = if self.bot_ships.is_empty() {
                            "⏳ Flota gotowa! Czekam na rozmieszczenie przeciwnika...".to_string()
                        } else if self.my_turn_online {
                            "🎯 Flota gotowa! TWOJA KOLEJ na strzał!".to_string()
                        } else {
                            "🎯 Flota gotowa! Czekaj na ruch przeciwnika...".to_string()
                        };
                    } else {
                        self.status_text =
                            "🎯 Flota gotowa! Kliknij na Radar Wroga (po prawej), aby oddać salwę!"
                                .to_string();
                    }
                }
            }

            ui.separator();
            ui.label(
                egui::RichText::new(&self.status_text)
                    .size(18.0)
                    .strong()
                    .color(egui::Color32::from_rgb(135, 225, 255)),
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
        });

        ui.add_space(12.0);

        let work_rect = ui.available_rect_before_wrap();
        let gap = (work_rect.width() * 0.04).clamp(20.0, 56.0);
        let max_board_w = ((work_rect.width() - gap) / 2.0).max(220.0);
        let max_board_h = (work_rect.height() - 42.0).max(220.0);
        let board_px = max_board_w.min(max_board_h);
        let cell = board_px / 10.0;

        let total_w = board_px * 2.0 + gap;
        let start_x = work_rect.left() + ((work_rect.width() - total_w) / 2.0).max(0.0);
        let start_y = work_rect.top() + 32.0 + ((work_rect.height() - 32.0 - board_px) / 2.0).max(0.0);

        let left_board =
            egui::Rect::from_min_size(egui::pos2(start_x, start_y), egui::vec2(board_px, board_px));
        let right_board = egui::Rect::from_min_size(
            egui::pos2(start_x + board_px + gap, start_y),
            egui::vec2(board_px, board_px),
        );

        let p_alive = self
            .player_ships
            .iter()
            .filter(|s| !s.is_sunk(&self.bot_shots))
            .count();
        let b_alive = self
            .bot_ships
            .iter()
            .filter(|s| !s.is_sunk(&self.player_shots))
            .count();

        ui.painter().text(
            egui::pos2(left_board.center().x, left_board.top() - 16.0),
            egui::Align2::CENTER_CENTER,
            format!("🛡 TWOJA FLOTA (Okręty: {}/5)", p_alive),
            egui::FontId::proportional(19.0),
            egui::Color32::WHITE,
        );

        ui.painter().text(
            egui::pos2(right_board.center().x, right_board.top() - 16.0),
            egui::Align2::CENTER_CENTER,
            format!("📡 RADAR WROGA (Pozostało: {}/5)", b_alive),
            egui::FontId::proportional(19.0),
            egui::Color32::from_rgb(255, 195, 110),
        );

        Self::draw_ocean_grid(ui.painter(), left_board, cell, false);

        for ship in &self.player_ships {
            Self::draw_ship_hull(
                ui.painter(),
                left_board,
                cell,
                ship,
                ship.is_sunk(&self.bot_shots),
            );
        }

        if self.phase == Phase::Placement && self.placing_idx < FLEET_SIZES.len() {
            let (len, _) = FLEET_SIZES[self.placing_idx];
            if let Some(ptr) = ui.ctx().pointer_latest_pos() {
                if left_board.contains(ptr) {
                    let gx = ((ptr.x - left_board.left()) / cell).floor() as usize;
                    let gy = ((ptr.y - left_board.top()) / cell).floor() as usize;
                    let candidate = Ship {
                        x: gx,
                        y: gy,
                        len,
                        horizontal: self.place_horizontal,
                    };
                    let valid = Self::can_place_ship(&self.player_ships, candidate);

                    let preview_col = if valid {
                        egui::Color32::from_rgba_unmultiplied(60, 220, 110, 150)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(235, 65, 65, 150)
                    };

                    for (cx, cy) in candidate.cells() {
                        if cx < 10 && cy < 10 {
                            let r = egui::Rect::from_min_size(
                                egui::pos2(
                                    left_board.left() + cx as f32 * cell,
                                    left_board.top() + cy as f32 * cell,
                                ),
                                egui::vec2(cell, cell),
                            );
                            ui.painter().rect_filled(r.shrink(2.0), 4.0, preview_col);
                        }
                    }

                    let board_resp = ui.interact(
                        left_board,
                        egui::Id::new("bs_left_place"),
                        egui::Sense::click(),
                    );
                    if board_resp.secondary_clicked() {
                        audio.play(Sfx::Click);
                        self.place_horizontal = !self.place_horizontal;
                    } else if board_resp.clicked() && valid && !self.waiting {
                        audio.play(Sfx::CardPlace);
                        self.player_ships.push(candidate);
                        self.placing_idx += 1;
                        if self.placing_idx >= FLEET_SIZES.len() {
                            self.phase = Phase::Battle;
                            if self.online_mode {
                                self.outgoing_fleet = Some(
                                    self.player_ships
                                        .iter()
                                        .map(|s| (s.x, s.y, s.len, s.horizontal))
                                        .collect(),
                                );
                                self.status_text = if self.bot_ships.is_empty() {
                                    "⏳ Flota gotowa! Czekam na rozmieszczenie przeciwnika..."
                                        .to_string()
                                } else if self.my_turn_online {
                                    "🎯 Flota gotowa! TWOJA KOLEJ na strzał!".to_string()
                                } else {
                                    "🎯 Flota gotowa! Czekaj na ruch przeciwnika...".to_string()
                                };
                            } else {
                                self.status_text =
                                    "🎯 Wszystkie okręty na pozycjach! Strzelaj w Radar Wroga po prawej!"
                                        .to_string();
                            }
                        } else {
                            self.status_text = format!(
                                "Ustaw swój okręt: {} (PPM lub klawisz R obraca)",
                                FLEET_SIZES[self.placing_idx].1
                            );
                        }
                    }
                }
            }
        }

        Self::draw_shots_markers(
            ui.painter(),
            left_board,
            cell,
            &self.bot_shots,
            &self.player_ships,
        );

        Self::draw_ocean_grid(ui.painter(), right_board, cell, true);

        for ship in &self.bot_ships {
            let sunk = ship.is_sunk(&self.player_shots);
            if sunk || matches!(self.phase, Phase::GameOver { .. }) {
                Self::draw_ship_hull(ui.painter(), right_board, cell, ship, sunk);
            }
        }

        Self::draw_shots_markers(
            ui.painter(),
            right_board,
            cell,
            &self.player_shots,
            &self.bot_ships,
        );

        if self.phase == Phase::Battle {
            if let Some(ptr) = ui.ctx().pointer_latest_pos() {
                if right_board.contains(ptr) {
                    let gx = ((ptr.x - right_board.left()) / cell).floor() as usize;
                    let gy = ((ptr.y - right_board.top()) / cell).floor() as usize;
                    if gx < 10 && gy < 10 && !self.player_shots[gx][gy] {
                        let target_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                right_board.left() + gx as f32 * cell,
                                right_board.top() + gy as f32 * cell,
                            ),
                            egui::vec2(cell, cell),
                        );
                        ui.painter().rect_stroke(
                            target_rect.shrink(2.0),
                            4.0,
                            egui::Stroke::new(2.5_f32, egui::Color32::GOLD),
                        );
                    }
                }
            }

            let fire_resp = ui.interact(
                right_board,
                egui::Id::new("bs_right_fire"),
                egui::Sense::click(),
            );
            if fire_resp.clicked() {
                if let Some(ptr) = ui.ctx().pointer_interact_pos() {
                    let gx = ((ptr.x - right_board.left()) / cell).floor() as usize;
                    let gy = ((ptr.y - right_board.top()) / cell).floor() as usize;
                    if gx < 10 && gy < 10 {
                        self.fire_at_bot(gx, gy, audio);
                    }
                }
            }
        }

        if let Some(b) = &self.blast {
            let elapsed = now.duration_since(b.start).as_secs_f32();
            if elapsed < 0.38 {
                let t = elapsed / 0.38;
                let base_board = if b.is_enemy_board {
                    right_board
                } else {
                    left_board
                };
                let center = egui::pos2(
                    base_board.left() + (b.gx as f32 + 0.5) * cell,
                    base_board.top() + (b.gy as f32 + 0.5) * cell,
                );
                let col = if b.is_hit {
                    egui::Color32::from_rgba_unmultiplied(255, 140, 30, ((1.0 - t) * 230.0) as u8)
                } else {
                    egui::Color32::from_rgba_unmultiplied(150, 220, 255, ((1.0 - t) * 210.0) as u8)
                };
                ui.painter().circle_stroke(
                    center,
                    cell * (0.2 + t * 0.65),
                    egui::Stroke::new(3.5_f32, col),
                );
                ui.ctx().request_repaint();
            } else {
                self.blast = None;
            }
        }
    }

    /// Widok dla miejsc 3/4 — obie plansze, bez możliwości strzelania.
    fn spectator_ui(&mut self, ui: &mut egui::Ui) {
        let full_rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(full_rect, 12.0, egui::Color32::from_rgb(16, 28, 42));

        let margin = (full_rect.width() * 0.02).clamp(10.0, 26.0);
        let content_rect = full_rect.shrink(margin);

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child_ui;

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("👁 Tryb obserwatora — nie możesz strzelać")
                    .size(19.0)
                    .strong()
                    .color(egui::Color32::GOLD),
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
                        .color(egui::Color32::GOLD),
                );
            }
            ui.separator();
            ui.label(
                egui::RichText::new(&self.status_text)
                    .size(16.0)
                    .strong()
                    .color(egui::Color32::from_rgb(135, 225, 255)),
            );
        });

        if !self.state_ready {
            ui.add_space(24.0);
            ui.label(
                egui::RichText::new("⏳ Czekam na przesłanie stanu plansz od graczy...")
                    .size(18.0)
                    .color(egui::Color32::GOLD),
            );
        }

        ui.add_space(8.0);
        if self.phase == Phase::Battle {
            ui.label(
                egui::RichText::new(format!(
                    "🎯 Tura: Gracz {}",
                    self.spectator_turn + 1
                ))
                .size(17.0)
                .strong()
                .color(egui::Color32::from_rgb(255, 195, 110)),
            );
        }

        let work_rect = ui.available_rect_before_wrap();
        let gap = (work_rect.width() * 0.04).clamp(20.0, 56.0);
        let max_board_w = ((work_rect.width() - gap) / 2.0).max(220.0);
        let max_board_h = (work_rect.height() - 70.0).max(220.0);
        let board_px = max_board_w.min(max_board_h);
        let cell = board_px / 10.0;

        let total_w = board_px * 2.0 + gap;
        let start_x = work_rect.left() + ((work_rect.width() - total_w) / 2.0).max(0.0);
        let start_y =
            work_rect.top() + 8.0 + ((work_rect.height() - board_px) / 2.0).max(0.0);

        let left_board = egui::Rect::from_min_size(
            egui::pos2(start_x, start_y),
            egui::vec2(board_px, board_px),
        );
        let right_board = egui::Rect::from_min_size(
            egui::pos2(start_x + board_px + gap, start_y),
            egui::vec2(board_px, board_px),
        );

        let p_alive = self
            .player_ships
            .iter()
            .filter(|s| !s.is_sunk(&self.bot_shots))
            .count();
        let b_alive = self
            .bot_ships
            .iter()
            .filter(|s| !s.is_sunk(&self.player_shots))
            .count();

        ui.painter().text(
            egui::pos2(left_board.center().x, left_board.top() - 18.0),
            egui::Align2::CENTER_CENTER,
            format!("🟦 GRACZ 1 (Okręty: {}/5)", p_alive),
            egui::FontId::proportional(19.0),
            egui::Color32::WHITE,
        );
        ui.painter().text(
            egui::pos2(right_board.center().x, right_board.top() - 18.0),
            egui::Align2::CENTER_CENTER,
            format!("🟥 GRACZ 2 (Okręty: {}/5)", b_alive),
            egui::FontId::proportional(19.0),
            egui::Color32::WHITE,
        );

        Self::draw_ocean_grid(ui.painter(), left_board, cell, false);
        Self::draw_ocean_grid(ui.painter(), right_board, cell, true);

        for ship in &self.player_ships {
            Self::draw_ship_hull(
                ui.painter(),
                left_board,
                cell,
                ship,
                ship.is_sunk(&self.bot_shots),
            );
        }
        for ship in &self.bot_ships {
            Self::draw_ship_hull(
                ui.painter(),
                right_board,
                cell,
                ship,
                ship.is_sunk(&self.player_shots),
            );
        }

        Self::draw_shots_markers(
            ui.painter(),
            left_board,
            cell,
            &self.bot_shots,
            &self.player_ships,
        );
        Self::draw_shots_markers(
            ui.painter(),
            right_board,
            cell,
            &self.player_shots,
            &self.bot_ships,
        );

        ui.painter().text(
            egui::pos2(full_rect.center().x, full_rect.bottom() - 12.0),
            egui::Align2::CENTER_CENTER,
            "👁 Obserwator widzi obie floty — gracze widzą tylko własną planszę i radar.",
            egui::FontId::proportional(13.0),
            egui::Color32::from_rgb(150, 170, 190),
        );
    }

    fn draw_ocean_grid(
        painter: &egui::Painter,
        rect: egui::Rect,
        cell: f32,
        radar_theme: bool,
    ) {
        painter.rect_filled(
            rect.expand(4.0),
            8.0,
            egui::Color32::from_rgb(45, 75, 105),
        );

        for r in 0..10 {
            for c in 0..10 {
                let cell_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + c as f32 * cell, rect.top() + r as f32 * cell),
                    egui::vec2(cell, cell),
                );
                let alt = (r + c) % 2 == 0;
                let fill = if radar_theme {
                    if alt {
                        egui::Color32::from_rgb(16, 48, 68)
                    } else {
                        egui::Color32::from_rgb(14, 42, 60)
                    }
                } else if alt {
                    egui::Color32::from_rgb(22, 62, 96)
                } else {
                    egui::Color32::from_rgb(18, 55, 86)
                };

                painter.rect(
                    cell_rect,
                    0.0,
                    fill,
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(28)),
                );
            }
        }
    }

    fn draw_ship_hull(
        painter: &egui::Painter,
        board_rect: egui::Rect,
        cell: f32,
        ship: &Ship,
        is_sunk: bool,
    ) {
        let w = if ship.horizontal {
            ship.len as f32 * cell
        } else {
            cell
        };
        let h = if ship.horizontal {
            cell
        } else {
            ship.len as f32 * cell
        };

        let r = egui::Rect::from_min_size(
            egui::pos2(
                board_rect.left() + ship.x as f32 * cell,
                board_rect.top() + ship.y as f32 * cell,
            ),
            egui::vec2(w, h),
        )
        .shrink(cell * 0.12);

        let hull_fill = if is_sunk {
            egui::Color32::from_rgb(95, 42, 42)
        } else {
            egui::Color32::from_rgb(110, 122, 138)
        };
        let stroke_col = if is_sunk {
            egui::Color32::from_rgb(220, 70, 70)
        } else {
            egui::Color32::from_rgb(185, 198, 212)
        };

        painter.rect(
            r,
            cell * 0.35,
            hull_fill,
            egui::Stroke::new(2.0_f32, stroke_col),
        );

        for (cx, cy) in ship.cells() {
            let center = egui::pos2(
                board_rect.left() + (cx as f32 + 0.5) * cell,
                board_rect.top() + (cy as f32 + 0.5) * cell,
            );
            painter.circle_filled(
                center,
                cell * 0.16,
                egui::Color32::from_black_alpha(75),
            );
        }
    }

    fn draw_shots_markers(
        painter: &egui::Painter,
        board_rect: egui::Rect,
        cell: f32,
        shots: &[[bool; 10]; 10],
        ships: &[Ship],
    ) {
        for x in 0..10 {
            for y in 0..10 {
                if !shots[x][y] {
                    continue;
                }
                let center = egui::pos2(
                    board_rect.left() + (x as f32 + 0.5) * cell,
                    board_rect.top() + (y as f32 + 0.5) * cell,
                );

                if Self::has_ship_at(ships, x, y) {
                    painter.circle_filled(
                        center,
                        cell * 0.30,
                        egui::Color32::from_rgb(235, 55, 45),
                    );
                    painter.circle_filled(
                        center,
                        cell * 0.15,
                        egui::Color32::from_rgb(255, 215, 60),
                    );
                } else {
                    painter.circle_filled(
                        center,
                        cell * 0.14,
                        egui::Color32::from_rgba_unmultiplied(180, 225, 255, 160),
                    );
                }
            }
        }
    }
}