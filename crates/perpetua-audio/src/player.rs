use crate::fader::{compute_crossfade_gains, FadeCurve};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey};
use symphonia::core::probe::Hint;

/// Capacidad por defecto del Ring Buffer SPSC para el Main Deck (potencia de 2: 262,144 muestras ≈ 2.7s a 48kHz estéreo).
const MAIN_RING_CAPACITY: usize = 1 << 18;
/// Capacidad del Ring Buffer SPSC para cada Jingle/Cart y para el reproductor Cue (131,072 muestras ≈ 1.36s a 48kHz estéreo).
const JINGLE_RING_CAPACITY: usize = 1 << 17;

/// Modo de superposición no destructiva para eventos del planificador sobre el Main Deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventOverlayMode {
    /// Atenúa suavemente el volumen del Main Deck un 70% (ganancia 0.30) mientras suena el evento,
    /// y restaura el volumen al 100% al finalizar sin detener la canción actual.
    #[default]
    Ducking,
    /// Pausa el Main Deck en el frame exacto, reproduce el evento en el canal superpuesto,
    /// y reanuda la canción exactamente donde se quedó al terminar el evento.
    PauseResume,
}

/// Metadatos extraídos de forma perezosa (Lazy Probing) leyendo únicamente las cabeceras del contenedor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackProbeInfo {
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// Tiempo de Intro leído de tags ID3/Vorbis (`INTRO`, `PERPETUA_INTRO`, `INTRO_MS`) si existe en el archivo.
    #[serde(default)]
    pub intro_ms: Option<u64>,
}

/// Picos de amplitud normalizados (`0.0..=1.0`) para dibujar la forma de onda en el Editor Visual de Puntos de Mezcla.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveformPeaks {
    pub path: String,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub peaks: Vec<f32>,
}

/// Especificación para armar el disparo sincronizado de un Pisador (Sweeper) sobre la Intro del Main Deck.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SweeperScheduleSpec {
    pub sweeper_path: PathBuf,
    pub intro_ms: u64,
    pub sweeper_duration_ms: u64,
    pub trigger_at_ms: u64,
}

/// Calcula el instante exacto de disparo de un Pisador (Sweeper) en milisegundos según la fórmula:
/// `(Tiempo de Intro de la Canción) - (Duración del Pisador)`.
/// Si la canción tiene 12s (`12000` ms) de intro y el pisador dura 4s (`4000` ms), devuelve `8000` ms (segundo 8).
/// Si el pisador es más largo o igual que la intro, devuelve `0` (disparo inmediato al iniciar la pista).
#[inline]
pub fn compute_sweeper_trigger_ms(intro_ms: u64, sweeper_duration_ms: u64) -> u64 {
    intro_ms.saturating_sub(sweeper_duration_ms)
}

/// Telemetría ligera y de alta frecuencia (16ms–30ms) exclusiva del Cue Deck
/// para sincronizar el Cabezal de Reproducción (Playhead) sobre el Canvas de Forma de Onda.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CueTelemetry {
    pub is_playing: bool,
    pub current_position_ms: u64,
    pub duration_ms: u64,
    pub cue_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStatus {
    pub is_playing: bool,
    #[serde(default)]
    pub is_crossfading: bool,
    /// Indica si el Main Deck está actualmente atenuado (-70% / ganancia 0.30) por un evento en modo Ducking.
    #[serde(default)]
    pub is_ducking: bool,
    /// Indica si el Main Deck está actualmente pausado por un evento en modo Pausa y Reanuda.
    #[serde(default)]
    pub is_paused_by_event: bool,
    /// Indica si hay un evento no destructivo reproduciéndose sobre el canal superpuesto.
    #[serde(default)]
    pub overlay_is_playing: bool,
    /// Nombre/ruta del evento superpuesto activo.
    #[serde(default)]
    pub overlay_file: Option<String>,
    /// Hito 8: Indica si hay un Pisador (Sweeper) armado esperando el segundo exacto de disparo o sonando.
    #[serde(default)]
    pub sweeper_armed: bool,
    /// Hito 8: Indica si el Pisador (Sweeper) está sonando activamente en este instante sobre la Intro.
    #[serde(default)]
    pub sweeper_is_playing: bool,
    /// Hito 8: Archivo del Pisador (Sweeper) armado o en reproducción.
    #[serde(default)]
    pub sweeper_file: Option<String>,
    /// Hito 8: Tiempo de Intro (ms) de la canción activa sobre la que se sincroniza el Pisador.
    #[serde(default)]
    pub sweeper_intro_ms: u64,
    /// Hito 8: Segundo/milisegundo exacto calculado `(Intro - Duración Pisador)` en el que dispara el Pisador.
    #[serde(default)]
    pub sweeper_trigger_ms: u64,
    /// Hito 8: Duración del Pisador (ms).
    #[serde(default)]
    pub sweeper_duration_ms: u64,
    pub current_file: Option<String>,
    pub elapsed_ms: u64,
    pub duration_ms: u64,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub decoded_frames: u64,
    pub last_error: Option<String>,
    /// Ranuras de Jingles (1..=9) que están sonando activamente en este instante sobre el Main Deck.
    #[serde(default)]
    pub active_jingle_slots: Vec<usize>,
    /// Estado del reproductor de Preescucha (Cue)
    #[serde(default)]
    pub cue_is_playing: bool,
    #[serde(default)]
    pub cue_file: Option<String>,
    #[serde(default)]
    pub cue_elapsed_ms: u64,
    #[serde(default)]
    pub cue_duration_ms: u64,
    #[serde(default)]
    pub cue_device_name: String,
    /// Configuración de ruteo seleccionada por el usuario
    #[serde(default)]
    pub air_device_selected: String,
    #[serde(default)]
    pub cue_device_selected: String,
    /// Lista de dispositivos de salida detectados en el sistema
    #[serde(default)]
    pub available_devices: Vec<String>,
}

impl Default for PlayerStatus {
    fn default() -> Self {
        Self {
            is_playing: false,
            is_crossfading: false,
            is_ducking: false,
            is_paused_by_event: false,
            overlay_is_playing: false,
            overlay_file: None,
            sweeper_armed: false,
            sweeper_is_playing: false,
            sweeper_file: None,
            sweeper_intro_ms: 0,
            sweeper_trigger_ms: 0,
            sweeper_duration_ms: 0,
            current_file: None,
            elapsed_ms: 0,
            duration_ms: 0,
            device_name: "Default Output".to_string(),
            sample_rate: 0,
            channels: 0,
            decoded_frames: 0,
            last_error: None,
            active_jingle_slots: Vec::new(),
            cue_is_playing: false,
            cue_file: None,
            cue_elapsed_ms: 0,
            cue_duration_ms: 0,
            cue_device_name: "Default Output".to_string(),
            air_device_selected: "Default Output".to_string(),
            cue_device_selected: "Default Output".to_string(),
            available_devices: vec!["Default Output".to_string()],
        }
    }
}

/// Enumera todas las tarjetas/dispositivos de salida disponibles en el sistema mediante `cpal`.
/// Siempre incluye `"Default Output"` como primera opción segura.
pub fn list_output_devices() -> Vec<String> {
    let mut names = vec!["Default Output".to_string()];
    let host = cpal::default_host();
    if let Ok(devices) = host.output_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                let trimmed = name.trim();
                if !trimmed.is_empty() && !names.iter().any(|n| n == trimmed) {
                    names.push(trimmed.to_string());
                }
            }
        }
    }
    names
}

/// Resuelve un dispositivo de salida `cpal` por nombre. Si no se encuentra o es `"Default Output"`,
/// hace fallback automático al dispositivo de salida por defecto del sistema.
fn resolve_output_device(requested_name: &str) -> Result<(cpal::Device, String), String> {
    let host = cpal::default_host();
    let req = requested_name.trim();

    if !req.is_empty() && req != "Default Output" {
        if let Ok(mut devices) = host.output_devices() {
            if let Some(found) = devices.find(|d| d.name().map(|n| n.trim() == req).unwrap_or(false))
            {
                let actual_name = found.name().unwrap_or_else(|_| req.to_string());
                return Ok((found, actual_name));
            }
        }
    }

    let fallback = host.default_output_device().ok_or_else(|| {
        "No se encontró ningún dispositivo de salida de audio en el sistema (cpal)".to_string()
    })?;
    let actual_name = fallback
        .name()
        .unwrap_or_else(|_| "Default Output".to_string());
    Ok((fallback, actual_name))
}

/// Buffer circular SPSC (Single-Producer Single-Consumer) libre de bloqueos (Wait-Free / Lock-Free).
pub struct SpscRingBuffer {
    buffer: Box<[AtomicU32]>,
    mask: usize,
    write_idx: AtomicUsize,
    read_idx: AtomicUsize,
    eof_reached: AtomicBool,
    cancelled: AtomicBool,
}

impl SpscRingBuffer {
    pub fn new(min_capacity: usize) -> Self {
        let cap = min_capacity.next_power_of_two().max(1024);
        let buffer = (0..cap)
            .map(|_| AtomicU32::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            buffer,
            mask: cap - 1,
            write_idx: AtomicUsize::new(0),
            read_idx: AtomicUsize::new(0),
            eof_reached: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn available_read(&self) -> usize {
        let w = self.write_idx.load(Ordering::Acquire);
        let r = self.read_idx.load(Ordering::Relaxed);
        w.wrapping_sub(r)
    }

    #[inline]
    pub fn available_write(&self) -> usize {
        let w = self.write_idx.load(Ordering::Relaxed);
        let r = self.read_idx.load(Ordering::Acquire);
        self.buffer.len().saturating_sub(w.wrapping_sub(r))
    }

    #[inline]
    pub fn push_slice(&self, samples: &[f32]) -> usize {
        let avail = self.available_write();
        let to_write = samples.len().min(avail);
        if to_write == 0 {
            return 0;
        }
        let w = self.write_idx.load(Ordering::Relaxed);
        for (i, &sample) in samples[..to_write].iter().enumerate() {
            self.buffer[(w.wrapping_add(i)) & self.mask]
                .store(sample.to_bits(), Ordering::Relaxed);
        }
        self.write_idx
            .store(w.wrapping_add(to_write), Ordering::Release);
        to_write
    }

    #[inline]
    pub fn pop_frame(&self, out_frame: &mut [f32]) -> bool {
        let needed = out_frame.len();
        if self.available_read() < needed {
            for s in out_frame.iter_mut() {
                *s = 0.0;
            }
            return false;
        }
        let r = self.read_idx.load(Ordering::Relaxed);
        for (i, slot) in out_frame.iter_mut().enumerate() {
            let bits = self.buffer[(r.wrapping_add(i)) & self.mask].load(Ordering::Relaxed);
            *slot = f32::from_bits(bits);
        }
        self.read_idx
            .store(r.wrapping_add(needed), Ordering::Release);
        true
    }

    #[inline]
    pub fn mark_eof(&self) {
        self.eof_reached.store(true, Ordering::Release);
    }

    #[inline]
    pub fn is_eof(&self) -> bool {
        self.eof_reached.load(Ordering::Acquire)
    }

    #[inline]
    pub fn is_finished(&self) -> bool {
        self.is_eof() && self.available_read() == 0
    }

    #[inline]
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

enum AudioThreadCommand {
    Play {
        path: PathBuf,
        overlap_ms: u64,
        reply: mpsc::Sender<Result<PlayerStatus, String>>,
    },
    Stop {
        reply: mpsc::Sender<PlayerStatus>,
    },
    PlayJingle {
        slot: usize,
        path: PathBuf,
        reply: mpsc::Sender<Result<PlayerStatus, String>>,
    },
    PlayEventOverlay {
        paths: Vec<PathBuf>,
        mode: EventOverlayMode,
        reply: mpsc::Sender<Result<PlayerStatus, String>>,
    },
    ScheduleSweeper {
        spec: Option<SweeperScheduleSpec>,
        reply: mpsc::Sender<Result<PlayerStatus, String>>,
    },
    PlayCue {
        path: Option<PathBuf>,
        start_offset_ms: u64,
        reply: mpsc::Sender<Result<PlayerStatus, String>>,
    },
    StopCue {
        reply: mpsc::Sender<PlayerStatus>,
    },
    SetRouting {
        air_device: String,
        cue_device: String,
        reply: mpsc::Sender<Result<PlayerStatus, String>>,
    },
    RefreshDevices {
        reply: mpsc::Sender<Vec<String>>,
    },
}

/// Pista principal en streaming en el mezclador de tiempo real (Main Deck).
struct PrimaryDeck {
    ring: Arc<SpscRingBuffer>,
    played_frames: usize,
    fade_in_elapsed_frames: usize,
    fade_in_total_frames: usize,
}

impl PrimaryDeck {
    #[inline]
    fn current_gain(&self) -> f32 {
        if self.fade_in_total_frames == 0 || self.fade_in_elapsed_frames >= self.fade_in_total_frames
        {
            1.0
        } else {
            let progress = self.fade_in_elapsed_frames as f32 / self.fade_in_total_frames as f32;
            let (_, gain_in) = compute_crossfade_gains(progress, FadeCurve::EqualPower);
            gain_in
        }
    }
}

impl Drop for PrimaryDeck {
    fn drop(&mut self) {
        self.ring.cancel();
    }
}

/// Pista saliente en proceso de fundido descendente (fade-out) durante un crossfade o parada suave.
struct OutgoingDeck {
    ring: Arc<SpscRingBuffer>,
    initial_gain: f32,
    fade_out_elapsed_frames: usize,
    fade_out_total_frames: usize,
}

impl OutgoingDeck {
    #[inline]
    fn current_gain(&self) -> f32 {
        if self.fade_out_total_frames == 0
            || self.fade_out_elapsed_frames >= self.fade_out_total_frames
        {
            0.0
        } else {
            let progress = self.fade_out_elapsed_frames as f32 / self.fade_out_total_frames as f32;
            let (gain_out, _) = compute_crossfade_gains(progress, FadeCurve::EqualPower);
            self.initial_gain * gain_out
        }
    }
}

impl Drop for OutgoingDeck {
    fn drop(&mut self) {
        self.ring.cancel();
    }
}

/// Voz concurrente de Jingle / Cart Machine reproducida en streaming por encima del Main Deck.
struct JingleVoice {
    slot: usize,
    ring: Arc<SpscRingBuffer>,
    fade_in_elapsed_frames: usize,
    fade_in_total_frames: usize,
}

impl JingleVoice {
    #[inline]
    fn current_gain(&self) -> f32 {
        if self.fade_in_total_frames == 0 || self.fade_in_elapsed_frames >= self.fade_in_total_frames
        {
            1.0
        } else {
            self.fade_in_elapsed_frames as f32 / self.fade_in_total_frames as f32
        }
    }
}

impl Drop for JingleVoice {
    fn drop(&mut self) {
        self.ring.cancel();
    }
}

/// Voz superpuesta para eventos programados no destructivos (Ducking -70% o Pausa y Reanudación).
struct OverlayVoice {
    ring: Arc<SpscRingBuffer>,
    fade_in_elapsed_frames: usize,
    fade_in_total_frames: usize,
}

impl OverlayVoice {
    #[inline]
    fn current_gain(&self) -> f32 {
        if self.fade_in_total_frames == 0 || self.fade_in_elapsed_frames >= self.fade_in_total_frames
        {
            1.0
        } else {
            self.fade_in_elapsed_frames as f32 / self.fade_in_total_frames as f32
        }
    }
}

impl Drop for OverlayVoice {
    fn drop(&mut self) {
        self.ring.cancel();
    }
}

/// Voz de Pisador (Sweeper) sincronizada al frame exacto `(Tiempo de Intro - Duración del Pisador)` sobre el Main Deck.
struct ScheduledSweeperVoice {
    ring: Arc<SpscRingBuffer>,
    trigger_frame: usize,
    started: bool,
    fade_in_elapsed_frames: usize,
    fade_in_total_frames: usize,
}

impl ScheduledSweeperVoice {
    #[inline]
    fn current_gain(&self) -> f32 {
        if self.fade_in_total_frames == 0 || self.fade_in_elapsed_frames >= self.fade_in_total_frames
        {
            1.0
        } else {
            self.fade_in_elapsed_frames as f32 / self.fade_in_total_frames as f32
        }
    }
}

impl Drop for ScheduledSweeperVoice {
    fn drop(&mut self) {
        self.ring.cancel();
    }
}

/// Deck de Preescucha (Cue). Puede ejecutarse en una segunda tarjeta de sonido independiente
/// o compartirse automáticamente sobre el mezclador principal cuando el usuario solo tiene una tarjeta.
struct CueDeck {
    ring: Arc<SpscRingBuffer>,
    played_frames: usize,
    fade_in_elapsed_frames: usize,
    fade_in_total_frames: usize,
}

impl CueDeck {
    #[inline]
    fn current_gain(&self) -> f32 {
        if self.fade_in_total_frames == 0 || self.fade_in_elapsed_frames >= self.fade_in_total_frames
        {
            1.0
        } else {
            self.fade_in_elapsed_frames as f32 / self.fade_in_total_frames as f32
        }
    }
}

impl Drop for CueDeck {
    fn drop(&mut self) {
        self.ring.cancel();
    }
}

/// Limitador suave (soft-knee) sin distorsión para la suma multicanal.
#[inline]
pub fn soft_limit_sample(x: f32) -> f32 {
    let abs_x = x.abs();
    if abs_x <= 0.85 {
        x
    } else {
        let excess = (abs_x - 0.85) / 0.15;
        let compressed = 0.85 + 0.15 * excess.tanh();
        compressed.copysign(x)
    }
}

struct MixerDecks {
    primary: Option<PrimaryDeck>,
    outgoing: Option<OutgoingDeck>,
    jingles: Vec<JingleVoice>,
    /// Cola secuencial de voces de eventos no destructivos (soporta tanto 1 archivo como secuencias .time).
    event_overlays: VecDeque<OverlayVoice>,
    /// Modo de interrupción del evento superpuesto activo (Ducking -70% o Pause & Resume).
    event_overlay_mode: EventOverlayMode,
    /// Ganancia de atenuación suave (Ducking) sobre el Main Deck (1.0 normal -> 0.30 cuando atenúa un 70%).
    duck_gain: f32,
    /// Ganancia de envolvente suave para Pausa y Reanudación (1.0 reproduciendo -> 0.0 en pausa).
    pause_envelope_gain: f32,
    /// Hito 8: Pisador (Sweeper) armado para dispararse en `trigger_frame = (Intro - Duración Pisador)`.
    sweeper: Option<ScheduledSweeperVoice>,
    /// Ganancia de atenuación suave de la música durante el Pisador (1.0 normal -> 0.55 mientras habla el pisador).
    sweeper_duck_gain: f32,
    /// Canal de Cue compartido cuando el usuario usa una única tarjeta de sonido para Aire y Cue.
    shared_cue: Option<CueDeck>,
}

impl Default for MixerDecks {
    fn default() -> Self {
        Self {
            primary: None,
            outgoing: None,
            jingles: Vec::new(),
            event_overlays: VecDeque::new(),
            event_overlay_mode: EventOverlayMode::Ducking,
            duck_gain: 1.0,
            pause_envelope_gain: 1.0,
            sweeper: None,
            sweeper_duck_gain: 1.0,
            shared_cue: None,
        }
    }
}

impl MixerDecks {
    /// Mezcla un frame multicanal aplicando Ducking (-70% -> 0.30), Pausa y Reanudación y Pisadores (Sweepers)
    /// sincronizados al frame exacto `(Intro - Duración Pisador)` sin destruir el Main Deck.
    /// Devuelve `(drop_outgoing, drop_shared_cue)`.
    #[inline]
    fn mix_next_frame(
        &mut self,
        ch_len: usize,
        sample_rate: u32,
        mixed_frame: &mut [f32; 8],
    ) -> (bool, bool) {
        while self
            .event_overlays
            .front()
            .map(|v| v.ring.is_finished())
            .unwrap_or(false)
        {
            self.event_overlays.pop_front();
        }

        let has_overlay = !self.event_overlays.is_empty();
        let sr = sample_rate.max(1) as f32;

        // 1. Cálculo de rampa suave para Modo Atenuación (Ducking -70% => ganancia objetivo 0.30)
        let target_duck = if has_overlay && self.event_overlay_mode == EventOverlayMode::Ducking {
            0.30_f32
        } else {
            1.0_f32
        };
        let duck_step = 0.70_f32 / (sr * 0.18).max(1.0);
        if self.duck_gain > target_duck {
            self.duck_gain = (self.duck_gain - duck_step).max(target_duck);
        } else if self.duck_gain < target_duck {
            self.duck_gain = (self.duck_gain + duck_step).min(target_duck);
        }

        // 2. Cálculo de envolvente y congelación de lectura para Modo Pausa y Reanudación
        let pause_requested =
            has_overlay && self.event_overlay_mode == EventOverlayMode::PauseResume;
        let pause_step = 1.0_f32 / (sr * 0.012).max(1.0);
        if pause_requested {
            if self.pause_envelope_gain > 0.0 {
                self.pause_envelope_gain = (self.pause_envelope_gain - pause_step).max(0.0);
            }
        } else if self.pause_envelope_gain < 1.0 {
            self.pause_envelope_gain = (self.pause_envelope_gain + pause_step).min(1.0);
        }

        // Cuando pause_requested está activo y la envolvente llegó a 0.0, congelamos el Main Deck
        // sin extraer muestras del SpscRingBuffer para reanudar exactamente en el mismo frame.
        let freeze_main_deck = pause_requested && self.pause_envelope_gain <= 1e-5;

        // 3. Evaluación sincronizada al frame del Pisador (Sweeper) sobre la Intro
        let current_primary_frame = self
            .primary
            .as_ref()
            .map(|p| p.played_frames)
            .unwrap_or(0);
        if !freeze_main_deck {
            if let Some(ref mut sw) = self.sweeper {
                if !sw.started && current_primary_frame >= sw.trigger_frame {
                    sw.started = true;
                }
            }
        }
        let sweeper_speaking = !freeze_main_deck
            && self
                .sweeper
                .as_ref()
                .map(|sw| sw.started && !sw.ring.is_finished())
                .unwrap_or(false);
        let target_sweeper_duck = if sweeper_speaking { 0.55_f32 } else { 1.0_f32 };
        let sweeper_duck_step = 0.45_f32 / (sr * 0.12).max(1.0);
        if self.sweeper_duck_gain > target_sweeper_duck {
            self.sweeper_duck_gain =
                (self.sweeper_duck_gain - sweeper_duck_step).max(target_sweeper_duck);
        } else if self.sweeper_duck_gain < target_sweeper_duck {
            self.sweeper_duck_gain =
                (self.sweeper_duck_gain + sweeper_duck_step).min(target_sweeper_duck);
        }

        let has_jingles = !self.jingles.is_empty();
        let jingle_duck = if has_jingles { 0.85_f32 } else { 1.0_f32 };
        let main_attenuation =
            self.duck_gain * self.pause_envelope_gain * self.sweeper_duck_gain * jingle_duck;

        let mut primary_frame = [0.0_f32; 8];
        let mut outgoing_frame = [0.0_f32; 8];
        let mut jingle_frame = [0.0_f32; 8];
        let mut overlay_frame = [0.0_f32; 8];
        let mut sweeper_frame = [0.0_f32; 8];
        let mut cue_frame = [0.0_f32; 8];

        // Main Deck
        let mut primary_gain = 0.0_f32;
        if !freeze_main_deck {
            if let Some(ref mut primary) = self.primary {
                primary_gain = primary.current_gain() * main_attenuation;
                if primary.ring.pop_frame(&mut primary_frame[..ch_len]) {
                    primary.played_frames += 1;
                    if primary.fade_in_elapsed_frames < primary.fade_in_total_frames {
                        primary.fade_in_elapsed_frames += 1;
                    }
                }
            }
        }

        // Outgoing Crossfade Deck
        let mut outgoing_gain = 0.0_f32;
        let mut drop_outgoing = false;
        if !freeze_main_deck {
            if let Some(ref mut outgoing) = self.outgoing {
                outgoing_gain = outgoing.current_gain() * main_attenuation;
                let _ = outgoing.ring.pop_frame(&mut outgoing_frame[..ch_len]);
                outgoing.fade_out_elapsed_frames += 1;
                if outgoing.ring.is_finished()
                    || outgoing.fade_out_elapsed_frames >= outgoing.fade_out_total_frames
                {
                    drop_outgoing = true;
                }
            }
            if drop_outgoing {
                self.outgoing = None;
            }
        }

        for c in 0..ch_len {
            mixed_frame[c] =
                primary_frame[c] * primary_gain + outgoing_frame[c] * outgoing_gain;
        }

        // Hito 8: Mezcla sincronizada del Pisador (Sweeper) cuando ha alcanzado su segundo/frame de disparo
        if !freeze_main_deck {
            let mut drop_sweeper = false;
            if let Some(ref mut sw) = self.sweeper {
                if sw.started {
                    let sw_gain = sw.current_gain();
                    if sw.ring.pop_frame(&mut sweeper_frame[..ch_len]) {
                        if sw.fade_in_elapsed_frames < sw.fade_in_total_frames {
                            sw.fade_in_elapsed_frames += 1;
                        }
                        for c in 0..ch_len {
                            mixed_frame[c] += sweeper_frame[c] * sw_gain;
                        }
                    }
                    if sw.ring.is_finished() {
                        drop_sweeper = true;
                    }
                }
            }
            if drop_sweeper {
                self.sweeper = None;
            }
        }

        // Eventos Programados Superpuestos (Ducking / Pause & Resume) — cola secuencial gapless
        if let Some(front_overlay) = self.event_overlays.front_mut() {
            let ov_gain = front_overlay.current_gain();
            if front_overlay.ring.pop_frame(&mut overlay_frame[..ch_len]) {
                if front_overlay.fade_in_elapsed_frames < front_overlay.fade_in_total_frames {
                    front_overlay.fade_in_elapsed_frames += 1;
                }
                for c in 0..ch_len {
                    mixed_frame[c] += overlay_frame[c] * ov_gain;
                }
            }
        }
        while self
            .event_overlays
            .front()
            .map(|v| v.ring.is_finished())
            .unwrap_or(false)
        {
            self.event_overlays.pop_front();
        }

        // Jingles (Cart Machine)
        for jingle in &mut self.jingles {
            let j_gain = jingle.current_gain();
            if jingle.ring.pop_frame(&mut jingle_frame[..ch_len]) {
                if jingle.fade_in_elapsed_frames < jingle.fade_in_total_frames {
                    jingle.fade_in_elapsed_frames += 1;
                }
                for c in 0..ch_len {
                    mixed_frame[c] += jingle_frame[c] * j_gain;
                }
            }
        }
        self.jingles.retain(|j| !j.ring.is_finished());

        // Canal Cue compartido (cuando Aire y Cue usan la misma tarjeta de sonido)
        let mut drop_shared_cue = false;
        if let Some(ref mut cue) = self.shared_cue {
            let c_gain = cue.current_gain();
            if cue.ring.pop_frame(&mut cue_frame[..ch_len]) {
                cue.played_frames += 1;
                if cue.fade_in_elapsed_frames < cue.fade_in_total_frames {
                    cue.fade_in_elapsed_frames += 1;
                }
                for c in 0..ch_len {
                    mixed_frame[c] += cue_frame[c] * c_gain;
                }
            }
            if cue.ring.is_finished() {
                drop_shared_cue = true;
            }
        }
        if drop_shared_cue {
            self.shared_cue = None;
        }

        (drop_outgoing, drop_shared_cue)
    }
}

#[derive(Default)]
struct DedicatedCueMixer {
    cue: Option<CueDeck>,
}

struct ActiveHardwareStream {
    _stream: cpal::Stream,
    requested_name: String,
    device_name: String,
    sample_rate: u32,
    channels: usize,
    mixer: Arc<Mutex<MixerDecks>>,
}

struct ActiveCueHardwareStream {
    _stream: cpal::Stream,
    requested_name: String,
    device_name: String,
    sample_rate: u32,
    channels: usize,
    mixer: Arc<Mutex<DedicatedCueMixer>>,
}

/// 1. Inspección Perezosa (Lazy Probing):
/// Inspecciona únicamente las cabeceras del archivo con `symphonia` para extraer duración,
/// sample rate, canales y metadatos (título/artista) SIN decodificar paquetes de audio.
pub fn probe_track_metadata(path: &Path) -> Result<TrackProbeInfo, String> {
    let file = File::open(path)
        .map_err(|e| format!("No se pudo abrir '{}': {}", path.display(), e))?;
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);

    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Formato no soportado por Symphonia: {}", e))?;

    let mut title: Option<String> = None;
    let mut artist: Option<String> = None;
    let mut intro_ms: Option<u64> = None;

    let parse_intro_tag = |key_raw: &str, val_raw: &str| -> Option<u64> {
        let k = key_raw.trim().to_ascii_uppercase();
        if k == "INTRO_MS" || k == "PERPETUA_INTRO_MS" {
            return val_raw.trim().parse::<u64>().ok();
        }
        if k == "INTRO" || k == "PERPETUA_INTRO" || k == "TIMEOFINTRO" || k == "INTRO_SEC" {
            if let Ok(sec) = val_raw.trim().parse::<f64>() {
                if sec >= 0.0 {
                    return Some((sec * 1000.0).round() as u64);
                }
            }
        }
        None
    };

    if let Some(meta_rev) = probed.metadata.get() {
        if let Some(rev) = meta_rev.current() {
            for tag in rev.tags() {
                if intro_ms.is_none() {
                    intro_ms = parse_intro_tag(&tag.key, &tag.value.to_string());
                }
                match tag.std_key {
                    Some(StandardTagKey::TrackTitle) if title.is_none() => {
                        let val = tag.value.to_string();
                        if !val.trim().is_empty() {
                            title = Some(val.trim().to_string());
                        }
                    }
                    Some(StandardTagKey::Artist) if artist.is_none() => {
                        let val = tag.value.to_string();
                        if !val.trim().is_empty() {
                            artist = Some(val.trim().to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(rev) = probed.format.metadata().current() {
        for tag in rev.tags() {
            if intro_ms.is_none() {
                intro_ms = parse_intro_tag(&tag.key, &tag.value.to_string());
            }
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) if title.is_none() => {
                    let val = tag.value.to_string();
                    if !val.trim().is_empty() {
                        title = Some(val.trim().to_string());
                    }
                }
                Some(StandardTagKey::Artist) if artist.is_none() => {
                    let val = tag.value.to_string();
                    if !val.trim().is_empty() {
                        artist = Some(val.trim().to_string());
                    }
                }
                _ => {}
            }
        }
    }

    let track = probed
        .format
        .default_track()
        .ok_or_else(|| "El archivo no contiene pista de audio".to_string())?;

    let track_id = track.id;
    let params = &track.codec_params;
    let sample_rate = params.sample_rate.unwrap_or(44100).max(1);
    let channels = params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(2)
        .max(1);

    let mut duration_ms = if let (Some(n_frames), Some(tb)) = (params.n_frames, params.time_base) {
        let t = tb.calc_time(n_frames);
        t.seconds * 1000 + (t.frac * 1000.0).round() as u64
    } else if let Some(n_frames) = params.n_frames {
        (n_frames * 1000) / u64::from(sample_rate)
    } else {
        0
    };

    if duration_ms == 0 && file_len > 0 {
        let mut sampled_packets = 0u32;
        let mut sampled_bytes = 0u64;
        let mut sampled_frames = 0u64;

        while sampled_packets < 32 {
            match probed.format.next_packet() {
                Ok(pkt) => {
                    if pkt.track_id() == track_id {
                        sampled_bytes += pkt.buf().len() as u64;
                        sampled_frames += pkt.dur();
                        sampled_packets += 1;
                    }
                }
                Err(_) => break,
            }
        }

        if sampled_bytes > 0 && sampled_frames > 0 {
            let estimated_total_frames =
                ((file_len as u128) * (sampled_frames as u128)) / (sampled_bytes as u128);
            duration_ms = ((estimated_total_frames as u64) * 1000) / u64::from(sample_rate);
        }
    }

    Ok(TrackProbeInfo {
        duration_ms,
        sample_rate,
        channels,
        title,
        artist,
        intro_ms,
    })
}

/// Obtiene de forma perezosa (sin decodificar paquetes) la duración en milisegundos de un archivo.
pub fn probe_duration_ms(path: &Path) -> u64 {
    probe_track_metadata(path)
        .map(|info| info.duration_ms)
        .unwrap_or(0)
}

/// Extrae picos de amplitud en streaming (`bucket_count` columnas normalizadas `0.0..=1.0`)
/// sin retener el buffer PCM completo en memoria, para el Editor Visual de Puntos de Mezcla (Canvas).
pub fn extract_waveform_peaks(path: &Path, bucket_count: usize) -> Result<WaveformPeaks, String> {
    let buckets = bucket_count.clamp(64, 2048);
    let (mut format, track_id, codec_params, duration_hint_ms) = open_format_reader(path)?;

    let src_sample_rate = codec_params.sample_rate.unwrap_or(44100).max(1);
    let src_channels = codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("No se pudo crear el decodificador Symphonia: {}", e))?;

    let estimated_total_frames = if duration_hint_ms > 0 {
        ((duration_hint_ms * u64::from(src_sample_rate)) / 1000).max(1) as usize
    } else {
        (src_sample_rate as usize) * 180
    };
    let frames_per_bucket = (estimated_total_frames / buckets).max(1);

    let mut raw_peaks: Vec<f32> = Vec::with_capacity(buckets + 64);
    let mut current_bucket_peak = 0.0_f32;
    let mut frames_in_bucket = 0usize;
    let mut total_decoded_frames = 0u64;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let spec = *audio_buf.spec();
                let duration = audio_buf.capacity() as u64;
                let mut sample_buf = SampleBuffer::<f32>::new(duration, spec);
                sample_buf.copy_interleaved_ref(audio_buf);
                let samples = sample_buf.samples();
                for frame in samples.chunks(src_channels) {
                    let mut frame_peak = 0.0_f32;
                    for &s in frame {
                        let a = s.abs();
                        if a > frame_peak {
                            frame_peak = a;
                        }
                    }
                    if frame_peak > current_bucket_peak {
                        current_bucket_peak = frame_peak;
                    }
                    frames_in_bucket += 1;
                    total_decoded_frames += 1;
                    if frames_in_bucket >= frames_per_bucket {
                        raw_peaks.push(current_bucket_peak);
                        current_bucket_peak = 0.0;
                        frames_in_bucket = 0;
                    }
                }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }

    if frames_in_bucket > 0 || raw_peaks.is_empty() {
        raw_peaks.push(current_bucket_peak);
    }

    // Re-muestrear exactamente a `buckets` columnas y normalizar a 0.0..=1.0
    let max_peak = raw_peaks
        .iter()
        .copied()
        .fold(0.0_f32, f32::max)
        .max(0.05);
    let mut normalized = Vec::with_capacity(buckets);
    let raw_len = raw_peaks.len().max(1);
    for i in 0..buckets {
        let start = (i * raw_len) / buckets;
        let end = (((i + 1) * raw_len) / buckets).max(start + 1).min(raw_len);
        let slice_max = raw_peaks[start..end]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        normalized.push((slice_max / max_peak).clamp(0.0, 1.0));
    }

    let exact_duration_ms = if total_decoded_frames > 0 {
        (total_decoded_frames * 1000) / u64::from(src_sample_rate)
    } else {
        duration_hint_ms
    };

    Ok(WaveformPeaks {
        path: path.to_string_lossy().to_string(),
        duration_ms: exact_duration_ms,
        sample_rate: src_sample_rate,
        channels: src_channels as u16,
        peaks: normalized,
    })
}

fn open_format_reader(
    path: &Path,
) -> Result<(Box<dyn FormatReader>, u32, CodecParameters, u64), String> {
    let duration_hint_ms = probe_duration_ms(path);

    let file = File::open(path)
        .map_err(|e| format!("No se pudo abrir el archivo '{}': {}", path.display(), e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Formato no soportado por Symphonia: {}", e))?;

    let format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "El archivo no contiene ninguna pista de audio".to_string())?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let dur_ms = duration_ms_or_hint(&codec_params, duration_hint_ms);

    Ok((format, track_id, codec_params, dur_ms))
}

fn duration_ms_or_hint(params: &CodecParameters, hint_ms: u64) -> u64 {
    if hint_ms > 0 {
        return hint_ms;
    }
    let sr = params.sample_rate.unwrap_or(44100).max(1);
    if let Some(n_frames) = params.n_frames {
        return (n_frames * 1000) / u64::from(sr);
    }
    0
}

fn spawn_jit_streaming_decoder(
    path: &Path,
    dst_rate: u32,
    dst_channels: usize,
    ring_capacity: usize,
    decoded_frames_counter: Option<Arc<AtomicU64>>,
) -> Result<(Arc<SpscRingBuffer>, u64), String> {
    spawn_jit_streaming_decoder_from(
        path,
        dst_rate,
        dst_channels,
        ring_capacity,
        decoded_frames_counter,
        0,
    )
}

fn spawn_jit_streaming_decoder_from(
    path: &Path,
    dst_rate: u32,
    dst_channels: usize,
    ring_capacity: usize,
    decoded_frames_counter: Option<Arc<AtomicU64>>,
    start_offset_ms: u64,
) -> Result<(Arc<SpscRingBuffer>, u64), String> {
    let (mut format, track_id, codec_params, duration_ms) = open_format_reader(path)?;

    let src_sample_rate = codec_params.sample_rate.unwrap_or(44100).max(1);
    let src_channels = codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("No se pudo crear el decodificador Symphonia: {}", e))?;

    let ring = Arc::new(SpscRingBuffer::new(ring_capacity));
    if let Some(ref counter) = decoded_frames_counter {
        counter.store(0, Ordering::Release);
    }

    let mut preloaded_packets = 0usize;
    let mut reached_eof_early = false;

    let effective_offset_ms = if duration_ms > 0 {
        start_offset_ms.min(duration_ms.saturating_sub(10))
    } else {
        start_offset_ms
    };

    if effective_offset_ms > 0 {
        let seek_time = symphonia::core::units::Time::new(
            effective_offset_ms / 1000,
            (effective_offset_ms % 1000) as f64 / 1000.0,
        );
        let seek_ok = format
            .seek(
                symphonia::core::formats::SeekMode::Accurate,
                symphonia::core::formats::SeekTo::Time {
                    time: seek_time,
                    track_id: Some(track_id),
                },
            )
            .is_ok();
        if seek_ok {
            decoder.reset();
        } else {
            // Fallback: avanzar decodificando y descartando muestras hasta llegar a effective_offset_ms
            let target_skip_frames =
                ((effective_offset_ms * u64::from(dst_rate)) / 1000) as usize;
            let mut skipped_frames = 0usize;
            while skipped_frames < target_skip_frames {
                match decode_next_chunk(
                    &mut *format,
                    &mut *decoder,
                    track_id,
                    src_sample_rate,
                    src_channels,
                    dst_rate,
                    dst_channels,
                ) {
                    ChunkResult::Samples(chunk) => {
                        let chunk_frames = chunk.len() / dst_channels.max(1);
                        if skipped_frames + chunk_frames <= target_skip_frames {
                            skipped_frames += chunk_frames;
                        } else {
                            let remain_skip = target_skip_frames - skipped_frames;
                            let start_sample = remain_skip * dst_channels.max(1);
                            if start_sample < chunk.len() {
                                let leftover = &chunk[start_sample..];
                                let frames = (leftover.len() / dst_channels.max(1)) as u64;
                                if let Some(ref counter) = decoded_frames_counter {
                                    counter.fetch_add(frames, Ordering::Relaxed);
                                }
                                let _ = ring.push_slice(leftover);
                                preloaded_packets += 1;
                            }
                            skipped_frames = target_skip_frames;
                        }
                    }
                    ChunkResult::Continue => continue,
                    ChunkResult::Eof => {
                        reached_eof_early = true;
                        ring.mark_eof();
                        break;
                    }
                }
            }
        }
    }

    while preloaded_packets < 2 && !reached_eof_early {
        match decode_next_chunk(
            &mut *format,
            &mut *decoder,
            track_id,
            src_sample_rate,
            src_channels,
            dst_rate,
            dst_channels,
        ) {
            ChunkResult::Samples(chunk) => {
                if !chunk.is_empty() {
                    let frames = (chunk.len() / dst_channels.max(1)) as u64;
                    if let Some(ref counter) = decoded_frames_counter {
                        counter.fetch_add(frames, Ordering::Relaxed);
                    }
                    let _ = ring.push_slice(&chunk);
                    preloaded_packets += 1;
                }
            }
            ChunkResult::Continue => continue,
            ChunkResult::Eof => {
                reached_eof_early = true;
                ring.mark_eof();
                break;
            }
        }
    }

    if !reached_eof_early {
        let ring_producer = Arc::clone(&ring);
        std::thread::Builder::new()
            .name("perpetua-jit-decoder".to_string())
            .spawn(move || {
                while !ring_producer.is_cancelled() {
                    match decode_next_chunk(
                        &mut *format,
                        &mut *decoder,
                        track_id,
                        src_sample_rate,
                        src_channels,
                        dst_rate,
                        dst_channels,
                    ) {
                        ChunkResult::Samples(chunk) => {
                            if chunk.is_empty() {
                                continue;
                            }
                            let frames = (chunk.len() / dst_channels.max(1)) as u64;
                            if let Some(ref counter) = decoded_frames_counter {
                                counter.fetch_add(frames, Ordering::Relaxed);
                            }

                            let mut offset = 0;
                            while offset < chunk.len() && !ring_producer.is_cancelled() {
                                let written = ring_producer.push_slice(&chunk[offset..]);
                                if written > 0 {
                                    offset += written;
                                } else {
                                    std::thread::sleep(Duration::from_millis(4));
                                }
                            }
                        }
                        ChunkResult::Continue => continue,
                        ChunkResult::Eof => {
                            ring_producer.mark_eof();
                            break;
                        }
                    }
                }
            })
            .map_err(|e| format!("No se pudo lanzar hilo decodificador JIT: {}", e))?;
    }

    Ok((ring, duration_ms))
}

enum ChunkResult {
    Samples(Vec<f32>),
    Continue,
    Eof,
}

fn decode_next_chunk(
    format: &mut dyn FormatReader,
    decoder: &mut dyn symphonia::core::codecs::Decoder,
    track_id: u32,
    src_rate: u32,
    src_channels: usize,
    dst_rate: u32,
    dst_channels: usize,
) -> ChunkResult {
    let packet = match format.next_packet() {
        Ok(p) => p,
        Err(SymphoniaError::IoError(ref err))
            if err.kind() == std::io::ErrorKind::UnexpectedEof =>
        {
            return ChunkResult::Eof;
        }
        Err(SymphoniaError::ResetRequired) => return ChunkResult::Eof,
        Err(_) => return ChunkResult::Eof,
    };

    if packet.track_id() != track_id {
        return ChunkResult::Continue;
    }

    match decoder.decode(&packet) {
        Ok(audio_buf) => {
            let spec = *audio_buf.spec();
            let duration = audio_buf.capacity() as u64;
            let mut sample_buf = SampleBuffer::<f32>::new(duration, spec);
            sample_buf.copy_interleaved_ref(audio_buf);
            let adapted = adapt_samples_to_device(
                sample_buf.samples(),
                src_rate,
                src_channels,
                dst_rate,
                dst_channels,
            );
            ChunkResult::Samples(adapted)
        }
        Err(SymphoniaError::DecodeError(_)) => ChunkResult::Continue,
        Err(_) => ChunkResult::Eof,
    }
}

pub fn decode_file_with_symphonia(path: &Path) -> Result<(Vec<f32>, u32, usize), String> {
    let (mut format, track_id, codec_params, _) = open_format_reader(path)?;
    let src_sample_rate = codec_params.sample_rate.unwrap_or(44100);
    let src_channels = codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("No se pudo crear el decodificador Symphonia: {}", e))?;

    let mut samples_f32: Vec<f32> = Vec::new();
    loop {
        match decode_next_chunk(
            &mut *format,
            &mut *decoder,
            track_id,
            src_sample_rate,
            src_channels,
            src_sample_rate,
            src_channels,
        ) {
            ChunkResult::Samples(chunk) => samples_f32.extend_from_slice(&chunk),
            ChunkResult::Continue => continue,
            ChunkResult::Eof => break,
        }
    }

    if samples_f32.is_empty() {
        return Err("Symphonia no decodificó ninguna muestra de audio".to_string());
    }
    Ok((samples_f32, src_sample_rate, src_channels))
}

fn adapt_samples_to_device(
    src_samples: &[f32],
    src_rate: u32,
    src_channels: usize,
    dst_rate: u32,
    dst_channels: usize,
) -> Vec<f32> {
    let src_frames = src_samples.len() / src_channels.max(1);
    if src_frames == 0 {
        return Vec::new();
    }

    if src_rate == dst_rate && src_channels == dst_channels {
        return src_samples.to_vec();
    }

    let ratio = src_rate as f64 / dst_rate.max(1) as f64;
    let dst_frames = ((src_frames as f64) / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(dst_frames * dst_channels);

    for dst_frame in 0..dst_frames {
        let src_pos = (dst_frame as f64 * ratio).min((src_frames - 1) as f64);
        let idx0 = src_pos.floor() as usize;
        let idx1 = (idx0 + 1).min(src_frames - 1);
        let frac = (src_pos - idx0 as f64) as f32;

        for ch in 0..dst_channels {
            let src_ch = ch.min(src_channels - 1);
            let s0 = src_samples[idx0 * src_channels + src_ch];
            let s1 = src_samples[idx1 * src_channels + src_ch];
            out.push(s0 + (s1 - s0) * frac);
        }
    }
    out
}

fn mask_to_slots(mask: u16) -> Vec<usize> {
    let mut slots = Vec::new();
    for slot in 1..=9 {
        if (mask & (1 << slot)) != 0 {
            slots.push(slot);
        }
    }
    slots
}

/// Mezclador multicanal y multitarjeta en tiempo real basado en `cpal` + `SpscRingBuffer` + `symphonia` JIT.
/// Soporta dos salidas independientes (`Main / Aire` y `Cue / Preescucha`) con fallback automático a tarjeta única.
pub struct AudioPlayer {
    cmd_tx: mpsc::Sender<AudioThreadCommand>,
    status: Arc<Mutex<PlayerStatus>>,
    played_frames: Arc<AtomicU64>,
    decoded_frames: Arc<AtomicU64>,
    is_playing_flag: Arc<AtomicBool>,
    is_crossfading_flag: Arc<AtomicBool>,
    is_ducking_flag: Arc<AtomicBool>,
    is_paused_by_event_flag: Arc<AtomicBool>,
    overlay_is_playing_flag: Arc<AtomicBool>,
    sweeper_armed_flag: Arc<AtomicBool>,
    sweeper_is_playing_flag: Arc<AtomicBool>,
    active_jingles_mask: Arc<AtomicU16>,
    cue_played_frames: Arc<AtomicU64>,
    cue_sample_rate: Arc<AtomicU32>,
    cue_is_playing_flag: Arc<AtomicBool>,
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPlayer {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AudioThreadCommand>();
        let status = Arc::new(Mutex::new(PlayerStatus::default()));
        let played_frames = Arc::new(AtomicU64::new(0));
        let decoded_frames = Arc::new(AtomicU64::new(0));
        let is_playing_flag = Arc::new(AtomicBool::new(false));
        let is_crossfading_flag = Arc::new(AtomicBool::new(false));
        let is_ducking_flag = Arc::new(AtomicBool::new(false));
        let is_paused_by_event_flag = Arc::new(AtomicBool::new(false));
        let overlay_is_playing_flag = Arc::new(AtomicBool::new(false));
        let sweeper_armed_flag = Arc::new(AtomicBool::new(false));
        let sweeper_is_playing_flag = Arc::new(AtomicBool::new(false));
        let active_jingles_mask = Arc::new(AtomicU16::new(0));
        let cue_played_frames = Arc::new(AtomicU64::new(0));
        let cue_sample_rate = Arc::new(AtomicU32::new(44100));
        let cue_is_playing_flag = Arc::new(AtomicBool::new(false));

        let status_clone = Arc::clone(&status);
        let played_frames_clone = Arc::clone(&played_frames);
        let decoded_frames_clone = Arc::clone(&decoded_frames);
        let is_playing_clone = Arc::clone(&is_playing_flag);
        let is_crossfading_clone = Arc::clone(&is_crossfading_flag);
        let is_ducking_clone = Arc::clone(&is_ducking_flag);
        let is_paused_by_event_clone = Arc::clone(&is_paused_by_event_flag);
        let overlay_is_playing_clone = Arc::clone(&overlay_is_playing_flag);
        let sweeper_armed_clone = Arc::clone(&sweeper_armed_flag);
        let sweeper_is_playing_clone = Arc::clone(&sweeper_is_playing_flag);
        let active_jingles_clone = Arc::clone(&active_jingles_mask);
        let cue_played_frames_clone = Arc::clone(&cue_played_frames);
        let cue_sample_rate_clone = Arc::clone(&cue_sample_rate);
        let cue_is_playing_clone = Arc::clone(&cue_is_playing_flag);

        std::thread::Builder::new()
            .name("perpetua-cpal-audio".to_string())
            .spawn(move || {
                let mut air_selected = "Default Output".to_string();
                let mut cue_selected = "Default Output".to_string();
                let mut hw_stream: Option<ActiveHardwareStream> = None;
                let mut cue_hw_stream: Option<ActiveCueHardwareStream> = None;

                while let Ok(cmd) = cmd_rx.recv() {
                    match cmd {
                        AudioThreadCommand::Play {
                            path,
                            overlap_ms,
                            reply,
                        } => {
                            let need_reinit = hw_stream
                                .as_ref()
                                .map(|s| s.requested_name != air_selected)
                                .unwrap_or(true);
                            if need_reinit {
                                match init_hardware_stream(
                                    &air_selected,
                                    &played_frames_clone,
                                    &is_playing_clone,
                                    &is_crossfading_clone,
                                    &is_ducking_clone,
                                    &is_paused_by_event_clone,
                                    &overlay_is_playing_clone,
                                    &sweeper_armed_clone,
                                    &sweeper_is_playing_clone,
                                    &active_jingles_clone,
                                    &cue_played_frames_clone,
                                    &cue_is_playing_clone,
                                ) {
                                    Ok(s) => hw_stream = Some(s),
                                    Err(err_msg) => {
                                        if let Ok(mut guard) = status_clone.lock() {
                                            guard.is_playing = false;
                                            guard.last_error = Some(err_msg.clone());
                                        }
                                        let _ = reply.send(Err(err_msg));
                                        continue;
                                    }
                                }
                            }

                            let hw = hw_stream.as_ref().unwrap();
                            let stream_res = spawn_jit_streaming_decoder(
                                &path,
                                hw.sample_rate,
                                hw.channels,
                                MAIN_RING_CAPACITY,
                                Some(Arc::clone(&decoded_frames_clone)),
                            );

                            match stream_res {
                                Ok((ring_arc, duration_ms)) => {
                                    let effective_fade_ms = overlap_ms.max(5);
                                    let fade_frames = ((effective_fade_ms
                                        * u64::from(hw.sample_rate))
                                        / 1000)
                                        .max(1)
                                        as usize;

                                    let mut crossfading_now = false;
                                    if let Ok(mut decks) = hw.mixer.lock() {
                                        decks.sweeper = None;
                                        decks.sweeper_duck_gain = 1.0;

                                        let had_active_primary = decks
                                            .primary
                                            .as_ref()
                                            .map(|p| !p.ring.is_finished())
                                            .unwrap_or(false);

                                        if let Some(prev_primary) = decks.primary.take() {
                                            if !prev_primary.ring.is_finished() {
                                                let init_gain = prev_primary.current_gain();
                                                let outgoing_ring = Arc::clone(&prev_primary.ring);
                                                std::mem::forget(prev_primary);
                                                decks.outgoing = Some(OutgoingDeck {
                                                    ring: outgoing_ring,
                                                    initial_gain: init_gain,
                                                    fade_out_elapsed_frames: 0,
                                                    fade_out_total_frames: fade_frames,
                                                });
                                                crossfading_now = overlap_ms > 0;
                                            }
                                        }

                                        decks.primary = Some(PrimaryDeck {
                                            ring: ring_arc,
                                            played_frames: 0,
                                            fade_in_elapsed_frames: 0,
                                            fade_in_total_frames: if had_active_primary
                                                && overlap_ms > 0
                                            {
                                                fade_frames
                                            } else {
                                                ((5 * u64::from(hw.sample_rate)) / 1000).max(1)
                                                    as usize
                                            },
                                        });
                                    }

                                    played_frames_clone.store(0, Ordering::SeqCst);
                                    is_playing_clone.store(true, Ordering::SeqCst);
                                    is_crossfading_clone.store(crossfading_now, Ordering::SeqCst);
                                    sweeper_armed_clone.store(false, Ordering::SeqCst);
                                    sweeper_is_playing_clone.store(false, Ordering::SeqCst);

                                    let st = if let Ok(mut guard) = status_clone.lock() {
                                        guard.is_playing = true;
                                        guard.is_crossfading = crossfading_now;
                                        guard.sweeper_armed = false;
                                        guard.sweeper_is_playing = false;
                                        guard.sweeper_file = None;
                                        guard.sweeper_intro_ms = 0;
                                        guard.sweeper_trigger_ms = 0;
                                        guard.sweeper_duration_ms = 0;
                                        guard.current_file =
                                            Some(path.to_string_lossy().to_string());
                                        guard.elapsed_ms = 0;
                                        guard.duration_ms = duration_ms;
                                        guard.device_name = hw.device_name.clone();
                                        guard.sample_rate = hw.sample_rate;
                                        guard.channels = hw.channels as u16;
                                        guard.decoded_frames =
                                            decoded_frames_clone.load(Ordering::Acquire);
                                        guard.last_error = None;
                                        guard.active_jingle_slots = mask_to_slots(
                                            active_jingles_clone.load(Ordering::SeqCst),
                                        );
                                        guard.clone()
                                    } else {
                                        PlayerStatus::default()
                                    };

                                    let _ = reply.send(Ok(st));
                                }
                                Err(err_msg) => {
                                    if let Ok(mut guard) = status_clone.lock() {
                                        guard.last_error = Some(err_msg.clone());
                                    }
                                    let _ = reply.send(Err(err_msg));
                                }
                            }
                        }
                        AudioThreadCommand::Stop { reply } => {
                            if let Some(hw) = hw_stream.as_ref() {
                                let anti_click_frames =
                                    ((8 * u64::from(hw.sample_rate)) / 1000).max(1) as usize;
                                if let Ok(mut decks) = hw.mixer.lock() {
                                    decks.event_overlays.clear();
                                    decks.sweeper = None;
                                    decks.sweeper_duck_gain = 1.0;
                                    decks.duck_gain = 1.0;
                                    decks.pause_envelope_gain = 1.0;
                                    if let Some(prev_primary) = decks.primary.take() {
                                        if !prev_primary.ring.is_finished() {
                                            let init_gain = prev_primary.current_gain();
                                            let outgoing_ring = Arc::clone(&prev_primary.ring);
                                            std::mem::forget(prev_primary);
                                            decks.outgoing = Some(OutgoingDeck {
                                                ring: outgoing_ring,
                                                initial_gain: init_gain,
                                                fade_out_elapsed_frames: 0,
                                                fade_out_total_frames: anti_click_frames,
                                            });
                                        }
                                    }
                                }
                            }
                            is_playing_clone.store(false, Ordering::SeqCst);
                            is_crossfading_clone.store(false, Ordering::SeqCst);
                            is_ducking_clone.store(false, Ordering::SeqCst);
                            is_paused_by_event_clone.store(false, Ordering::SeqCst);
                            overlay_is_playing_clone.store(false, Ordering::SeqCst);
                            sweeper_armed_clone.store(false, Ordering::SeqCst);
                            sweeper_is_playing_clone.store(false, Ordering::SeqCst);
                            played_frames_clone.store(0, Ordering::SeqCst);

                            let st = if let Ok(mut guard) = status_clone.lock() {
                                guard.is_playing = false;
                                guard.is_crossfading = false;
                                guard.is_ducking = false;
                                guard.is_paused_by_event = false;
                                guard.overlay_is_playing = false;
                                guard.overlay_file = None;
                                guard.sweeper_armed = false;
                                guard.sweeper_is_playing = false;
                                guard.sweeper_file = None;
                                guard.sweeper_intro_ms = 0;
                                guard.sweeper_trigger_ms = 0;
                                guard.sweeper_duration_ms = 0;
                                guard.elapsed_ms = 0;
                                guard.decoded_frames =
                                    decoded_frames_clone.load(Ordering::Acquire);
                                guard.active_jingle_slots = mask_to_slots(
                                    active_jingles_clone.load(Ordering::SeqCst),
                                );
                                guard.clone()
                            } else {
                                PlayerStatus::default()
                            };
                            let _ = reply.send(st);
                        }
                        AudioThreadCommand::PlayJingle { slot, path, reply } => {
                            if hw_stream.is_none() {
                                match init_hardware_stream(
                                    &air_selected,
                                    &played_frames_clone,
                                    &is_playing_clone,
                                    &is_crossfading_clone,
                                    &is_ducking_clone,
                                    &is_paused_by_event_clone,
                                    &overlay_is_playing_clone,
                                    &sweeper_armed_clone,
                                    &sweeper_is_playing_clone,
                                    &active_jingles_clone,
                                    &cue_played_frames_clone,
                                    &cue_is_playing_clone,
                                ) {
                                    Ok(s) => hw_stream = Some(s),
                                    Err(err_msg) => {
                                        let _ = reply.send(Err(err_msg));
                                        continue;
                                    }
                                }
                            }

                            let hw = hw_stream.as_ref().unwrap();
                            let stream_res = spawn_jit_streaming_decoder(
                                &path,
                                hw.sample_rate,
                                hw.channels,
                                JINGLE_RING_CAPACITY,
                                None,
                            );

                            match stream_res {
                                Ok((jingle_ring, _)) => {
                                    let fade_in_frames =
                                        ((5 * u64::from(hw.sample_rate)) / 1000).max(1) as usize;
                                    if let Ok(mut decks) = hw.mixer.lock() {
                                        decks.jingles.retain(|j| j.slot != slot);
                                        decks.jingles.push(JingleVoice {
                                            slot,
                                            ring: jingle_ring,
                                            fade_in_elapsed_frames: 0,
                                            fade_in_total_frames: fade_in_frames,
                                        });
                                        let mut mask = 0u16;
                                        for j in &decks.jingles {
                                            if (1..=9).contains(&j.slot) {
                                                mask |= 1 << j.slot;
                                            }
                                        }
                                        active_jingles_clone.store(mask, Ordering::SeqCst);
                                    }

                                    let mut st = status_clone
                                        .lock()
                                        .map(|g| g.clone())
                                        .unwrap_or_default();
                                    if st.sample_rate == 0 {
                                        st.device_name = hw.device_name.clone();
                                        st.sample_rate = hw.sample_rate;
                                        st.channels = hw.channels as u16;
                                    }
                                    st.is_playing = is_playing_clone.load(Ordering::SeqCst);
                                    st.is_crossfading =
                                        is_crossfading_clone.load(Ordering::SeqCst);
                                    st.decoded_frames =
                                        decoded_frames_clone.load(Ordering::Acquire);
                                    let frames = played_frames_clone.load(Ordering::SeqCst);
                                    if st.sample_rate > 0 {
                                        st.elapsed_ms =
                                            (frames * 1000) / u64::from(st.sample_rate);
                                    }
                                    st.active_jingle_slots = mask_to_slots(
                                        active_jingles_clone.load(Ordering::SeqCst),
                                    );
                                    let _ = reply.send(Ok(st));
                                }
                                Err(err_msg) => {
                                    let _ = reply.send(Err(err_msg));
                                }
                            }
                        }
                        AudioThreadCommand::PlayEventOverlay { paths, mode, reply } => {
                            if paths.is_empty() {
                                let _ = reply.send(Err(
                                    "No se proporcionaron archivos para el evento superpuesto"
                                        .to_string(),
                                ));
                                continue;
                            }
                            if hw_stream.is_none() {
                                match init_hardware_stream(
                                    &air_selected,
                                    &played_frames_clone,
                                    &is_playing_clone,
                                    &is_crossfading_clone,
                                    &is_ducking_clone,
                                    &is_paused_by_event_clone,
                                    &overlay_is_playing_clone,
                                    &sweeper_armed_clone,
                                    &sweeper_is_playing_clone,
                                    &active_jingles_clone,
                                    &cue_played_frames_clone,
                                    &cue_is_playing_clone,
                                ) {
                                    Ok(s) => hw_stream = Some(s),
                                    Err(err_msg) => {
                                        let _ = reply.send(Err(err_msg));
                                        continue;
                                    }
                                }
                            }

                            let hw = hw_stream.as_ref().unwrap();
                            let fade_in_frames =
                                ((5 * u64::from(hw.sample_rate)) / 1000).max(1) as usize;
                            let mut voices = Vec::with_capacity(paths.len());
                            let mut decode_err: Option<String> = None;

                            for (idx, p) in paths.iter().enumerate() {
                                match spawn_jit_streaming_decoder(
                                    p,
                                    hw.sample_rate,
                                    hw.channels,
                                    JINGLE_RING_CAPACITY,
                                    None,
                                ) {
                                    Ok((ring, _)) => {
                                        voices.push(OverlayVoice {
                                            ring,
                                            fade_in_elapsed_frames: 0,
                                            fade_in_total_frames: if idx == 0 {
                                                fade_in_frames
                                            } else {
                                                1
                                            },
                                        });
                                    }
                                    Err(e) => {
                                        decode_err = Some(e);
                                        break;
                                    }
                                }
                            }

                            if let Some(err_msg) = decode_err {
                                let _ = reply.send(Err(err_msg));
                                continue;
                            }

                            if let Ok(mut decks) = hw.mixer.lock() {
                                decks.event_overlays.clear();
                                decks.event_overlay_mode = mode;
                                for v in voices {
                                    decks.event_overlays.push_back(v);
                                }
                            }

                            let is_duck = mode == EventOverlayMode::Ducking;
                            let is_pause = mode == EventOverlayMode::PauseResume;
                            overlay_is_playing_clone.store(true, Ordering::SeqCst);
                            is_ducking_clone.store(is_duck, Ordering::SeqCst);
                            is_paused_by_event_clone.store(is_pause, Ordering::SeqCst);

                            let label = paths
                                .first()
                                .map(|p| p.to_string_lossy().to_string());

                            let st = if let Ok(mut guard) = status_clone.lock() {
                                if guard.sample_rate == 0 {
                                    guard.device_name = hw.device_name.clone();
                                    guard.sample_rate = hw.sample_rate;
                                    guard.channels = hw.channels as u16;
                                }
                                guard.overlay_is_playing = true;
                                guard.is_ducking = is_duck;
                                guard.is_paused_by_event = is_pause;
                                guard.overlay_file = label;
                                guard.last_error = None;
                                guard.clone()
                            } else {
                                PlayerStatus::default()
                            };
                            let _ = reply.send(Ok(st));
                        }
                        AudioThreadCommand::ScheduleSweeper { spec, reply } => {
                            let Some(sw_spec) = spec else {
                                if let Some(ref hw) = hw_stream {
                                    if let Ok(mut decks) = hw.mixer.lock() {
                                        decks.sweeper = None;
                                        decks.sweeper_duck_gain = 1.0;
                                    }
                                }
                                sweeper_armed_clone.store(false, Ordering::SeqCst);
                                sweeper_is_playing_clone.store(false, Ordering::SeqCst);
                                let st = if let Ok(mut guard) = status_clone.lock() {
                                    guard.sweeper_armed = false;
                                    guard.sweeper_is_playing = false;
                                    guard.sweeper_file = None;
                                    guard.sweeper_intro_ms = 0;
                                    guard.sweeper_trigger_ms = 0;
                                    guard.sweeper_duration_ms = 0;
                                    guard.clone()
                                } else {
                                    PlayerStatus::default()
                                };
                                let _ = reply.send(Ok(st));
                                continue;
                            };

                            if hw_stream.is_none() {
                                match init_hardware_stream(
                                    &air_selected,
                                    &played_frames_clone,
                                    &is_playing_clone,
                                    &is_crossfading_clone,
                                    &is_ducking_clone,
                                    &is_paused_by_event_clone,
                                    &overlay_is_playing_clone,
                                    &sweeper_armed_clone,
                                    &sweeper_is_playing_clone,
                                    &active_jingles_clone,
                                    &cue_played_frames_clone,
                                    &cue_is_playing_clone,
                                ) {
                                    Ok(s) => hw_stream = Some(s),
                                    Err(err_msg) => {
                                        let _ = reply.send(Err(err_msg));
                                        continue;
                                    }
                                }
                            }

                            let hw = hw_stream.as_ref().unwrap();
                            match spawn_jit_streaming_decoder(
                                &sw_spec.sweeper_path,
                                hw.sample_rate,
                                hw.channels,
                                JINGLE_RING_CAPACITY,
                                None,
                            ) {
                                Ok((sweeper_ring, probed_sw_dur)) => {
                                    let effective_sw_dur = if sw_spec.sweeper_duration_ms > 0 {
                                        sw_spec.sweeper_duration_ms
                                    } else {
                                        probed_sw_dur
                                    };
                                    let trigger_at_ms = compute_sweeper_trigger_ms(
                                        sw_spec.intro_ms,
                                        effective_sw_dur,
                                    );
                                    let trigger_frame = ((trigger_at_ms
                                        * u64::from(hw.sample_rate))
                                        / 1000)
                                        as usize;
                                    let fade_in_frames =
                                        ((5 * u64::from(hw.sample_rate)) / 1000).max(1) as usize;

                                    let mut already_started = false;
                                    if let Ok(mut decks) = hw.mixer.lock() {
                                        let current_frames = decks
                                            .primary
                                            .as_ref()
                                            .map(|p| p.played_frames)
                                            .unwrap_or(0);
                                        already_started = current_frames >= trigger_frame;
                                        decks.sweeper = Some(ScheduledSweeperVoice {
                                            ring: sweeper_ring,
                                            trigger_frame,
                                            started: already_started,
                                            fade_in_elapsed_frames: 0,
                                            fade_in_total_frames: fade_in_frames,
                                        });
                                    }

                                    sweeper_armed_clone.store(true, Ordering::SeqCst);
                                    sweeper_is_playing_clone
                                        .store(already_started, Ordering::SeqCst);

                                    let st = if let Ok(mut guard) = status_clone.lock() {
                                        guard.sweeper_armed = true;
                                        guard.sweeper_is_playing = already_started;
                                        guard.sweeper_file = Some(
                                            sw_spec.sweeper_path.to_string_lossy().to_string(),
                                        );
                                        guard.sweeper_intro_ms = sw_spec.intro_ms;
                                        guard.sweeper_trigger_ms = trigger_at_ms;
                                        guard.sweeper_duration_ms = effective_sw_dur;
                                        guard.last_error = None;
                                        guard.clone()
                                    } else {
                                        PlayerStatus::default()
                                    };
                                    let _ = reply.send(Ok(st));
                                }
                                Err(err_msg) => {
                                    let _ = reply.send(Err(err_msg));
                                }
                            }
                        }
                        AudioThreadCommand::PlayCue {
                            path,
                            start_offset_ms,
                            reply,
                        } => {
                            let resolved_path = match path {
                                Some(p) => p,
                                None => {
                                    let existing = status_clone
                                        .lock()
                                        .ok()
                                        .and_then(|g| g.cue_file.clone())
                                        .map(PathBuf::from);
                                    match existing {
                                        Some(p) => p,
                                        None => {
                                            let _ = reply.send(Err(
                                                "No hay ninguna pista activa en el Cue Deck para hacer Seek"
                                                    .to_string(),
                                            ));
                                            continue;
                                        }
                                    }
                                }
                            };

                            // Determinar si Cue usa una segunda tarjeta independiente o comparte la tarjeta del Aire
                            let same_card_requested = cue_selected == air_selected;
                            let mut use_shared_main_stream = same_card_requested;

                            if !same_card_requested {
                                let need_cue_init = cue_hw_stream
                                    .as_ref()
                                    .map(|s| s.requested_name != cue_selected)
                                    .unwrap_or(true);
                                if need_cue_init {
                                    match init_dedicated_cue_stream(
                                        &cue_selected,
                                        &cue_played_frames_clone,
                                        &cue_is_playing_clone,
                                    ) {
                                        Ok(s) => {
                                            // Si el nombre real resuelto coincide con el stream de Aire activo,
                                            // comprobamos si ambos corren bien por separado
                                            cue_hw_stream = Some(s);
                                        }
                                        Err(_) => {
                                            // Fallback automático a la tarjeta principal si falla abrir segunda tarjeta
                                            cue_hw_stream = None;
                                            use_shared_main_stream = true;
                                        }
                                    }
                                }
                            } else {
                                cue_hw_stream = None;
                            }

                            if use_shared_main_stream && hw_stream.is_none() {
                                match init_hardware_stream(
                                    &air_selected,
                                    &played_frames_clone,
                                    &is_playing_clone,
                                    &is_crossfading_clone,
                                    &is_ducking_clone,
                                    &is_paused_by_event_clone,
                                    &overlay_is_playing_clone,
                                    &sweeper_armed_clone,
                                    &sweeper_is_playing_clone,
                                    &active_jingles_clone,
                                    &cue_played_frames_clone,
                                    &cue_is_playing_clone,
                                ) {
                                    Ok(s) => hw_stream = Some(s),
                                    Err(err_msg) => {
                                        let _ = reply.send(Err(err_msg));
                                        continue;
                                    }
                                }
                            }

                            // Limpiar cualquier Cue previo en ambos destinos antes de arrancar el nuevo
                            if let Some(ref hw) = hw_stream {
                                if let Ok(mut decks) = hw.mixer.lock() {
                                    decks.shared_cue = None;
                                }
                            }
                            if let Some(ref chw) = cue_hw_stream {
                                if let Ok(mut cdecks) = chw.mixer.lock() {
                                    cdecks.cue = None;
                                }
                            }

                            let (target_rate, target_channels, active_cue_dev_name) =
                                if !use_shared_main_stream {
                                    let chw = cue_hw_stream.as_ref().unwrap();
                                    (chw.sample_rate, chw.channels, chw.device_name.clone())
                                } else {
                                    let hw = hw_stream.as_ref().unwrap();
                                    (
                                        hw.sample_rate,
                                        hw.channels,
                                        format!("{} (Compartido Aire/Cue)", hw.device_name),
                                    )
                                };

                            match spawn_jit_streaming_decoder_from(
                                &resolved_path,
                                target_rate,
                                target_channels,
                                JINGLE_RING_CAPACITY,
                                None,
                                start_offset_ms,
                            ) {
                                Ok((cue_ring, cue_dur_ms)) => {
                                    let clamped_offset_ms = if cue_dur_ms > 0 {
                                        start_offset_ms.min(cue_dur_ms.saturating_sub(10))
                                    } else {
                                        start_offset_ms
                                    };
                                    let initial_played_frames =
                                        ((clamped_offset_ms * u64::from(target_rate)) / 1000)
                                            as usize;
                                    let fade_in_frames = ((5 * u64::from(target_rate)) / 1000)
                                        .max(1)
                                        as usize;
                                    let new_cue_deck = CueDeck {
                                        ring: cue_ring,
                                        played_frames: initial_played_frames,
                                        fade_in_elapsed_frames: 0,
                                        fade_in_total_frames: fade_in_frames,
                                    };

                                    if !use_shared_main_stream {
                                        if let Some(ref chw) = cue_hw_stream {
                                            if let Ok(mut cdecks) = chw.mixer.lock() {
                                                cdecks.cue = Some(new_cue_deck);
                                            }
                                        }
                                    } else if let Some(ref hw) = hw_stream {
                                        if let Ok(mut decks) = hw.mixer.lock() {
                                            decks.shared_cue = Some(new_cue_deck);
                                        }
                                    }

                                    cue_played_frames_clone
                                        .store(initial_played_frames as u64, Ordering::SeqCst);
                                    cue_sample_rate_clone.store(target_rate, Ordering::SeqCst);
                                    cue_is_playing_clone.store(true, Ordering::SeqCst);

                                    let st = if let Ok(mut guard) = status_clone.lock() {
                                        guard.cue_is_playing = true;
                                        guard.cue_file =
                                            Some(resolved_path.to_string_lossy().to_string());
                                        guard.cue_elapsed_ms = clamped_offset_ms;
                                        guard.cue_duration_ms = cue_dur_ms;
                                        guard.cue_device_name = active_cue_dev_name;
                                        guard.last_error = None;
                                        guard.clone()
                                    } else {
                                        PlayerStatus::default()
                                    };
                                    let _ = reply.send(Ok(st));
                                }
                                Err(err_msg) => {
                                    let _ = reply.send(Err(err_msg));
                                }
                            }
                        }
                        AudioThreadCommand::StopCue { reply } => {
                            if let Some(ref hw) = hw_stream {
                                if let Ok(mut decks) = hw.mixer.lock() {
                                    decks.shared_cue = None;
                                }
                            }
                            if let Some(ref chw) = cue_hw_stream {
                                if let Ok(mut cdecks) = chw.mixer.lock() {
                                    cdecks.cue = None;
                                }
                            }
                            cue_is_playing_clone.store(false, Ordering::SeqCst);
                            cue_played_frames_clone.store(0, Ordering::SeqCst);

                            let st = if let Ok(mut guard) = status_clone.lock() {
                                guard.cue_is_playing = false;
                                guard.cue_elapsed_ms = 0;
                                guard.clone()
                            } else {
                                PlayerStatus::default()
                            };
                            let _ = reply.send(st);
                        }
                        AudioThreadCommand::SetRouting {
                            air_device,
                            cue_device,
                            reply,
                        } => {
                            let air_changed = air_selected != air_device;
                            let cue_changed = cue_selected != cue_device;
                            air_selected = air_device.clone();
                            cue_selected = cue_device.clone();

                            if air_changed {
                                hw_stream = None;
                                is_playing_clone.store(false, Ordering::SeqCst);
                                is_crossfading_clone.store(false, Ordering::SeqCst);
                                sweeper_armed_clone.store(false, Ordering::SeqCst);
                                sweeper_is_playing_clone.store(false, Ordering::SeqCst);
                                played_frames_clone.store(0, Ordering::SeqCst);
                            }
                            if cue_changed || air_changed {
                                cue_hw_stream = None;
                                cue_is_playing_clone.store(false, Ordering::SeqCst);
                                cue_played_frames_clone.store(0, Ordering::SeqCst);
                            }

                            // Probar inicialización del dispositivo de Aire seleccionado (con fallback seguro)
                            if hw_stream.is_none() {
                                if let Ok(s) = init_hardware_stream(
                                    &air_selected,
                                    &played_frames_clone,
                                    &is_playing_clone,
                                    &is_crossfading_clone,
                                    &is_ducking_clone,
                                    &is_paused_by_event_clone,
                                    &overlay_is_playing_clone,
                                    &sweeper_armed_clone,
                                    &sweeper_is_playing_clone,
                                    &active_jingles_clone,
                                    &cue_played_frames_clone,
                                    &cue_is_playing_clone,
                                ) {
                                    hw_stream = Some(s);
                                }
                            }

                            let resolved_air_name = hw_stream
                                .as_ref()
                                .map(|s| s.device_name.clone())
                                .unwrap_or_else(|| air_selected.clone());

                            let resolved_cue_name = if cue_selected == air_selected {
                                format!("{} (Compartido Aire/Cue)", resolved_air_name)
                            } else {
                                resolve_output_device(&cue_selected)
                                    .map(|(_, name)| name)
                                    .unwrap_or_else(|_| {
                                        format!("{} (Fallback)", resolved_air_name)
                                    })
                            };

                            let devices = list_output_devices();
                            let st = if let Ok(mut guard) = status_clone.lock() {
                                guard.air_device_selected = air_selected.clone();
                                guard.cue_device_selected = cue_selected.clone();
                                guard.device_name = resolved_air_name;
                                guard.cue_device_name = resolved_cue_name;
                                guard.available_devices = devices;
                                if let Some(ref hw) = hw_stream {
                                    guard.sample_rate = hw.sample_rate;
                                    guard.channels = hw.channels as u16;
                                }
                                guard.clone()
                            } else {
                                PlayerStatus::default()
                            };
                            let _ = reply.send(Ok(st));
                        }
                        AudioThreadCommand::RefreshDevices { reply } => {
                            let devices = list_output_devices();
                            if let Ok(mut guard) = status_clone.lock() {
                                guard.available_devices = devices.clone();
                            }
                            let _ = reply.send(devices);
                        }
                    }
                }
            })
            .expect("No se pudo iniciar el hilo de audio cpal");

        Self {
            cmd_tx,
            status,
            played_frames,
            decoded_frames,
            is_playing_flag,
            is_crossfading_flag,
            is_ducking_flag,
            is_paused_by_event_flag,
            overlay_is_playing_flag,
            sweeper_armed_flag,
            sweeper_is_playing_flag,
            active_jingles_mask,
            cue_played_frames,
            cue_sample_rate,
            cue_is_playing_flag,
        }
    }

    pub fn play(&self, path: impl AsRef<Path>) -> Result<PlayerStatus, String> {
        self.play_with_crossfade(path, 0)
    }

    pub fn play_with_crossfade(
        &self,
        path: impl AsRef<Path>,
        overlap_ms: u64,
    ) -> Result<PlayerStatus, String> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(AudioThreadCommand::Play {
                path: path.as_ref().to_path_buf(),
                overlap_ms,
                reply: tx,
            })
            .map_err(|e| format!("Hilo de audio no disponible: {}", e))?;
        rx.recv()
            .map_err(|e| format!("Sin respuesta del hilo de audio: {}", e))?
    }

    pub fn play_jingle(&self, slot: usize, path: impl AsRef<Path>) -> Result<PlayerStatus, String> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(AudioThreadCommand::PlayJingle {
                slot,
                path: path.as_ref().to_path_buf(),
                reply: tx,
            })
            .map_err(|e| format!("Hilo de audio no disponible: {}", e))?;
        rx.recv()
            .map_err(|e| format!("Sin respuesta del hilo de audio: {}", e))?
    }

    /// Reproduce un evento programado de forma NO destructiva sobre el canal superpuesto,
    /// aplicando `Ducking` (-70% volumen Main Deck) o `PauseResume` (pausa y reanuda Main Deck).
    pub fn play_event_overlay(
        &self,
        paths: Vec<PathBuf>,
        mode: EventOverlayMode,
    ) -> Result<PlayerStatus, String> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(AudioThreadCommand::PlayEventOverlay {
                paths,
                mode,
                reply: tx,
            })
            .map_err(|e| format!("Hilo de audio no disponible: {}", e))?;
        rx.recv()
            .map_err(|e| format!("Sin respuesta del hilo de audio: {}", e))?
    }

    /// Arma o cancela un Pisador (Sweeper) sincronizado al frame exacto `(Tiempo de Intro - Duración del Pisador)`
    /// sobre el Main Deck activo.
    pub fn schedule_sweeper(
        &self,
        spec: Option<SweeperScheduleSpec>,
    ) -> Result<PlayerStatus, String> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(AudioThreadCommand::ScheduleSweeper { spec, reply: tx })
            .map_err(|e| format!("Hilo de audio no disponible: {}", e))?;
        rx.recv()
            .map_err(|e| format!("Sin respuesta del hilo de audio: {}", e))?
    }

    /// Reproduce un archivo por la salida asignada a Preescucha (`Cue`).
    pub fn play_cue(&self, path: impl AsRef<Path>) -> Result<PlayerStatus, String> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(AudioThreadCommand::PlayCue {
                path: Some(path.as_ref().to_path_buf()),
                start_offset_ms: 0,
                reply: tx,
            })
            .map_err(|e| format!("Hilo de audio no disponible: {}", e))?;
        rx.recv()
            .map_err(|e| format!("Sin respuesta del hilo de audio: {}", e))?
    }

    /// Salta (Seek) inmediatamente al milisegundo `position_ms` dentro del reproductor de Preescucha (`Cue`).
    /// Si se proporciona `path`, carga o reutiliza ese archivo directamente en el Cue Deck.
    pub fn seek_cue(
        &self,
        path: Option<&Path>,
        position_ms: u64,
    ) -> Result<PlayerStatus, String> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(AudioThreadCommand::PlayCue {
                path: path.map(|p| p.to_path_buf()),
                start_offset_ms: position_ms,
                reply: tx,
            })
            .map_err(|e| format!("Hilo de audio no disponible: {}", e))?;
        rx.recv()
            .map_err(|e| format!("Sin respuesta del hilo de audio: {}", e))?
    }

    /// Devuelve la telemetría de posición en tiempo real del Cue Deck sin bloquear el hilo de audio.
    pub fn get_cue_telemetry(&self) -> CueTelemetry {
        let is_playing = self.cue_is_playing_flag.load(Ordering::SeqCst);
        let cue_frames = self.cue_played_frames.load(Ordering::SeqCst);
        let cue_sr = self.cue_sample_rate.load(Ordering::SeqCst).max(1);
        let raw_pos_ms = (cue_frames * 1000) / u64::from(cue_sr);

        let (duration_ms, cue_file) = self
            .status
            .lock()
            .map(|g| (g.cue_duration_ms, g.cue_file.clone()))
            .unwrap_or((0, None));

        let current_position_ms = if duration_ms > 0 {
            raw_pos_ms.min(duration_ms)
        } else {
            raw_pos_ms
        };

        CueTelemetry {
            is_playing,
            current_position_ms,
            duration_ms,
            cue_file,
        }
    }

    /// Detiene el reproductor de Preescucha (`Cue`) sin afectar la salida de Aire.
    pub fn stop_cue(&self) -> PlayerStatus {
        let (tx, rx) = mpsc::channel();
        let _ = self.cmd_tx.send(AudioThreadCommand::StopCue { reply: tx });
        rx.recv().unwrap_or_default()
    }

    /// Configura las tarjetas de sonido para `Main / Aire` y `Cue / Preescucha`.
    pub fn set_audio_routing(
        &self,
        air_device: String,
        cue_device: String,
    ) -> Result<PlayerStatus, String> {
        let (tx, rx) = mpsc::channel();
        self.cmd_tx
            .send(AudioThreadCommand::SetRouting {
                air_device,
                cue_device,
                reply: tx,
            })
            .map_err(|e| format!("Hilo de audio no disponible: {}", e))?;
        rx.recv()
            .map_err(|e| format!("Sin respuesta del hilo de audio: {}", e))?
    }

    /// Refresca y devuelve la lista de dispositivos de salida de audio del sistema.
    pub fn refresh_devices(&self) -> Vec<String> {
        let (tx, rx) = mpsc::channel();
        let _ = self
            .cmd_tx
            .send(AudioThreadCommand::RefreshDevices { reply: tx });
        rx.recv().unwrap_or_else(|_| list_output_devices())
    }

    pub fn stop(&self) -> PlayerStatus {
        let (tx, rx) = mpsc::channel();
        let _ = self.cmd_tx.send(AudioThreadCommand::Stop { reply: tx });
        rx.recv().unwrap_or_default()
    }

    pub fn get_status(&self) -> PlayerStatus {
        let mut st = self.status.lock().map(|g| g.clone()).unwrap_or_default();
        st.is_playing = self.is_playing_flag.load(Ordering::SeqCst);
        st.is_crossfading = self.is_crossfading_flag.load(Ordering::SeqCst);
        st.is_ducking = self.is_ducking_flag.load(Ordering::SeqCst);
        st.is_paused_by_event = self.is_paused_by_event_flag.load(Ordering::SeqCst);
        st.overlay_is_playing = self.overlay_is_playing_flag.load(Ordering::SeqCst);
        if !st.overlay_is_playing {
            st.overlay_file = None;
        }
        st.sweeper_armed = self.sweeper_armed_flag.load(Ordering::SeqCst);
        st.sweeper_is_playing = self.sweeper_is_playing_flag.load(Ordering::SeqCst);
        if !st.sweeper_armed && !st.sweeper_is_playing {
            st.sweeper_file = None;
            st.sweeper_intro_ms = 0;
            st.sweeper_trigger_ms = 0;
            st.sweeper_duration_ms = 0;
        }
        st.decoded_frames = self.decoded_frames.load(Ordering::Acquire);
        let frames = self.played_frames.load(Ordering::SeqCst);
        if st.sample_rate > 0 {
            st.elapsed_ms = (frames * 1000) / u64::from(st.sample_rate);
        }
        st.active_jingle_slots = mask_to_slots(self.active_jingles_mask.load(Ordering::SeqCst));

        st.cue_is_playing = self.cue_is_playing_flag.load(Ordering::SeqCst);
        let cue_frames = self.cue_played_frames.load(Ordering::SeqCst);
        let cue_sr = self.cue_sample_rate.load(Ordering::SeqCst).max(1);
        st.cue_elapsed_ms = (cue_frames * 1000) / u64::from(cue_sr);
        st
    }
}

#[allow(clippy::too_many_arguments)]
fn init_hardware_stream(
    requested_name: &str,
    played_frames: &Arc<AtomicU64>,
    is_playing_flag: &Arc<AtomicBool>,
    is_crossfading_flag: &Arc<AtomicBool>,
    is_ducking_flag: &Arc<AtomicBool>,
    is_paused_by_event_flag: &Arc<AtomicBool>,
    overlay_is_playing_flag: &Arc<AtomicBool>,
    sweeper_armed_flag: &Arc<AtomicBool>,
    sweeper_is_playing_flag: &Arc<AtomicBool>,
    active_jingles_mask: &Arc<AtomicU16>,
    cue_played_frames: &Arc<AtomicU64>,
    cue_is_playing_flag: &Arc<AtomicBool>,
) -> Result<ActiveHardwareStream, String> {
    let (device, device_name) = resolve_output_device(requested_name)?;

    let supported_config = device
        .default_output_config()
        .map_err(|e| format!("No se pudo obtener la configuración de salida cpal: {}", e))?;

    let sample_format = supported_config.sample_format();
    let config: cpal::StreamConfig = supported_config.into();
    let dst_rate = config.sample_rate.0;
    let dst_channels = (config.channels as usize).max(1);

    let mixer = Arc::new(Mutex::new(MixerDecks::default()));
    let mixer_cb = Arc::clone(&mixer);
    let played_frames_cb = Arc::clone(played_frames);
    let is_playing_cb = Arc::clone(is_playing_flag);
    let is_crossfading_cb = Arc::clone(is_crossfading_flag);
    let is_ducking_cb = Arc::clone(is_ducking_flag);
    let is_paused_cb = Arc::clone(is_paused_by_event_flag);
    let overlay_playing_cb = Arc::clone(overlay_is_playing_flag);
    let sweeper_armed_cb = Arc::clone(sweeper_armed_flag);
    let sweeper_playing_cb = Arc::clone(sweeper_is_playing_flag);
    let active_jingles_cb = Arc::clone(active_jingles_mask);
    let cue_played_cb = Arc::clone(cue_played_frames);
    let cue_playing_cb = Arc::clone(cue_is_playing_flag);

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_mixer_output_stream::<f32>(
            &device,
            &config,
            mixer_cb,
            dst_channels,
            dst_rate,
            played_frames_cb,
            is_playing_cb,
            is_crossfading_cb,
            is_ducking_cb,
            is_paused_cb,
            overlay_playing_cb,
            sweeper_armed_cb,
            sweeper_playing_cb,
            active_jingles_cb,
            cue_played_cb,
            cue_playing_cb,
        )?,
        cpal::SampleFormat::I16 => build_mixer_output_stream::<i16>(
            &device,
            &config,
            mixer_cb,
            dst_channels,
            dst_rate,
            played_frames_cb,
            is_playing_cb,
            is_crossfading_cb,
            is_ducking_cb,
            is_paused_cb,
            overlay_playing_cb,
            sweeper_armed_cb,
            sweeper_playing_cb,
            active_jingles_cb,
            cue_played_cb,
            cue_playing_cb,
        )?,
        cpal::SampleFormat::U16 => build_mixer_output_stream::<u16>(
            &device,
            &config,
            mixer_cb,
            dst_channels,
            dst_rate,
            played_frames_cb,
            is_playing_cb,
            is_crossfading_cb,
            is_ducking_cb,
            is_paused_cb,
            overlay_playing_cb,
            sweeper_armed_cb,
            sweeper_playing_cb,
            active_jingles_cb,
            cue_played_cb,
            cue_playing_cb,
        )?,
        other => {
            return Err(format!("Formato de muestra CPAL no soportado: {:?}", other));
        }
    };

    stream
        .play()
        .map_err(|e| format!("Error al iniciar cpal::Stream::play(): {}", e))?;

    Ok(ActiveHardwareStream {
        _stream: stream,
        requested_name: requested_name.to_string(),
        device_name,
        sample_rate: dst_rate,
        channels: dst_channels,
        mixer,
    })
}

fn init_dedicated_cue_stream(
    requested_name: &str,
    cue_played_frames: &Arc<AtomicU64>,
    cue_is_playing_flag: &Arc<AtomicBool>,
) -> Result<ActiveCueHardwareStream, String> {
    let (device, device_name) = resolve_output_device(requested_name)?;

    let supported_config = device
        .default_output_config()
        .map_err(|e| format!("No se pudo obtener configuración cpal para Cue: {}", e))?;

    let sample_format = supported_config.sample_format();
    let config: cpal::StreamConfig = supported_config.into();
    let dst_rate = config.sample_rate.0;
    let dst_channels = (config.channels as usize).max(1);

    let mixer = Arc::new(Mutex::new(DedicatedCueMixer::default()));
    let mixer_cb = Arc::clone(&mixer);
    let cue_played_cb = Arc::clone(cue_played_frames);
    let cue_playing_cb = Arc::clone(cue_is_playing_flag);

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_dedicated_cue_stream::<f32>(
            &device,
            &config,
            mixer_cb,
            dst_channels,
            cue_played_cb,
            cue_playing_cb,
        )?,
        cpal::SampleFormat::I16 => build_dedicated_cue_stream::<i16>(
            &device,
            &config,
            mixer_cb,
            dst_channels,
            cue_played_cb,
            cue_playing_cb,
        )?,
        cpal::SampleFormat::U16 => build_dedicated_cue_stream::<u16>(
            &device,
            &config,
            mixer_cb,
            dst_channels,
            cue_played_cb,
            cue_playing_cb,
        )?,
        other => {
            return Err(format!("Formato de muestra CPAL Cue no soportado: {:?}", other));
        }
    };

    stream
        .play()
        .map_err(|e| format!("Error al iniciar stream de Cue: {}", e))?;

    Ok(ActiveCueHardwareStream {
        _stream: stream,
        requested_name: requested_name.to_string(),
        device_name,
        sample_rate: dst_rate,
        channels: dst_channels,
        mixer,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_mixer_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mixer: Arc<Mutex<MixerDecks>>,
    channels: usize,
    sample_rate: u32,
    played_frames: Arc<AtomicU64>,
    is_playing_flag: Arc<AtomicBool>,
    is_crossfading_flag: Arc<AtomicBool>,
    is_ducking_flag: Arc<AtomicBool>,
    is_paused_by_event_flag: Arc<AtomicBool>,
    overlay_is_playing_flag: Arc<AtomicBool>,
    sweeper_armed_flag: Arc<AtomicBool>,
    sweeper_is_playing_flag: Arc<AtomicBool>,
    active_jingles_mask: Arc<AtomicU16>,
    cue_played_frames: Arc<AtomicU64>,
    cue_is_playing_flag: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let err_fn = |err| eprintln!("Error en stream de salida CPAL (Aire): {}", err);
    let max_ch = channels.clamp(1, 8);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let Ok(mut decks) = mixer.try_lock() else {
                    for sample_out in data.iter_mut() {
                        *sample_out = T::EQUILIBRIUM;
                    }
                    return;
                };

                for frame in data.chunks_mut(channels) {
                    let ch_len = frame.len().min(max_ch);
                    let mut mixed_frame = [0.0_f32; 8];
                    let (drop_outgoing, drop_shared_cue) =
                        decks.mix_next_frame(ch_len, sample_rate, &mut mixed_frame);

                    if drop_outgoing {
                        is_crossfading_flag.store(false, Ordering::Relaxed);
                    }
                    if drop_shared_cue {
                        cue_is_playing_flag.store(false, Ordering::Relaxed);
                    }

                    for (c, sample_slot) in frame.iter_mut().enumerate() {
                        let val = if c < ch_len { mixed_frame[c] } else { 0.0 };
                        *sample_slot = T::from_sample(soft_limit_sample(val));
                    }
                }

                let overlay_active = !decks.event_overlays.is_empty();
                let is_ducking =
                    overlay_active && decks.event_overlay_mode == EventOverlayMode::Ducking;
                let is_paused =
                    overlay_active && decks.event_overlay_mode == EventOverlayMode::PauseResume;

                overlay_is_playing_flag.store(overlay_active, Ordering::Relaxed);
                is_ducking_flag.store(is_ducking, Ordering::Relaxed);
                is_paused_by_event_flag.store(is_paused, Ordering::Relaxed);

                let sweeper_armed = decks.sweeper.is_some();
                let sweeper_playing = decks
                    .sweeper
                    .as_ref()
                    .map(|sw| sw.started && !sw.ring.is_finished())
                    .unwrap_or(false);
                sweeper_armed_flag.store(sweeper_armed, Ordering::Relaxed);
                sweeper_is_playing_flag.store(sweeper_playing, Ordering::Relaxed);

                let mut mask = 0u16;
                for j in &decks.jingles {
                    if (1..=9).contains(&j.slot) {
                        mask |= 1 << j.slot;
                    }
                }
                active_jingles_mask.store(mask, Ordering::Relaxed);

                if let Some(ref primary) = decks.primary {
                    played_frames.store(primary.played_frames as u64, Ordering::Relaxed);
                    if primary.ring.is_finished() && !is_paused {
                        is_playing_flag.store(false, Ordering::Relaxed);
                    }
                } else {
                    is_playing_flag.store(false, Ordering::Relaxed);
                }

                if let Some(ref cue) = decks.shared_cue {
                    cue_played_frames.store(cue.played_frames as u64, Ordering::Relaxed);
                }
            },
            err_fn,
            Some(Duration::from_secs(2)),
        )
        .map_err(|e| format!("Fallo en build_output_stream (Aire): {}", e))
}

fn build_dedicated_cue_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mixer: Arc<Mutex<DedicatedCueMixer>>,
    channels: usize,
    cue_played_frames: Arc<AtomicU64>,
    cue_is_playing_flag: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let err_fn = |err| eprintln!("Error en stream de salida CPAL (Cue): {}", err);
    let max_ch = channels.clamp(1, 8);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let Ok(mut cdecks) = mixer.try_lock() else {
                    for sample_out in data.iter_mut() {
                        *sample_out = T::EQUILIBRIUM;
                    }
                    return;
                };

                let mut cue_frame = [0.0_f32; 8];
                for frame in data.chunks_mut(channels) {
                    let ch_len = frame.len().min(max_ch);
                    let mut drop_cue = false;

                    if let Some(ref mut cue) = cdecks.cue {
                        let c_gain = cue.current_gain();
                        if cue.ring.pop_frame(&mut cue_frame[..ch_len]) {
                            cue.played_frames += 1;
                            if cue.fade_in_elapsed_frames < cue.fade_in_total_frames {
                                cue.fade_in_elapsed_frames += 1;
                            }
                            for (c, sample_slot) in frame.iter_mut().enumerate() {
                                let val = if c < ch_len {
                                    cue_frame[c] * c_gain
                                } else {
                                    0.0
                                };
                                *sample_slot = T::from_sample(soft_limit_sample(val));
                            }
                        } else {
                            for sample_slot in frame.iter_mut() {
                                *sample_slot = T::EQUILIBRIUM;
                            }
                        }
                        if cue.ring.is_finished() {
                            drop_cue = true;
                        }
                    } else {
                        for sample_slot in frame.iter_mut() {
                            *sample_slot = T::EQUILIBRIUM;
                        }
                    }

                    if drop_cue {
                        cdecks.cue = None;
                        cue_is_playing_flag.store(false, Ordering::Relaxed);
                    }
                }

                if let Some(ref cue) = cdecks.cue {
                    cue_played_frames.store(cue.played_frames as u64, Ordering::Relaxed);
                }
            },
            err_fn,
            Some(Duration::from_secs(2)),
        )
        .map_err(|e| format!("Fallo en build_dedicated_cue_stream: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soft_limit_prevents_clipping() {
        assert!((soft_limit_sample(0.5) - 0.5).abs() < 1e-6);
        assert!((soft_limit_sample(-0.5) + 0.5).abs() < 1e-6);

        let high_pos = soft_limit_sample(2.4);
        let high_neg = soft_limit_sample(-2.4);
        assert!(high_pos > 0.85 && high_pos <= 1.0);
        assert!(high_neg < -0.85 && high_neg >= -1.0);
    }

    #[test]
    fn test_spsc_ring_buffer_lock_free_streaming() {
        let ring = SpscRingBuffer::new(1024);
        assert_eq!(ring.capacity(), 1024);
        assert_eq!(ring.available_read(), 0);

        let input = [0.25_f32, -0.25, 0.75, -0.75];
        let written = ring.push_slice(&input);
        assert_eq!(written, 4);
        assert_eq!(ring.available_read(), 4);

        let mut frame = [0.0_f32; 2];
        assert!(ring.pop_frame(&mut frame));
        assert_eq!(frame, [0.25, -0.25]);
        assert!(ring.pop_frame(&mut frame));
        assert_eq!(frame, [0.75, -0.75]);
        assert!(!ring.pop_frame(&mut frame));

        ring.mark_eof();
        assert!(ring.is_finished());
    }

    #[test]
    fn test_lazy_probing_does_not_decode_audio() {
        let wav_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_tone.wav");
        let info =
            probe_track_metadata(&wav_path).expect("debe inspeccionar cabeceras sin decodificar");
        assert!(info.duration_ms >= 3500);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 2);
    }

    #[test]
    fn test_list_output_devices_always_has_default() {
        let devices = list_output_devices();
        assert!(!devices.is_empty());
        assert_eq!(devices[0], "Default Output");
    }

    #[test]
    fn test_mixer_ducking_70_percent_and_restore() {
        let mut decks = MixerDecks::default();
        let main_ring = Arc::new(SpscRingBuffer::new(4096));
        let main_samples = vec![1.0_f32; 2000];
        main_ring.push_slice(&main_samples);
        decks.primary = Some(PrimaryDeck {
            ring: main_ring,
            played_frames: 0,
            fade_in_elapsed_frames: 10,
            fade_in_total_frames: 10,
        });

        let overlay_ring = Arc::new(SpscRingBuffer::new(2048));
        let overlay_samples = vec![0.20_f32; 400]; // 200 frames estéreo
        overlay_ring.push_slice(&overlay_samples);
        overlay_ring.mark_eof();

        decks.event_overlay_mode = EventOverlayMode::Ducking;
        decks.event_overlays.push_back(OverlayVoice {
            ring: overlay_ring,
            fade_in_elapsed_frames: 1,
            fade_in_total_frames: 1,
        });

        let mut mixed = [0.0_f32; 8];
        // Usamos sample_rate = 100 Hz para que la rampa de 180ms alcance 0.30 en ~18 frames
        for _ in 0..100 {
            decks.mix_next_frame(2, 100, &mut mixed);
        }
        assert!(
            (decks.duck_gain - 0.30).abs() < 1e-4,
            "El modo Ducking debe atenuar el Main Deck un 70% (ganancia 0.30), actual: {}",
            decks.duck_gain
        );
        // Mientras suena el evento (0.20) + Main Deck atenuado (0.30) = 0.50
        assert!((mixed[0] - 0.50).abs() < 1e-3);

        // Consumir los 100 frames restantes del evento y 50 frames adicionales para restaurar volumen
        for _ in 0..150 {
            decks.mix_next_frame(2, 100, &mut mixed);
        }
        assert!(decks.event_overlays.is_empty());
        assert!(
            (decks.duck_gain - 1.0).abs() < 1e-4,
            "Al finalizar el evento, la ganancia del Main Deck debe volver a 1.0 (100%)"
        );
        // La pista principal nunca se detuvo ni se destruyó (250 frames reproducidos)
        assert_eq!(decks.primary.as_ref().unwrap().played_frames, 250);
    }

    #[test]
    fn test_mixer_pause_and_resume_freezes_main_deck() {
        let mut decks = MixerDecks::default();
        let main_ring = Arc::new(SpscRingBuffer::new(4096));
        let main_samples = vec![0.8_f32; 2000];
        main_ring.push_slice(&main_samples);
        decks.primary = Some(PrimaryDeck {
            ring: main_ring,
            played_frames: 0,
            fade_in_elapsed_frames: 10,
            fade_in_total_frames: 10,
        });

        // Reproducir 20 frames normales del Main Deck
        let mut mixed = [0.0_f32; 8];
        for _ in 0..20 {
            decks.mix_next_frame(2, 100, &mut mixed);
        }
        assert_eq!(decks.primary.as_ref().unwrap().played_frames, 20);

        // Inyectar evento en modo PauseResume (100 frames estéreo = 200 muestras)
        let overlay_ring = Arc::new(SpscRingBuffer::new(2048));
        overlay_ring.push_slice(&vec![0.5_f32; 200]);
        overlay_ring.mark_eof();

        decks.event_overlay_mode = EventOverlayMode::PauseResume;
        decks.event_overlays.push_back(OverlayVoice {
            ring: overlay_ring,
            fade_in_elapsed_frames: 1,
            fade_in_total_frames: 1,
        });

        // Con sample_rate=100, la rampa anti-click de pausa dura ~1-2 frames y luego congela el Main Deck
        for _ in 0..100 {
            decks.mix_next_frame(2, 100, &mut mixed);
        }
        let frozen_frames = decks.primary.as_ref().unwrap().played_frames;
        assert!(
            frozen_frames <= 22,
            "El Main Deck debe quedar congelado durante el evento PauseResume (frames = {})",
            frozen_frames
        );
        assert!(decks.event_overlays.is_empty());

        // Tras terminar el evento, los siguientes 30 frames reanudan el Main Deck exactamente donde se quedó
        for _ in 0..30 {
            decks.mix_next_frame(2, 100, &mut mixed);
        }
        assert_eq!(
            decks.primary.as_ref().unwrap().played_frames,
            frozen_frames + 30
        );
        assert!((decks.pause_envelope_gain - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_sweeper_trigger_formula_and_sample_accurate_firing() {
        // Caso del enunciado: Intro de canción = 12s (12_000 ms), Pisador = 4s (4_000 ms) => Disparo = 8s (8_000 ms)
        assert_eq!(compute_sweeper_trigger_ms(12_000, 4_000), 8_000);
        // Caso con decimales: Intro = 00:12.5 (12_500 ms), Pisador = 4s (4_000 ms) => Disparo = 00:08.5 (8_500 ms)
        assert_eq!(compute_sweeper_trigger_ms(12_500, 4_000), 8_500);
        // Si el pisador es más largo que la intro, dispara en 0ms
        assert_eq!(compute_sweeper_trigger_ms(3_000, 4_000), 0);

        // Verificar disparo exacto a nivel de frame en MixerDecks (con sample_rate = 10 Hz para que 1s = 10 frames):
        // Intro = 12s (120 frames), Pisador = 4s (40 frames) => trigger_frame = 80 (segundo 8.0)
        let mut decks = MixerDecks::default();
        let main_ring = Arc::new(SpscRingBuffer::new(1024));
        main_ring.push_slice(&vec![0.40_f32; 400]); // 200 frames estéreo
        decks.primary = Some(PrimaryDeck {
            ring: main_ring,
            played_frames: 0,
            fade_in_elapsed_frames: 1,
            fade_in_total_frames: 1,
        });

        let sweeper_ring = Arc::new(SpscRingBuffer::new(512));
        sweeper_ring.push_slice(&vec![0.30_f32; 80]); // 40 frames estéreo (= 4s a 10 Hz)
        sweeper_ring.mark_eof();

        decks.sweeper = Some(ScheduledSweeperVoice {
            ring: sweeper_ring,
            trigger_frame: 80,
            started: false,
            fade_in_elapsed_frames: 1,
            fade_in_total_frames: 1,
        });

        let mut mixed = [0.0_f32; 8];
        // Frames 0..80 (segundos 0.0 a 7.9): el pisador debe permanecer armado y en silencio
        for _ in 0..80 {
            decks.mix_next_frame(2, 10, &mut mixed);
            assert!(
                !decks.sweeper.as_ref().unwrap().started,
                "El pisador no debe dispararse antes del frame 80 (segundo 8)"
            );
            assert!((mixed[0] - 0.40).abs() < 1e-4);
        }

        // Frame 80 (exactamente segundo 8.0): el pisador se dispara y se suma sobre la música
        decks.mix_next_frame(2, 10, &mut mixed);
        assert!(decks.sweeper.as_ref().unwrap().started);
        assert!(mixed[0] > 0.50);

        // Frames 81..120 (los 39 frames restantes de los 4s del pisador): concluye exactamente en el frame 120 (segundo 12.0, fin de Intro)
        for _ in 81..120 {
            decks.mix_next_frame(2, 10, &mut mixed);
        }
        assert!(
            decks.sweeper.is_none(),
            "El pisador de 4s debe concluir exactamente al llegar al frame 120 (fin de la Intro de 12s)"
        );
        assert_eq!(decks.primary.as_ref().unwrap().played_frames, 120);
    }

    #[test]
    fn test_extract_waveform_peaks() {
        let wav_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_tone.wav");
        let wf = extract_waveform_peaks(&wav_path, 240)
            .expect("debe extraer picos de forma de onda sin error");
        assert_eq!(wf.peaks.len(), 240);
        assert!(wf.duration_ms >= 3500);
        assert!(wf.peaks.iter().any(|&p| p > 0.5));
    }

    #[test]
    fn test_jit_streaming_decoder_seek_offset() {
        let wav_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_tone.wav");
        let (ring, duration_ms) =
            spawn_jit_streaming_decoder_from(&wav_path, 44100, 2, 65536, None, 2000)
                .expect("debe iniciar el decodificador JIT con offset de 2000ms (Seek)");
        assert!(duration_ms >= 4000);
        let mut frame = [0.0_f32; 2];
        assert!(
            ring.pop_frame(&mut frame),
            "el buffer circular debe contener muestras precargadas tras el Seek"
        );
        ring.cancel();
    }
}
