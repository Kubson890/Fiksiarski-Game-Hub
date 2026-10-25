use rodio::{buffer::SamplesBuffer, OutputStream, OutputStreamHandle, Sink};
use std::f32::consts::PI;

#[derive(Clone, Copy)]
pub enum Sfx {
    Click,     // Kliknięcie w menu
    CardSlide, // Dobieranie / przekładanie karty
    CardPlace, // Upuszczenie karty na stos
    ChipBet,   // Postawienie żetonów
    ChessMove, // Drewniane stuknięcie figury o szachownicę
    ChessCap,  // Mocniejsze zbicie figury
    Check,     // Ostrzegawczy dźwięk szacha
    Win,       // Fanfara wygranej (akord durowy)
    Lose,      // Przegrana / Fura w Blackjacku
}

pub struct SoundManager {
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    pub muted: bool,
}

impl Default for SoundManager {
    fn default() -> Self {
        match OutputStream::try_default() {
            Ok((stream, handle)) => Self {
                _stream: Some(stream),
                handle: Some(handle),
                muted: false,
            },
            Err(_) => Self {
                _stream: None,
                handle: None,
                muted: false,
            },
        }
    }
}

impl SoundManager {
    pub fn play(&self, sfx: Sfx) {
        if self.muted {
            return;
        }
        let Some(handle) = &self.handle else { return };

        let sample_rate = 44_100;
        let samples = match sfx {
            // Krótki, przyjemny "klik" interfejsu
            Sfx::Click => Self::synth_tone(sample_rate, 680.0, 920.0, 0.045, 0.18, 0.0),

            // Miękki szelest przesuwanej karty (połączenie wysokiego tonu z szumem)
            Sfx::CardSlide => Self::synth_tone(sample_rate, 520.0, 880.0, 0.065, 0.14, 0.45),

            // Położenie karty na stole
            Sfx::CardPlace => Self::synth_tone(sample_rate, 340.0, 190.0, 0.06, 0.22, 0.25),

            // Metaliczny brzęk żetonu kasynowego
            Sfx::ChipBet => Self::synth_chord(sample_rate, &[1760.0, 2637.0], 0.09, 0.14),

            // Drewniane puknięcie figury szachowej (niska częstotliwość szybko opadająca)
            Sfx::ChessMove => Self::synth_tone(sample_rate, 240.0, 95.0, 0.075, 0.35, 0.10),

            // Zbicie figury na szachownicy (bardziej basowe, wyraziste uderzenie)
            Sfx::ChessCap => Self::synth_tone(sample_rate, 310.0, 70.0, 0.11, 0.45, 0.25),

            // Dźwięk ostrzegawczy "Szach!"
            Sfx::Check => Self::synth_arpeggio(sample_rate, &[587.3, 880.0], 0.11, 0.22),

            // Radosne arpeggio C-dur (C5 -> E5 -> G5 -> C6)
            Sfx::Win => {
                Self::synth_arpeggio(sample_rate, &[523.25, 659.25, 783.99, 1046.50], 0.11, 0.22)
            }

            // Opadające tony przegranej
            Sfx::Lose => Self::synth_arpeggio(sample_rate, &[392.0, 349.2, 311.1], 0.14, 0.20),
        };

        if let Ok(sink) = Sink::try_new(handle) {
            let buf = SamplesBuffer::new(1, sample_rate, samples);
            sink.append(buf);
            sink.detach();
        }
    }

    /// Generuje pojedynczy ton z płynną zmianą częstotliwości (pitch sweep) i domieszką szumu
    fn synth_tone(
        rate: u32,
        f_start: f32,
        f_end: f32,
        duration: f32,
        vol: f32,
        noise_mix: f32,
    ) -> Vec<f32> {
        let len = (rate as f32 * duration) as usize;
        let mut out = Vec::with_capacity(len);
        let mut phase = 0.0f32;
        let mut rng_state = 123456789u32;

        for i in 0..len {
            let t = i as f32 / len as f32;
            let freq = f_start + (f_end - f_start) * t;
            phase += 2.0 * PI * freq / rate as f32;

            // Szybki atak i wykładnicze wygaszanie (envelope)
            let attack = (t / 0.08).min(1.0);
            let decay = (1.0 - t).powi(2);
            let env = attack * decay;

            // Prosty generator szumu (XOR-shift) dla efektu tarcia karty/drewna
            rng_state ^= rng_state << 13;
            rng_state ^= rng_state >> 17;
            rng_state ^= rng_state << 5;
            let noise = (rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0;

            let signal = phase.sin() * (1.0 - noise_mix) + noise * noise_mix;
            out.push(signal * env * vol);
        }
        out
    }

    /// Generuje akord (kilka częstotliwości jednocześnie)
    fn synth_chord(rate: u32, freqs: &[f32], duration: f32, vol: f32) -> Vec<f32> {
        let len = (rate as f32 * duration) as usize;
        let mut out = Vec::with_capacity(len);

        for i in 0..len {
            let time = i as f32 / rate as f32;
            let t = i as f32 / len as f32;
            let env = (t / 0.05).min(1.0) * (1.0 - t).powi(2);

            let mut sum = 0.0;
            for &f in freqs {
                sum += (2.0 * PI * f * time).sin();
            }
            out.push((sum / freqs.len() as f32) * env * vol);
        }
        out
    }

    /// Generuje sekwencję nut (arpeggio) np. dla wygranej lub przegranej
    fn synth_arpeggio(rate: u32, notes: &[f32], note_dur: f32, vol: f32) -> Vec<f32> {
        let mut out = Vec::new();
        for &freq in notes {
            let part = Self::synth_tone(rate, freq, freq, note_dur, vol, 0.0);
            out.extend(part);
        }
        out
    }
}