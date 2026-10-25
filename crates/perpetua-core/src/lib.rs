/// Locuciones de hora (`.time` -> `horas/son_las.*`, `horas/{HH}.*`, `horas/y_{MM}.*`).
pub mod announcements;
/// Motor de carpetas aleatorias con historial Anti-Repeat de las últimas 50 canciones.
pub mod random_pool;
/// Planificador de eventos (Hito 6): lógica pura evaluada cada segundo por el hilo de reloj.
pub mod scheduler;
/// Gestor de metadatos de mezcla (Hito 8): persistencia SQLite de Tiempo de Intro (`intro_ms`) y Pisadores (`sweeper_path`).
pub mod track_metadata;

use announcements::{resolve_time_announcement_sequence, TIME_COMMAND_TOKEN};
use chrono::{DateTime, Local, Timelike};
use rand::Rng;
use random_pool::{scan_audio_files_recursive, AntiRepeatPool};
pub use scheduler::EventInterruptionMode;
use scheduler::{EventScheduler, NextEventInfo, ScheduledEvent};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
pub use track_metadata::{TrackMetadataRecord, TrackMetadataStore};
pub use perpetua_audio::CueTelemetry;
use perpetua_audio::{
    compute_sweeper_trigger_ms, extract_waveform_peaks, probe_track_metadata, AudioPlayer,
    EventOverlayMode, PlayerStatus, SweeperScheduleSpec,
};

/// Datos devueltos por `get_track_waveform` para renderizar el Editor Visual de Puntos de Mezcla (Hito 8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveformEditorData {
    pub path: String,
    pub title: String,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub peaks: Vec<f32>,
    pub intro_ms: u64,
    pub sweeper_path: Option<String>,
    pub sweeper_title: Option<String>,
    pub sweeper_duration_ms: u64,
    pub sweeper_trigger_ms: Option<u64>,
}

/// Entrada de audio devuelta al expandir visualmente una Carpeta Aleatoria en la UI (sin decodificar a memoria).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FolderAudioEntry {
    pub path: String,
    pub title: String,
    pub extension: String,
    pub size_bytes: u64,
    pub in_recent_history: bool,
    #[serde(default)]
    pub intro_ms: u64,
    #[serde(default)]
    pub sweeper_path: Option<String>,
    #[serde(default)]
    pub sweeper_title: Option<String>,
    #[serde(default)]
    pub sweeper_duration_ms: u64,
}

/// Tipo de elemento en la lista de reproducción (Hito 7).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// Archivo de audio regular (`.wav`, `.mp3`, `.flac`, `.ogg`).
    #[default]
    File,
    /// Carpeta Aleatoria (`Random Folder`): escanea el directorio y elige una pista sin repetir en las últimas 50.
    RandomFolder,
    /// Comando Virtual de Hora (`.time`): pronuncia la hora actual encadenando audios de `horas/`.
    TimeAnnouncement,
}

/// Elemento de audio inyectado por un evento o locución `.time` en el Main Deck.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventItem {
    pub event_id: u64,
    pub title: String,
    pub path: String,
    pub wait_for_current: bool,
    /// Si es `true` (ej. segmentos encadenados de `.time`), se reproduce hasta el final sin crossfade.
    #[serde(default)]
    pub no_crossfade: bool,
}

/// Siguiente fuente de audio para el Main Deck: los eventos/segmentos encolados tienen prioridad sobre la lista.
enum NextSource {
    Event(EventItem),
    Track(usize),
}

/// Modos clásicos de emisión de radio.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    /// Continuo / Auto: Reproduce toda la lista en orden hasta el final y se detiene al concluir la última pista.
    #[default]
    Auto,
    /// Manual: Al finalizar la canción actual, se detiene y deja lista la siguiente pista esperando que el usuario presione Play.
    Manual,
    /// Repetir lista (Loop): Reproduce en orden y al terminar la última pista vuelve automáticamente a la primera.
    Loop,
    /// Aleatorio (Shuffle): Selecciona la siguiente pista de la cola al azar.
    Shuffle,
}

impl PlaybackMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlaybackMode::Auto => "auto",
            PlaybackMode::Manual => "manual",
            PlaybackMode::Loop => "loop",
            PlaybackMode::Shuffle => "shuffle",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "manual" => PlaybackMode::Manual,
            "loop" => PlaybackMode::Loop,
            "shuffle" => PlaybackMode::Shuffle,
            _ => PlaybackMode::Auto,
        }
    }
}

/// Elemento individual dentro de la lista de reproducción en memoria.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackItem {
    pub id: usize,
    pub path: String,
    pub title: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub sample_rate: u32,
    #[serde(default)]
    pub kind: TrackKind,
    /// Último archivo elegido cuando `kind == TrackKind::RandomFolder`.
    #[serde(default)]
    pub last_resolved_file: Option<String>,
    /// Hito 8: Tiempo de Intro en milisegundos (donde empieza la voz del cantante, ej. `12500` ms para `00:12.5`).
    #[serde(default)]
    pub intro_ms: u64,
    /// Hito 8: Ruta del Pisador (Sweeper) asignado a esta pista o carpeta.
    #[serde(default)]
    pub sweeper_path: Option<String>,
    /// Hito 8: Nombre legible del Pisador (Sweeper) asignado.
    #[serde(default)]
    pub sweeper_title: Option<String>,
    /// Hito 8: Duración del Pisador (Sweeper) en milisegundos.
    #[serde(default)]
    pub sweeper_duration_ms: u64,
    /// Hito 8: Instante exacto de disparo en ms: `(Tiempo de Intro) - (Duración del Pisador)`.
    #[serde(default)]
    pub sweeper_trigger_ms: Option<u64>,
}

impl TrackItem {
    /// Recalcula `sweeper_title`, `sweeper_duration_ms` y `sweeper_trigger_ms` a partir de `intro_ms` y `sweeper_path`.
    pub fn recompute_sweeper_fields(&mut self) {
        let Some(ref sp) = self.sweeper_path.clone().filter(|s| !s.trim().is_empty()) else {
            self.sweeper_path = None;
            self.sweeper_title = None;
            self.sweeper_duration_ms = 0;
            self.sweeper_trigger_ms = None;
            return;
        };

        let sp_path = Path::new(sp);
        if self.sweeper_duration_ms == 0 && sp_path.exists() {
            if let Ok(info) = probe_track_metadata(sp_path) {
                self.sweeper_duration_ms = info.duration_ms;
            }
        }
        let fallback_title = sp_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(sp)
            .to_string();
        self.sweeper_title = Some(fallback_title);
        self.sweeper_trigger_ms = Some(compute_sweeper_trigger_ms(
            self.intro_ms,
            self.sweeper_duration_ms,
        ));
    }
}

/// Ranura individual de la Botonera de Jingles (Cart Machine, teclas 1 a 9).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JingleSlot {
    /// Número de ranura y atajo de teclado (`1`..=`9`).
    pub slot: usize,
    pub path: Option<String>,
    pub title: Option<String>,
    pub duration_ms: u64,
    #[serde(default)]
    pub is_playing: bool,
}

/// Estructura serializable para persistencia de listas en JSON estructurado (`.perpetua.json` / `.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPlaylistDocument {
    pub version: String,
    pub playback_mode: PlaybackMode,
    pub crossfade_ms: u64,
    pub tracks: Vec<TrackItem>,
    #[serde(default)]
    pub jingles: Vec<JingleSlot>,
}

/// Estado interno de la cola de reproducción, configuración de emisión y botonera de Jingles.
#[derive(Debug)]
struct PlaylistState {
    tracks: Vec<TrackItem>,
    current_index: Option<usize>,
    next_id: usize,
    auto_advance_armed: bool,
    playback_mode: PlaybackMode,
    crossfade_ms: u64,
    crossfade_triggered_for_current: bool,
    jingles: Vec<JingleSlot>,
    autosave_path: Option<PathBuf>,
    /// Eventos programados (Hito 6).
    scheduler: EventScheduler,
    /// Eventos "con espera" o segmentos encadenados (`.time`) que sonarán antes de la siguiente pista.
    pending_events: VecDeque<EventItem>,
    /// Evento o segmento que está sonando ahora en el Main Deck.
    on_air_event: Option<EventItem>,
    events_path: Option<PathBuf>,
    last_event_message: Option<String>,
    /// Hito 7: Selector aleatorio con historial de los últimos 50 archivos reproducidos.
    anti_repeat: AntiRepeatPool,
    /// Hito 7: Carpeta predefinida de locuciones horarias (`horas/`).
    horas_dir: PathBuf,
    /// Hito 8: Gestor SQLite de Tiempo de Intro y Pisadores (Sweepers) por canción.
    metadata_store: TrackMetadataStore,
}

impl Default for PlaylistState {
    fn default() -> Self {
        let jingles = (1..=9)
            .map(|slot| JingleSlot {
                slot,
                path: None,
                title: None,
                duration_ms: 0,
                is_playing: false,
            })
            .collect();

        let default_horas = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("horas");

        Self {
            tracks: Vec::new(),
            current_index: None,
            next_id: 0,
            auto_advance_armed: false,
            playback_mode: PlaybackMode::Auto,
            crossfade_ms: 2000,
            crossfade_triggered_for_current: false,
            jingles,
            autosave_path: None,
            scheduler: EventScheduler::default(),
            pending_events: VecDeque::new(),
            on_air_event: None,
            events_path: None,
            last_event_message: None,
            anti_repeat: AntiRepeatPool::new(),
            horas_dir: default_horas,
            metadata_store: TrackMetadataStore::default(),
        }
    }
}

impl PlaylistState {
    /// Sincroniza un `TrackItem` recién clasificado o cargado con la base de datos SQLite de metadatos.
    fn hydrate_track_item(&self, item: &mut TrackItem) {
        if item.kind == TrackKind::TimeAnnouncement {
            item.recompute_sweeper_fields();
            return;
        }

        if let Some(record) = self.metadata_store.get(&item.path) {
            if record.intro_ms > 0 {
                item.intro_ms = record.intro_ms;
            } else if item.intro_ms > 0 {
                let _ = self.metadata_store.set_intro_ms(&item.path, item.intro_ms);
            }
            if item.sweeper_path.is_none() && record.sweeper_path.is_some() {
                item.sweeper_path = record.sweeper_path;
                item.sweeper_duration_ms = record.sweeper_duration_ms;
            }
        } else if item.intro_ms > 0 || item.sweeper_path.is_some() {
            let _ = self.metadata_store.set_markers(
                &item.path,
                item.intro_ms,
                item.sweeper_path.as_deref(),
                item.sweeper_duration_ms,
            );
        }

        item.recompute_sweeper_fields();
    }

    /// Calcula el `SweeperScheduleSpec` para el archivo que acaba de entrar al Main Deck,
    /// aplicando la fórmula exacta `(Tiempo de Intro de la Canción) - (Duración del Pisador)`.
    fn resolve_sweeper_for_active_playback(
        &mut self,
        active_file_path: &str,
    ) -> Option<SweeperScheduleSpec> {
        if self.on_air_event.is_some() {
            return None;
        }

        let db_record = self.metadata_store.get(active_file_path);
        let current_track = self.current_index.and_then(|idx| self.tracks.get(idx)).cloned();

        // 1. Determinar el Tiempo de Intro (ms) de la canción activa
        let mut intro_ms = db_record.as_ref().map(|r| r.intro_ms).unwrap_or(0);
        if intro_ms == 0 {
            if let Some(ref tr) = current_track {
                if tr.intro_ms > 0 && (tr.kind == TrackKind::File || tr.path == active_file_path) {
                    intro_ms = tr.intro_ms;
                }
            }
        }
        if intro_ms == 0 {
            if let Ok(probed) = probe_track_metadata(Path::new(active_file_path)) {
                if let Some(tag_intro) = probed.intro_ms {
                    intro_ms = tag_intro;
                }
            }
        }

        // 2. Determinar qué Pisador (Sweeper) tiene asignado la fila de la Playlist o el archivo en SQLite
        let (sweeper_path_opt, mut sweeper_dur_ms) = if let Some(ref tr) = current_track {
            if tr.sweeper_path.is_some() {
                (tr.sweeper_path.clone(), tr.sweeper_duration_ms)
            } else if let Some(ref rec) = db_record {
                (rec.sweeper_path.clone(), rec.sweeper_duration_ms)
            } else {
                (None, 0)
            }
        } else if let Some(ref rec) = db_record {
            (rec.sweeper_path.clone(), rec.sweeper_duration_ms)
        } else {
            (None, 0)
        };

        let sweeper_path_str = sweeper_path_opt?.trim().to_string();
        if sweeper_path_str.is_empty() {
            return None;
        }
        let sweeper_path = PathBuf::from(&sweeper_path_str);
        if !sweeper_path.exists() {
            return None;
        }

        if sweeper_dur_ms == 0 {
            if let Ok(info) = probe_track_metadata(&sweeper_path) {
                sweeper_dur_ms = info.duration_ms;
            }
        }

        let trigger_at_ms = compute_sweeper_trigger_ms(intro_ms, sweeper_dur_ms);
        let sweeper_name = sweeper_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&sweeper_path_str)
            .to_string();

        self.last_event_message = Some(format!(
            "🎙 Pisador armado: '{}' → disparo en {:.1}s (Intro {:.1}s - Pisador {:.1}s)",
            sweeper_name,
            trigger_at_ms as f64 / 1000.0,
            intro_ms as f64 / 1000.0,
            sweeper_dur_ms as f64 / 1000.0
        ));

        Some(SweeperScheduleSpec {
            sweeper_path,
            intro_ms,
            sweeper_duration_ms: sweeper_dur_ms,
            trigger_at_ms,
        })
    }

    /// Devuelve la siguiente fuente: primero los eventos/segmentos encolados, luego la lista según el modo.
    fn take_next_source(&mut self, manual_skip: bool) -> Option<NextSource> {
        if let Some(ev) = self.pending_events.pop_front() {
            return Some(NextSource::Event(ev));
        }
        self.compute_next_index(manual_skip).map(NextSource::Track)
    }

    /// Activa el ítem `idx` de la lista de reproducción resolviendo su `TrackKind`:
    /// - `File`: devuelve el archivo y lo registra en los últimos 50 reproducidos.
    /// - `RandomFolder`: escanea la carpeta, elige un archivo al azar sin repetir en los últimos 50 y lo registra.
    /// - `TimeAnnouncement` (`.time`): lee la hora `now`, resuelve `horas/son_las.*`, `horas/{HH}.*`, `horas/y_{MM}.*`,
    ///   reproduce el primero y encola los siguientes al frente de `pending_events` con `no_crossfade = true`.
    /// Devuelve `(ruta_a_reproducir, desactivar_crossfade)`.
    fn activate_playlist_index(
        &mut self,
        idx: usize,
        now: DateTime<Local>,
    ) -> Result<(String, bool), String> {
        let track = self
            .tracks
            .get(idx)
            .cloned()
            .ok_or_else(|| format!("Índice de pista fuera de rango: {}", idx))?;

        self.current_index = Some(idx);
        self.crossfade_triggered_for_current = false;

        match track.kind {
            TrackKind::File => {
                self.on_air_event = None;
                self.anti_repeat.record_played(&track.path);
                Ok((track.path, false))
            }
            TrackKind::RandomFolder => {
                let chosen_path = self.anti_repeat.pick_non_repeating_track(&track.path)?;
                let chosen_name = Path::new(&chosen_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&chosen_path)
                    .to_string();

                if let Some(slot) = self.tracks.get_mut(idx) {
                    if let Ok(info) = probe_track_metadata(Path::new(&chosen_path)) {
                        slot.duration_ms = info.duration_ms;
                        slot.sample_rate = info.sample_rate;
                    }
                    slot.last_resolved_file = Some(chosen_path.clone());
                }

                self.on_air_event = None;
                self.last_event_message = Some(format!(
                    "📁 Carpeta Aleatoria '{}' → '{}' (Historial Anti-Repeat: {}/50)",
                    track.title,
                    chosen_name,
                    self.anti_repeat.recent_history.len()
                ));
                Ok((chosen_path, false))
            }
            TrackKind::TimeAnnouncement => {
                let clips = resolve_time_announcement_sequence(&self.horas_dir, now)?;
                let mut iter = clips.into_iter();
                let first = iter
                    .next()
                    .ok_or_else(|| "La secuencia .time no generó clips.".to_string())?;
                let rest: Vec<_> = iter.collect();

                // Limpiar posibles segmentos huérfanos previos de .time (event_id == 0)
                self.pending_events.retain(|e| e.event_id != 0);

                for clip in rest.into_iter().rev() {
                    self.pending_events.push_front(EventItem {
                        event_id: 0,
                        title: clip.label,
                        path: clip.path,
                        wait_for_current: true,
                        no_crossfade: true,
                    });
                }

                let first_ev = EventItem {
                    event_id: 0,
                    title: first.label,
                    path: first.path.clone(),
                    wait_for_current: false,
                    no_crossfade: true,
                };
                self.on_air_event = Some(first_ev);
                self.last_event_message = Some(format!(
                    "🕒 Locución de Hora (.time) ejecutada para las {:02}:{:02}",
                    now.hour(),
                    now.minute()
                ));
                Ok((first.path, true))
            }
        }
    }

    /// Aplica la fuente elegida al estado y devuelve `(ruta_a_reproducir, desactivar_crossfade)`.
    fn commit_source(
        &mut self,
        source: NextSource,
        now: DateTime<Local>,
    ) -> Result<(String, bool), String> {
        self.crossfade_triggered_for_current = false;
        match source {
            NextSource::Event(ev) => {
                let path = ev.path.clone();
                let no_xfade = ev.no_crossfade;
                if !no_xfade {
                    self.anti_repeat.record_played(&path);
                }
                self.last_event_message = Some(format!("Al aire: {}", ev.title));
                self.on_air_event = Some(ev);
                Ok((path, no_xfade))
            }
            NextSource::Track(idx) => self.activate_playlist_index(idx, now),
        }
    }

    fn compute_next_index(&self, manual_skip: bool) -> Option<usize> {
        let len = self.tracks.len();
        if len == 0 {
            return None;
        }
        let Some(curr) = self.current_index else {
            return Some(0);
        };

        match self.playback_mode {
            PlaybackMode::Auto => {
                if curr + 1 < len {
                    Some(curr + 1)
                } else if manual_skip {
                    Some(0)
                } else {
                    None
                }
            }
            PlaybackMode::Manual => {
                if curr + 1 < len {
                    Some(curr + 1)
                } else if manual_skip {
                    Some(0)
                } else {
                    None
                }
            }
            PlaybackMode::Loop => Some((curr + 1) % len),
            PlaybackMode::Shuffle => {
                if len == 1 {
                    Some(0)
                } else {
                    let mut rng = rand::thread_rng();
                    let offset = rng.gen_range(1..len);
                    Some((curr + offset) % len)
                }
            }
        }
    }
}

/// Estado unificado del reproductor, modo de emisión, crossfade, Cue, ruteo, playlist y botonera de Jingles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistStatus {
    #[serde(flatten)]
    pub player: PlayerStatus,
    pub remaining_ms: u64,
    pub playback_mode: PlaybackMode,
    pub crossfade_ms: u64,
    pub playlist: Vec<TrackItem>,
    pub current_index: Option<usize>,
    pub jingles: Vec<JingleSlot>,
    /// Hito 6: eventos programados, próximo evento, cola y evento al aire.
    pub scheduled_events: Vec<ScheduledEvent>,
    pub next_event: Option<NextEventInfo>,
    pub pending_events: Vec<EventItem>,
    pub on_air_event: Option<EventItem>,
    pub last_event_message: Option<String>,
    /// Reloj del backend (`HH:MM:SS`) para la cabina.
    pub clock: String,
    /// Hito 7: Historial Anti-Repeat (últimos 50 archivos reproducidos, del más reciente al más antiguo).
    pub recent_played_history: Vec<String>,
    /// Hito 7: Ruta de la carpeta `horas/` usada por el comando `.time`.
    pub horas_dir: String,
}

#[derive(Clone)]
pub struct MinimalCore {
    pub player: Arc<AudioPlayer>,
    playlist: Arc<Mutex<PlaylistState>>,
    /// Serializa las decisiones de transición (UI a 100 ms + hilo de reloj)
    /// para que dos llamadas simultáneas nunca avancen dos pistas a la vez.
    transition_lock: Arc<Mutex<()>>,
}

impl Default for MinimalCore {
    fn default() -> Self {
        Self::new()
    }
}

impl MinimalCore {
    pub fn new() -> Self {
        let cwd = std::env::current_dir().ok();
        let default_autosave = cwd.as_ref().map(|dir| dir.join(".perpetua_last_session.json"));
        let events_path = cwd.as_ref().map(|dir| dir.join(".perpetua_events.json"));
        let db_path = cwd
            .as_ref()
            .map(|dir| dir.join(".perpetua_metadata.db"))
            .unwrap_or_else(|| PathBuf::from(".perpetua_metadata.db"));
        let horas_dir = cwd
            .as_ref()
            .map(|dir| dir.join("horas"))
            .unwrap_or_else(|| PathBuf::from("horas"));

        let core = Self {
            player: Arc::new(AudioPlayer::new()),
            playlist: Arc::new(Mutex::new(PlaylistState {
                autosave_path: default_autosave.clone(),
                events_path: events_path.clone(),
                horas_dir,
                metadata_store: TrackMetadataStore::open_or_memory(&db_path),
                ..PlaylistState::default()
            })),
            transition_lock: Arc::new(Mutex::new(())),
        };

        if let Some(ref path) = default_autosave {
            if path.exists() {
                let _ = core.load_playlist_internal(path, false);
            }
        }
        if let Some(ref path) = events_path {
            core.load_events_internal(path);
        }

        core.spawn_scheduler_clock();
        core
    }

    /// Hilo de reloj y transición sin pausas: evalúa transiciones cada 50 ms (para que los segmentos
    /// de `.time` como `son_las.mp3` -> `14.mp3` -> `y_15.mp3` se encadenen sin pausa) y evalúa el
    /// planificador de eventos cada segundo.
    fn spawn_scheduler_clock(&self) {
        let ticker = self.clone();
        let _ = std::thread::Builder::new()
            .name("perpetua-event-scheduler".to_string())
            .spawn(move || {
                let mut last_sec: i64 = -1;
                loop {
                    let now = Local::now();
                    let sec = now.timestamp();
                    if sec != last_sec {
                        last_sec = sec;
                        ticker.scheduler_tick(now);
                    }
                    let _ = ticker.get_status();
                    std::thread::sleep(Duration::from_millis(50));
                }
            });
    }

    /// Crea una instancia aislada sin auto-guardado en disco ni hilo de reloj (para tests unitarios).
    pub fn new_isolated() -> Self {
        Self {
            player: Arc::new(AudioPlayer::new()),
            playlist: Arc::new(Mutex::new(PlaylistState::default())),
            transition_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Permite configurar una carpeta personalizada `horas/` (útil para tests o configuración de cabina).
    pub fn set_horas_dir(&self, dir: impl Into<PathBuf>) {
        let mut state = self.playlist.lock().unwrap();
        state.horas_dir = dir.into();
    }

    fn trigger_autosave(&self) {
        let maybe_path = {
            let state = self.playlist.lock().unwrap();
            state.autosave_path.clone()
        };
        if let Some(path) = maybe_path {
            let _ = self.save_playlist_internal(&path);
        }
    }

    pub fn play(&self, path: Option<&str>, index: Option<usize>) -> Result<PlaylistStatus, String> {
        let _guard = self.transition_lock.lock().unwrap();
        let was_playing = self.player.get_status().is_playing;
        let now = Local::now();

        let (target_path, overlap_ms) = {
            let mut state = self.playlist.lock().unwrap();

            let (chosen_path, disable_xfade) = if let Some(idx) = index {
                state.activate_playlist_index(idx, now)?
            } else if let Some(p) = path.filter(|s| !s.trim().is_empty()) {
                if let Some(pos) = state.tracks.iter().position(|t| t.path == p) {
                    state.activate_playlist_index(pos, now)?
                } else if p.trim().eq_ignore_ascii_case(TIME_COMMAND_TOKEN) {
                    let clips = resolve_time_announcement_sequence(&state.horas_dir, now)?;
                    let mut iter = clips.into_iter();
                    let first = iter
                        .next()
                        .ok_or_else(|| "La secuencia .time no generó clips.".to_string())?;
                    for clip in iter.collect::<Vec<_>>().into_iter().rev() {
                        state.pending_events.push_front(EventItem {
                            event_id: 0,
                            title: clip.label,
                            path: clip.path,
                            wait_for_current: true,
                            no_crossfade: true,
                        });
                    }
                    state.on_air_event = Some(EventItem {
                        event_id: 0,
                        title: first.label,
                        path: first.path.clone(),
                        wait_for_current: false,
                        no_crossfade: true,
                    });
                    (first.path, true)
                } else if Path::new(p).is_dir() {
                    let chosen = state.anti_repeat.pick_non_repeating_track(p)?;
                    state.on_air_event = None;
                    (chosen, false)
                } else {
                    state.anti_repeat.record_played(p);
                    state.on_air_event = None;
                    (p.to_string(), false)
                }
            } else if let Some(curr_idx) = state.current_index {
                if curr_idx < state.tracks.len() {
                    state.activate_playlist_index(curr_idx, now)?
                } else if !state.tracks.is_empty() {
                    state.activate_playlist_index(0, now)?
                } else {
                    return Err(
                        "La lista de reproducción está vacía. Añade archivos, carpetas o comandos primero."
                            .to_string(),
                    );
                }
            } else if !state.tracks.is_empty() {
                state.activate_playlist_index(0, now)?
            } else {
                return Err(
                    "La lista de reproducción está vacía. Añade archivos, carpetas o comandos primero."
                        .to_string(),
                );
            };

            state.auto_advance_armed = true;
            state.crossfade_triggered_for_current = false;

            let overlap = if was_playing && !disable_xfade {
                state.crossfade_ms
            } else {
                0
            };
            (chosen_path, overlap)
        };

        let mut player_status = self.player.play_with_crossfade(&target_path, overlap_ms)?;
        let sweeper_spec = {
            let mut state = self.playlist.lock().unwrap();
            if state.on_air_event.is_none() {
                if let Some(idx) = state.current_index {
                    if let Some(track) = state.tracks.get_mut(idx) {
                        if player_status.duration_ms > 0 {
                            track.duration_ms = player_status.duration_ms;
                        }
                    }
                }
            }
            state.resolve_sweeper_for_active_playback(&target_path)
        };

        if let Ok(updated) = self.player.schedule_sweeper(sweeper_spec) {
            player_status = updated;
        }

        let state = self.playlist.lock().unwrap();
        Ok(Self::build_status(player_status, &state))
    }

    pub fn stop(&self) -> PlaylistStatus {
        {
            let mut state = self.playlist.lock().unwrap();
            state.auto_advance_armed = false;
            state.crossfade_triggered_for_current = false;
            state.on_air_event = None;
        }
        let player_status = self.player.stop();
        let state = self.playlist.lock().unwrap();
        Self::build_status(player_status, &state)
    }

    /// Reproduce un archivo en el canal de Preescucha (`Cue`).
    pub fn play_cue(&self, path: &str) -> Result<PlaylistStatus, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("La ruta del archivo para Preescucha (Cue) está vacía.".to_string());
        }
        let player_status = self.player.play_cue(trimmed)?;
        let state = self.playlist.lock().unwrap();
        Ok(Self::build_status(player_status, &state))
    }

    /// Salta (Seek) a un milisegundo exacto (`position_ms`) en el reproductor de Preescucha (`Cue Deck`).
    pub fn seek_cue_deck(
        &self,
        path: Option<&str>,
        position_ms: u64,
    ) -> Result<PlaylistStatus, String> {
        let clean_path = path.map(str::trim).filter(|s| !s.is_empty()).map(Path::new);
        let player_status = self.player.seek_cue(clean_path, position_ms)?;
        let state = self.playlist.lock().unwrap();
        Ok(Self::build_status(player_status, &state))
    }

    /// Devuelve la telemetría de posición en tiempo real (`current_position_ms`) del Cue Deck.
    pub fn get_cue_telemetry(&self) -> CueTelemetry {
        self.player.get_cue_telemetry()
    }

    /// Detiene el reproductor de Preescucha (`Cue`) sin afectar el Aire.
    pub fn stop_cue(&self) -> PlaylistStatus {
        let player_status = self.player.stop_cue();
        let state = self.playlist.lock().unwrap();
        Self::build_status(player_status, &state)
    }

    /// Configura las tarjetas de sonido de salida para `Main / Aire` y `Cue / Preescucha`.
    pub fn set_audio_routing(
        &self,
        air_device: String,
        cue_device: String,
    ) -> Result<PlaylistStatus, String> {
        let player_status = self.player.set_audio_routing(air_device, cue_device)?;
        let state = self.playlist.lock().unwrap();
        Ok(Self::build_status(player_status, &state))
    }

    /// Actualiza la lista de tarjetas de sonido detectadas en el sistema.
    pub fn refresh_devices(&self) -> PlaylistStatus {
        let _ = self.player.refresh_devices();
        self.get_status()
    }

    pub fn set_playback_mode(&self, mode: PlaybackMode) -> PlaylistStatus {
        {
            let mut state = self.playlist.lock().unwrap();
            state.playback_mode = mode;
        }
        self.trigger_autosave();
        self.get_status()
    }

    pub fn set_crossfade(&self, crossfade_ms: u64) -> PlaylistStatus {
        {
            let mut state = self.playlist.lock().unwrap();
            state.crossfade_ms = crossfade_ms.min(5000);
        }
        self.trigger_autosave();
        self.get_status()
    }

    pub fn assign_jingle(
        &self,
        slot: usize,
        path: Option<&str>,
    ) -> Result<PlaylistStatus, String> {
        if !(1..=9).contains(&slot) {
            return Err(format!(
                "Número de ranura de Jingle inválido ({}): debe estar entre 1 y 9",
                slot
            ));
        }

        match path.map(str::trim).filter(|s| !s.is_empty()) {
            Some(clean_path) => {
                let path_obj = Path::new(clean_path);
                if !path_obj.exists() {
                    return Err(format!("El archivo de Jingle no existe: {}", clean_path));
                }

                let probe = probe_track_metadata(path_obj)?;
                let fallback_title = path_obj
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(clean_path)
                    .to_string();
                let display_title = match (probe.artist, probe.title) {
                    (Some(a), Some(t)) => format!("{} - {}", a, t),
                    (None, Some(t)) => t,
                    _ => fallback_title,
                };

                let mut state = self.playlist.lock().unwrap();
                if let Some(jingle_slot) = state.jingles.get_mut(slot - 1) {
                    jingle_slot.path = Some(clean_path.to_string());
                    jingle_slot.title = Some(display_title);
                    jingle_slot.duration_ms = probe.duration_ms;
                }
            }
            None => {
                let mut state = self.playlist.lock().unwrap();
                if let Some(jingle_slot) = state.jingles.get_mut(slot - 1) {
                    jingle_slot.path = None;
                    jingle_slot.title = None;
                    jingle_slot.duration_ms = 0;
                    jingle_slot.is_playing = false;
                }
            }
        }

        self.trigger_autosave();
        Ok(self.get_status())
    }

    pub fn play_jingle(&self, slot: usize) -> Result<PlaylistStatus, String> {
        if !(1..=9).contains(&slot) {
            return Err(format!(
                "Número de ranura de Jingle inválido ({}): debe estar entre 1 y 9",
                slot
            ));
        }

        let target_path = {
            let state = self.playlist.lock().unwrap();
            let jingle = state
                .jingles
                .get(slot - 1)
                .ok_or_else(|| format!("Ranura de Jingle #{} no encontrada", slot))?;
            jingle.path.clone().ok_or_else(|| {
                format!(
                    "La ranura de Jingle #{} está vacía. Haz clic derecho o pulsa 'Asignar' para cargar un audio.",
                    slot
                )
            })?
        };

        let player_status = self.player.play_jingle(slot, &target_path)?;
        let state = self.playlist.lock().unwrap();
        Ok(Self::build_status(player_status, &state))
    }

    /// Clasifica una entrada de la lista de reproducción:
    /// - `".time"` -> `TrackKind::TimeAnnouncement`
    /// - Directorio existente -> `TrackKind::RandomFolder`
    /// - Archivo de audio -> `TrackKind::File`
    fn classify_playlist_entry(
        raw_path: &str,
        override_title: Option<String>,
        override_duration_ms: Option<u64>,
    ) -> Option<TrackItem> {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return None;
        }

        if trimmed.eq_ignore_ascii_case(TIME_COMMAND_TOKEN) {
            return Some(TrackItem {
                id: 0,
                path: TIME_COMMAND_TOKEN.to_string(),
                title: override_title.unwrap_or_else(|| "Locución de Hora (.time)".to_string()),
                duration_ms: 0,
                sample_rate: 44100,
                kind: TrackKind::TimeAnnouncement,
                last_resolved_file: None,
                intro_ms: 0,
                sweeper_path: None,
                sweeper_title: None,
                sweeper_duration_ms: 0,
                sweeper_trigger_ms: None,
            });
        }

        let path_obj = Path::new(trimmed);
        if path_obj.is_dir() {
            let folder_name = path_obj
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(trimmed);
            let count = scan_audio_files_recursive(path_obj).len();
            let default_title = format!("{} ({} audios)", folder_name, count);
            return Some(TrackItem {
                id: 0,
                path: trimmed.to_string(),
                title: override_title.unwrap_or(default_title),
                duration_ms: 0,
                sample_rate: 44100,
                kind: TrackKind::RandomFolder,
                last_resolved_file: None,
                intro_ms: 0,
                sweeper_path: None,
                sweeper_title: None,
                sweeper_duration_ms: 0,
                sweeper_trigger_ms: None,
            });
        }

        let file_name = path_obj
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(trimmed)
            .to_string();

        let (probed_title, duration_ms, sample_rate, probed_intro_ms) = match override_duration_ms {
            Some(d) if d > 0 => (file_name, d, 44100, 0),
            _ => match probe_track_metadata(path_obj) {
                Ok(info) => {
                    let display_title = match (info.artist, info.title) {
                        (Some(a), Some(t)) => format!("{} - {}", a, t),
                        (None, Some(t)) => t,
                        _ => file_name,
                    };
                    (
                        display_title,
                        info.duration_ms,
                        info.sample_rate,
                        info.intro_ms.unwrap_or(0),
                    )
                }
                Err(_) => (file_name, 0, 44100, 0),
            },
        };

        Some(TrackItem {
            id: 0,
            path: trimmed.to_string(),
            title: override_title.unwrap_or(probed_title),
            duration_ms,
            sample_rate,
            kind: TrackKind::File,
            last_resolved_file: None,
            intro_ms: probed_intro_ms,
            sweeper_path: None,
            sweeper_title: None,
            sweeper_duration_ms: 0,
            sweeper_trigger_ms: None,
        })
    }

    pub fn add_tracks(&self, paths: Vec<String>) -> PlaylistStatus {
        let mut classified = Vec::with_capacity(paths.len());
        for raw_path in paths {
            if let Some(item) = Self::classify_playlist_entry(&raw_path, None, None) {
                classified.push(item);
            }
        }

        {
            let mut state = self.playlist.lock().unwrap();
            for mut item in classified {
                state.hydrate_track_item(&mut item);
                state.next_id += 1;
                item.id = state.next_id;
                state.tracks.push(item);
            }
        }

        self.trigger_autosave();
        self.get_status()
    }

    /// Hito 7: Añade una Carpeta Aleatoria (`Random Folder`) validando que el directorio exista.
    pub fn add_random_folder(&self, folder_path: &str) -> Result<PlaylistStatus, String> {
        let trimmed = folder_path.trim();
        if trimmed.is_empty() {
            return Err("La ruta de la carpeta está vacía.".to_string());
        }
        let path_obj = Path::new(trimmed);
        if !path_obj.exists() || !path_obj.is_dir() {
            return Err(format!("El directorio especificado no existe: {}", trimmed));
        }
        let files = scan_audio_files_recursive(path_obj);
        if files.is_empty() {
            return Err(format!(
                "La carpeta '{}' no contiene archivos de audio (.wav, .mp3, .flac, .ogg).",
                trimmed
            ));
        }
        Ok(self.add_tracks(vec![trimmed.to_string()]))
    }

    /// Hito 7: Añade el comando virtual `.time` (Locución de Hora) a la lista de reproducción.
    pub fn add_time_announcement(&self) -> PlaylistStatus {
        self.add_tracks(vec![TIME_COMMAND_TOKEN.to_string()])
    }

    /// Lee los archivos de audio de una carpeta sin cargarlos ni decodificarlos en memoria.
    /// Se usa exclusivamente para desplegar sub-filas visuales en la tabla de React mientras
    /// el motor de Rust sigue tratando la carpeta como un único slot aleatorio con Anti-Repeat.
    pub fn get_folder_contents(&self, folder_path: &str) -> Result<Vec<FolderAudioEntry>, String> {
        let trimmed = folder_path.trim();
        if trimmed.is_empty() {
            return Err("La ruta de la carpeta está vacía.".to_string());
        }
        let path_obj = Path::new(trimmed);
        if !path_obj.exists() || !path_obj.is_dir() {
            return Err(format!("El directorio no existe: {}", trimmed));
        }

        let files = scan_audio_files_recursive(path_obj);
        let state = self.playlist.lock().unwrap();
        let recent_set: std::collections::HashSet<String> =
            state.anti_repeat.recent_history.iter().cloned().collect();

        let entries = files
            .into_iter()
            .map(|path_str| {
                let p = Path::new(&path_str);
                let title = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&path_str)
                    .to_string();
                let extension = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_uppercase();
                let size_bytes = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                let in_recent_history = recent_set.contains(&path_str);
                let meta = state.metadata_store.get(&path_str);
                let intro_ms = meta.as_ref().map(|m| m.intro_ms).unwrap_or(0);
                let sweeper_path = meta.as_ref().and_then(|m| m.sweeper_path.clone());
                let sweeper_duration_ms = meta.as_ref().map(|m| m.sweeper_duration_ms).unwrap_or(0);
                let sweeper_title = sweeper_path.as_ref().map(|sp| {
                    Path::new(sp)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(sp)
                        .to_string()
                });
                FolderAudioEntry {
                    path: path_str,
                    title,
                    extension,
                    size_bytes,
                    in_recent_history,
                    intro_ms,
                    sweeper_path,
                    sweeper_title,
                    sweeper_duration_ms,
                }
            })
            .collect();

        Ok(entries)
    }

    // =========================================================================
    // Hito 8: Gestor de Metadatos (Intro), Motor de Sweepers y Editor Visual
    // =========================================================================

    /// Guarda el 'Tiempo de Intro' (`intro_ms` en milisegundos, ej. `12500` para `00:12.5`)
    /// para una canción en la base de datos SQLite y actualiza las filas correspondientes de la Playlist.
    pub fn set_track_intro(&self, path: &str, intro_ms: u64) -> Result<PlaylistStatus, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("La ruta de la pista para guardar el Tiempo de Intro está vacía.".to_string());
        }

        let maybe_rearm_spec = {
            let mut state = self.playlist.lock().unwrap();
            state.metadata_store.set_intro_ms(trimmed, intro_ms)?;

            for track in &mut state.tracks {
                if track.path == trimmed {
                    track.intro_ms = intro_ms;
                    track.recompute_sweeper_fields();
                }
            }

            let title_name = Path::new(trimmed)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(trimmed);
            state.last_event_message = Some(format!(
                "🚩 Tiempo de Intro actualizado para '{}': {:02}:{:04.1}",
                title_name,
                (intro_ms / 1000) / 60,
                (intro_ms % 60_000) as f64 / 1000.0
            ));

            let player_st = self.player.get_status();
            if player_st.is_playing
                && player_st.current_file.as_deref() == Some(trimmed)
                && !player_st.sweeper_is_playing
            {
                Some(state.resolve_sweeper_for_active_playback(trimmed))
            } else {
                None
            }
        };

        if let Some(spec) = maybe_rearm_spec {
            let _ = self.player.schedule_sweeper(spec);
        }

        self.trigger_autosave();
        Ok(self.get_status())
    }

    /// Asigna o limpia un Pisador (`sweeper_path`) para una pista de la Playlist (por `index` y/o `path`),
    /// persistiéndolo en SQLite y calculando el instante exacto de disparo `(Tiempo de Intro) - (Duración del Pisador)`.
    pub fn set_track_sweeper(
        &self,
        index: Option<usize>,
        path: Option<&str>,
        sweeper_path: Option<&str>,
    ) -> Result<PlaylistStatus, String> {
        let clean_sweeper = sweeper_path.map(str::trim).filter(|s| !s.is_empty());

        let (validated_sweeper, sweeper_dur_ms) = if let Some(sp) = clean_sweeper {
            let sp_obj = Path::new(sp);
            if !sp_obj.exists() {
                return Err(format!("El archivo de Pisador (Sweeper) no existe: {}", sp));
            }
            let probe = probe_track_metadata(sp_obj)?;
            (Some(sp.to_string()), probe.duration_ms)
        } else {
            (None, 0)
        };

        let maybe_rearm_spec = {
            let mut state = self.playlist.lock().unwrap();
            let resolved_target_path = if let Some(p) = path.map(str::trim).filter(|s| !s.is_empty()) {
                Some(p.to_string())
            } else if let Some(idx) = index {
                state.tracks.get(idx).map(|t| t.path.clone())
            } else {
                None
            };

            let target_path = resolved_target_path.ok_or_else(|| {
                "Debes indicar el índice o la ruta de la pista para asignar el Pisador.".to_string()
            })?;

            state.metadata_store.set_sweeper(
                &target_path,
                validated_sweeper.as_deref(),
                sweeper_dur_ms,
            )?;

            if let Some(idx) = index {
                if let Some(track) = state.tracks.get_mut(idx) {
                    track.sweeper_path = validated_sweeper.clone();
                    track.sweeper_duration_ms = sweeper_dur_ms;
                    track.recompute_sweeper_fields();
                }
            } else {
                for track in &mut state.tracks {
                    if track.path == target_path {
                        track.sweeper_path = validated_sweeper.clone();
                        track.sweeper_duration_ms = sweeper_dur_ms;
                        track.recompute_sweeper_fields();
                    }
                }
            }

            let track_name = Path::new(&target_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&target_path)
                .to_string();

            if let Some(ref sp) = validated_sweeper {
                let sw_name = Path::new(sp)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(sp);
                let intro_ms = state
                    .metadata_store
                    .get(&target_path)
                    .map(|r| r.intro_ms)
                    .unwrap_or(0);
                let trigger_ms = compute_sweeper_trigger_ms(intro_ms, sweeper_dur_ms);
                state.last_event_message = Some(format!(
                    "🎙 Pisador '{}' asignado a '{}' → disparo en {:.1}s (Intro {:.1}s - Pisador {:.1}s)",
                    sw_name,
                    track_name,
                    trigger_ms as f64 / 1000.0,
                    intro_ms as f64 / 1000.0,
                    sweeper_dur_ms as f64 / 1000.0
                ));
            } else {
                state.last_event_message =
                    Some(format!("🎙 Pisador desasignado de '{}'", track_name));
            }

            let player_st = self.player.get_status();
            let active_matches = player_st.current_file.as_deref() == Some(target_path.as_str())
                || (index.is_some() && state.current_index == index);
            if player_st.is_playing && active_matches && !player_st.sweeper_is_playing {
                if let Some(ref cur_file) = player_st.current_file {
                    Some(state.resolve_sweeper_for_active_playback(cur_file))
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(spec) = maybe_rearm_spec {
            let _ = self.player.schedule_sweeper(spec);
        }

        self.trigger_autosave();
        Ok(self.get_status())
    }

    /// Guarda de forma atómica el Tiempo de Intro (`intro_ms`) y el Pisador (`sweeper_path`)
    /// desde el modal 'Editar Marcadores'.
    pub fn set_track_markers(
        &self,
        index: Option<usize>,
        path: &str,
        intro_ms: u64,
        sweeper_path: Option<&str>,
    ) -> Result<PlaylistStatus, String> {
        let _ = self.set_track_intro(path, intro_ms)?;
        self.set_track_sweeper(index, Some(path), sweeper_path)
    }

    /// Extrae la envolvente de picos PCM de una pista (`peaks`) junto con sus marcadores de Intro y Pisador
    /// para el Editor Visual de Puntos de Mezcla en React.
    pub fn get_track_waveform(
        &self,
        path: &str,
        buckets: Option<usize>,
    ) -> Result<WaveformEditorData, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("La ruta del archivo para el Editor de Marcadores está vacía.".to_string());
        }
        let path_obj = Path::new(trimmed);
        if !path_obj.exists() || path_obj.is_dir() {
            return Err(format!(
                "El archivo de audio para inspeccionar la forma de onda no existe: {}",
                trimmed
            ));
        }

        let wf = extract_waveform_peaks(path_obj, buckets.unwrap_or(240))?;
        let probe = probe_track_metadata(path_obj).ok();
        let fallback_title = path_obj
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(trimmed)
            .to_string();
        let title = probe
            .as_ref()
            .and_then(|info| match (&info.artist, &info.title) {
                (Some(a), Some(t)) => Some(format!("{} - {}", a, t)),
                (None, Some(t)) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or(fallback_title);

        let state = self.playlist.lock().unwrap();
        let record = state.metadata_store.get(trimmed);
        let playlist_track = state.tracks.iter().find(|t| t.path == trimmed);

        let intro_ms = record
            .as_ref()
            .map(|r| r.intro_ms)
            .filter(|&v| v > 0)
            .or_else(|| playlist_track.map(|t| t.intro_ms).filter(|&v| v > 0))
            .or_else(|| probe.and_then(|p| p.intro_ms))
            .unwrap_or(0);

        let (sweeper_path, mut sweeper_duration_ms) = record
            .as_ref()
            .and_then(|r| r.sweeper_path.clone().map(|sp| (Some(sp), r.sweeper_duration_ms)))
            .or_else(|| {
                playlist_track.and_then(|t| {
                    t.sweeper_path
                        .clone()
                        .map(|sp| (Some(sp), t.sweeper_duration_ms))
                })
            })
            .unwrap_or((None, 0));

        let sweeper_title = sweeper_path.as_ref().map(|sp| {
            let sp_path = Path::new(sp);
            if sweeper_duration_ms == 0 && sp_path.exists() {
                if let Ok(info) = probe_track_metadata(sp_path) {
                    sweeper_duration_ms = info.duration_ms;
                }
            }
            sp_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(sp)
                .to_string()
        });

        let sweeper_trigger_ms = sweeper_path
            .as_ref()
            .map(|_| compute_sweeper_trigger_ms(intro_ms, sweeper_duration_ms));

        Ok(WaveformEditorData {
            path: trimmed.to_string(),
            title,
            duration_ms: wf.duration_ms,
            sample_rate: wf.sample_rate,
            peaks: wf.peaks,
            intro_ms,
            sweeper_path,
            sweeper_title,
            sweeper_duration_ms,
            sweeper_trigger_ms,
        })
    }

    pub fn remove_track(&self, index: usize) -> Result<PlaylistStatus, String> {
        let mut stop_needed = false;
        {
            let mut state = self.playlist.lock().unwrap();
            if index >= state.tracks.len() {
                return Err(format!("Índice fuera de rango al eliminar pista: {}", index));
            }
            state.tracks.remove(index);

            if let Some(curr) = state.current_index {
                if index == curr && state.on_air_event.is_some() {
                    // La pista no está sonando (suena un evento): solo reajustar el puntero
                    // para que al terminar el evento continúe la pista que venía después.
                    state.current_index = index.checked_sub(1);
                } else if index == curr {
                    state.current_index = None;
                    state.auto_advance_armed = false;
                    state.crossfade_triggered_for_current = false;
                    stop_needed = true;
                } else if index < curr {
                    state.current_index = Some(curr - 1);
                }
            }
        }
        if stop_needed {
            self.player.stop();
        }
        self.trigger_autosave();
        Ok(self.get_status())
    }

    pub fn clear_playlist(&self) -> PlaylistStatus {
        {
            let mut state = self.playlist.lock().unwrap();
            state.tracks.clear();
            state.current_index = None;
            state.auto_advance_armed = false;
            state.crossfade_triggered_for_current = false;
            state.on_air_event = None;
        }
        let player_status = self.player.stop();
        self.trigger_autosave();
        let state = self.playlist.lock().unwrap();
        Self::build_status(player_status, &state)
    }

    pub fn next_track(&self) -> Result<PlaylistStatus, String> {
        let _guard = self.transition_lock.lock().unwrap();
        let was_playing = self.player.get_status().is_playing;
        let now = Local::now();

        let (target_path, overlap_ms) = {
            let mut state = self.playlist.lock().unwrap();
            let source = state.take_next_source(true).ok_or_else(|| {
                "La lista de reproducción y la cola de eventos están vacías.".to_string()
            })?;
            state.auto_advance_armed = true;
            let (path, disable_xfade) = state.commit_source(source, now)?;
            let overlap = if was_playing && !disable_xfade {
                state.crossfade_ms
            } else {
                0
            };
            (path, overlap)
        };

        let mut player_status = self.player.play_with_crossfade(&target_path, overlap_ms)?;
        let sweeper_spec = {
            let mut state = self.playlist.lock().unwrap();
            if state.on_air_event.is_none() {
                if let Some(idx) = state.current_index {
                    if let Some(track) = state.tracks.get_mut(idx) {
                        if player_status.duration_ms > 0 {
                            track.duration_ms = player_status.duration_ms;
                        }
                    }
                }
            }
            state.resolve_sweeper_for_active_playback(&target_path)
        };

        if let Ok(updated) = self.player.schedule_sweeper(sweeper_spec) {
            player_status = updated;
        }

        let state = self.playlist.lock().unwrap();
        Ok(Self::build_status(player_status, &state))
    }

    // =========================================================================
    // Hito 6: Planificador de Eventos (Event Scheduler) y Persistencia
    // =========================================================================

    fn trigger_events_autosave(&self) {
        let maybe_path = {
            let state = self.playlist.lock().unwrap();
            state.events_path.clone()
        };
        if let Some(path) = maybe_path {
            let _ = self.save_events_internal(&path);
        }
    }

    fn save_events_internal(&self, path: &Path) -> Result<(), String> {
        let events: Vec<ScheduledEvent> = {
            let state = self.playlist.lock().unwrap();
            state.scheduler.events().to_vec()
        };
        let json_str = serde_json::to_string_pretty(&events)
            .map_err(|e| format!("Error al serializar eventos programados: {}", e))?;
        fs::write(path, json_str)
            .map_err(|e| format!("No se pudo guardar '{}': {}", path.display(), e))?;
        Ok(())
    }

    fn load_events_internal(&self, path: &Path) {
        if !path.exists() {
            return;
        }
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(events) = serde_json::from_str::<Vec<ScheduledEvent>>(&content) {
                let mut state = self.playlist.lock().unwrap();
                state.scheduler = EventScheduler::from_events(events);
            }
        }
    }

    /// Añade un nuevo evento programado con compatibilidad hacia atrás (`wait_for_current`).
    pub fn add_event(
        &self,
        time: String,
        days: Vec<u8>,
        path: String,
        wait_for_current: bool,
    ) -> Result<PlaylistStatus, String> {
        let mode = if wait_for_current {
            EventInterruptionMode::WaitForCurrent
        } else {
            EventInterruptionMode::Ducking
        };
        self.add_event_with_mode(time, days, path, mode)
    }

    /// Añade un nuevo evento programado especificando el modo de interrupción no destructiva
    /// (`Ducking` -70%, `PauseResume` o `WaitForCurrent`).
    pub fn add_event_with_mode(
        &self,
        time: String,
        days: Vec<u8>,
        path: String,
        interruption_mode: EventInterruptionMode,
    ) -> Result<PlaylistStatus, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("Debes seleccionar un archivo de audio, carpeta, lista o .time para el evento.".to_string());
        }

        let title = if trimmed.eq_ignore_ascii_case(TIME_COMMAND_TOKEN) {
            "🕒 Locución de Hora (.time)".to_string()
        } else {
            let path_obj = Path::new(trimmed);
            if !path_obj.exists() {
                return Err(format!(
                    "El archivo, carpeta o lista del evento no existe: {}",
                    trimmed
                ));
            }

            let fallback_title = path_obj
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(trimmed)
                .to_string();

            if path_obj.is_dir() {
                format!("📁 {}", fallback_title)
            } else {
                let ext = path_obj
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();

                if ext == "m3u8" || ext == "m3u" || ext == "json" {
                    format!("📋 {}", fallback_title)
                } else {
                    match probe_track_metadata(path_obj) {
                        Ok(info) => match (info.artist, info.title) {
                            (Some(a), Some(t)) => format!("{} - {}", a, t),
                            (None, Some(t)) => t,
                            _ => fallback_title,
                        },
                        Err(_) => fallback_title,
                    }
                }
            }
        };

        {
            let mut state = self.playlist.lock().unwrap();
            let ev = state
                .scheduler
                .add_with_mode(&time, &days, trimmed, &title, interruption_mode)?;
            let mode_label = match ev.interruption_mode {
                EventInterruptionMode::Ducking => "Ducking -70%",
                EventInterruptionMode::PauseResume => "Pausa y Reanuda",
                EventInterruptionMode::WaitForCurrent => "Espera al Final",
            };
            state.last_event_message = Some(format!(
                "Evento programado [{}]: '{}' a las {}",
                mode_label, ev.title, ev.time
            ));
        }

        self.trigger_events_autosave();
        Ok(self.get_status())
    }

    /// Elimina un evento programado por su `id` (y limpia entradas pendientes en cola de ese mismo evento).
    pub fn remove_event(&self, id: u64) -> Result<PlaylistStatus, String> {
        {
            let mut state = self.playlist.lock().unwrap();
            if !state.scheduler.remove(id) {
                return Err(format!("No se encontró el evento con ID #{}.", id));
            }
            state.pending_events.retain(|e| e.event_id != id);
        }
        self.trigger_events_autosave();
        Ok(self.get_status())
    }

    /// Activa o desactiva un evento programado sin eliminarlo.
    pub fn toggle_event(&self, id: u64, enabled: bool) -> Result<PlaylistStatus, String> {
        {
            let mut state = self.playlist.lock().unwrap();
            if !state.scheduler.set_enabled(id, enabled) {
                return Err(format!("No se encontró el evento con ID #{}.", id));
            }
        }
        self.trigger_events_autosave();
        Ok(self.get_status())
    }

    /// Permite disparar manualmente un evento programado desde la tabla de eventos (para pruebas o adelanto en cabina).
    pub fn trigger_event_now(&self, id: u64) -> Result<PlaylistStatus, String> {
        let ev = {
            let state = self.playlist.lock().unwrap();
            state
                .scheduler
                .get(id)
                .cloned()
                .ok_or_else(|| format!("No se encontró el evento con ID #{}.", id))?
        };
        self.execute_scheduled_event(ev)?;
        Ok(self.get_status())
    }

    /// Evalúa un tick del reloj sobre el planificador y ejecuta todos los eventos cuya hora haya llegado.
    pub fn scheduler_tick(&self, now: DateTime<Local>) {
        let due_list = {
            let mut state = self.playlist.lock().unwrap();
            state.scheduler.due_events(now)
        };

        for ev in due_list {
            let _ = self.execute_scheduled_event(ev);
        }
    }

    /// Expande un `ScheduledEvent` en uno o varios `EventItem` (soporta `.time`, carpetas aleatorias,
    /// listas `.m3u8`/`.m3u`/`.json` y archivos de audio individuales).
    fn expand_event_items(&self, ev: &ScheduledEvent, now: DateTime<Local>) -> Vec<EventItem> {
        if ev.path.trim().eq_ignore_ascii_case(TIME_COMMAND_TOKEN) {
            let horas_dir = {
                let state = self.playlist.lock().unwrap();
                state.horas_dir.clone()
            };
            if let Ok(clips) = resolve_time_announcement_sequence(&horas_dir, now) {
                return clips
                    .into_iter()
                    .map(|c| EventItem {
                        event_id: ev.id,
                        title: c.label,
                        path: c.path,
                        wait_for_current: ev.wait_for_current,
                        no_crossfade: true,
                    })
                    .collect();
            }
        }

        let p = Path::new(&ev.path);
        if p.is_dir() {
            let mut state = self.playlist.lock().unwrap();
            if let Ok(chosen) = state.anti_repeat.pick_non_repeating_track(&ev.path) {
                let chosen_name = Path::new(&chosen)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&chosen);
                return vec![EventItem {
                    event_id: ev.id,
                    title: format!("{} · {}", ev.title, chosen_name),
                    path: chosen,
                    wait_for_current: ev.wait_for_current,
                    no_crossfade: false,
                }];
            }
        }

        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if (ext == "m3u8" || ext == "m3u" || ext == "json") && p.exists() {
            if let Ok(content) = fs::read_to_string(p) {
                let trimmed = content.trim();
                let mut items = Vec::new();
                if trimmed.starts_with('{') {
                    if let Ok(doc) = serde_json::from_str::<SavedPlaylistDocument>(trimmed) {
                        for t in doc.tracks {
                            items.push(EventItem {
                                event_id: ev.id,
                                title: format!("{} · {}", ev.title, t.title),
                                path: t.path,
                                wait_for_current: ev.wait_for_current,
                                no_crossfade: false,
                            });
                        }
                    }
                } else {
                    for raw_line in content.lines() {
                        let line = raw_line.trim();
                        if !line.is_empty() && !line.starts_with('#') {
                            let track_name = Path::new(line)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(line);
                            items.push(EventItem {
                                event_id: ev.id,
                                title: format!("{} · {}", ev.title, track_name),
                                path: line.to_string(),
                                wait_for_current: ev.wait_for_current,
                                no_crossfade: false,
                            });
                        }
                    }
                }
                if !items.is_empty() {
                    return items;
                }
            }
        }

        vec![EventItem {
            event_id: ev.id,
            title: ev.title.clone(),
            path: ev.path.clone(),
            wait_for_current: ev.wait_for_current,
            no_crossfade: false,
        }]
    }

    /// Inyecta un evento en la emisión según su modo de interrupción (`Ducking`, `PauseResume` o `WaitForCurrent`)
    /// sin destruir ni saltar la canción activa del Main Deck.
    fn execute_scheduled_event(&self, ev: ScheduledEvent) -> Result<(), String> {
        let _guard = self.transition_lock.lock().unwrap();
        let now = Local::now();
        let items = self.expand_event_items(&ev, now);
        if items.is_empty() {
            return Ok(());
        }

        let effective_mode = if ev.wait_for_current
            && ev.interruption_mode == EventInterruptionMode::Ducking
        {
            EventInterruptionMode::WaitForCurrent
        } else {
            ev.interruption_mode
        };

        let current_player = self.player.get_status();
        let is_playing = current_player.is_playing;

        match effective_mode {
            EventInterruptionMode::Ducking | EventInterruptionMode::PauseResume => {
                let overlay_mode = match effective_mode {
                    EventInterruptionMode::Ducking => EventOverlayMode::Ducking,
                    EventInterruptionMode::PauseResume => EventOverlayMode::PauseResume,
                    EventInterruptionMode::WaitForCurrent => unreachable!(),
                };
                let overlay_paths: Vec<PathBuf> =
                    items.iter().map(|i| PathBuf::from(&i.path)).collect();

                {
                    let mut state = self.playlist.lock().unwrap();
                    for item in &items {
                        if !item.no_crossfade {
                            state.anti_repeat.record_played(&item.path);
                        }
                    }
                    let mode_desc = match overlay_mode {
                        EventOverlayMode::Ducking => "🔉 Ducking -70% (Talkover)",
                        EventOverlayMode::PauseResume => "⏸ Pausa y Reanudación",
                    };
                    state.last_event_message = Some(format!(
                        "📡 Evento No Destructivo [{}]: '{}' ({})",
                        mode_desc, ev.title, ev.time
                    ));
                }

                self.player.play_event_overlay(overlay_paths, overlay_mode)?;
                Ok(())
            }
            EventInterruptionMode::WaitForCurrent => {
                if is_playing {
                    let mut state = self.playlist.lock().unwrap();
                    for item in items {
                        state.pending_events.push_back(item);
                    }
                    state.auto_advance_armed = true;
                    state.last_event_message = Some(format!(
                        "⏳ Evento en espera: '{}' (sonará al terminar la pista actual)",
                        ev.title
                    ));
                    Ok(())
                } else {
                    let mut iter = items.into_iter();
                    let first = iter.next().unwrap();
                    let rest: Vec<EventItem> = iter.collect();

                    {
                        let mut state = self.playlist.lock().unwrap();
                        for item in rest.into_iter().rev() {
                            state.pending_events.push_front(item);
                        }
                        if !first.no_crossfade {
                            state.anti_repeat.record_played(&first.path);
                        }
                        state.auto_advance_armed = true;
                        state.crossfade_triggered_for_current = false;
                        state.on_air_event = Some(first.clone());
                        state.last_event_message = Some(format!(
                            "📡 Evento en emisión: '{}' ({})",
                            first.title, ev.time
                        ));
                    }

                    self.player.play_with_crossfade(&first.path, 0)?;
                    Ok(())
                }
            }
        }
    }

    /// 3. Guardar Lista de Reproducción (`.m3u8` / `.m3u` o `.json` estructurado)
    /// conservando pistas, carpetas aleatorias, comandos `.time`, modo de emisión, crossfade y botonera de jingles.
    pub fn save_playlist(&self, file_path: &str) -> Result<PlaylistStatus, String> {
        let trimmed = file_path.trim();
        if trimmed.is_empty() {
            return Err("La ruta de destino para guardar la lista está vacía.".to_string());
        }
        self.save_playlist_internal(Path::new(trimmed))?;
        Ok(self.get_status())
    }

    fn save_playlist_internal(&self, path: &Path) -> Result<(), String> {
        let doc = {
            let state = self.playlist.lock().unwrap();
            SavedPlaylistDocument {
                version: "2.0".to_string(),
                playback_mode: state.playback_mode,
                crossfade_ms: state.crossfade_ms,
                tracks: state.tracks.clone(),
                jingles: state.jingles.clone(),
            }
        };

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "m3u8" || ext == "m3u" {
            let mut out = String::from("#EXTM3U\n");
            out.push_str(&format!("#EXTPERPETUA-MODE:{}\n", doc.playback_mode.as_str()));
            out.push_str(&format!("#EXTPERPETUA-CROSSFADE:{}\n", doc.crossfade_ms));
            for j in &doc.jingles {
                if let Some(ref jpath) = j.path {
                    out.push_str(&format!(
                        "#EXTPERPETUA-JINGLE:{}|{}|{}\n",
                        j.slot, j.duration_ms, jpath
                    ));
                }
            }
            for t in &doc.tracks {
                if t.intro_ms > 0 || t.sweeper_path.is_some() {
                    out.push_str(&format!(
                        "#EXTPERPETUA-META:{}|{}|{}\n",
                        t.intro_ms,
                        t.sweeper_duration_ms,
                        t.sweeper_path.as_deref().unwrap_or("")
                    ));
                }
                let secs = (t.duration_ms / 1000) as i64;
                out.push_str(&format!("#EXTINF:{},{}\n{}\n", secs, t.title, t.path));
            }
            fs::write(path, out)
                .map_err(|e| format!("No se pudo escribir el archivo M3U8 '{}': {}", path.display(), e))?;
        } else {
            let json_str = serde_json::to_string_pretty(&doc)
                .map_err(|e| format!("Error al serializar lista JSON: {}", e))?;
            fs::write(path, json_str)
                .map_err(|e| format!("No se pudo escribir el archivo JSON '{}': {}", path.display(), e))?;
        }

        Ok(())
    }

    /// 3. Cargar Lista de Reproducción desde disco (`.m3u8`, `.m3u` o `.json`).
    pub fn load_playlist(&self, file_path: &str) -> Result<PlaylistStatus, String> {
        let trimmed = file_path.trim();
        if trimmed.is_empty() {
            return Err("La ruta del archivo de lista está vacía.".to_string());
        }
        self.load_playlist_internal(Path::new(trimmed), true)?;
        Ok(self.get_status())
    }

    fn load_playlist_internal(&self, path: &Path, trigger_save: bool) -> Result<(), String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("No se pudo leer la lista '{}': {}", path.display(), e))?;

        let trimmed_content = content.trim();
        if trimmed_content.starts_with('{') {
            let doc: SavedPlaylistDocument = serde_json::from_str(trimmed_content)
                .map_err(|e| format!("Formato JSON de lista inválido: {}", e))?;

            let mut state = self.playlist.lock().unwrap();
            state.playback_mode = doc.playback_mode;
            state.crossfade_ms = doc.crossfade_ms.min(5000);
            state.tracks.clear();
            state.current_index = None;
            state.auto_advance_armed = false;
            state.crossfade_triggered_for_current = false;

            for mut t in doc.tracks {
                if t.path.trim().eq_ignore_ascii_case(TIME_COMMAND_TOKEN) {
                    t.kind = TrackKind::TimeAnnouncement;
                } else if Path::new(&t.path).is_dir() {
                    t.kind = TrackKind::RandomFolder;
                }
                state.hydrate_track_item(&mut t);
                state.next_id += 1;
                t.id = state.next_id;
                state.tracks.push(t);
            }

            if doc.jingles.len() == 9 {
                state.jingles = doc.jingles;
            }
        } else {
            // Parsear formato estándar .m3u8 / .m3u (con directivas #EXTPERPETUA-* opcionales)
            let mut mode = PlaybackMode::Auto;
            let mut crossfade_ms = 2000u64;
            let mut pending_title: Option<String> = None;
            let mut pending_duration_ms: Option<u64> = None;
            let mut pending_meta: Option<(u64, u64, Option<String>)> = None;
            let mut loaded_tracks: Vec<TrackItem> = Vec::new();
            let mut loaded_jingles: Vec<(usize, u64, String)> = Vec::new();

            for raw_line in content.lines() {
                let line = raw_line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("#EXTPERPETUA-MODE:") {
                    mode = PlaybackMode::from_str_lossy(rest);
                } else if let Some(rest) = line.strip_prefix("#EXTPERPETUA-CROSSFADE:") {
                    if let Ok(ms) = rest.trim().parse::<u64>() {
                        crossfade_ms = ms.min(5000);
                    }
                } else if let Some(rest) = line.strip_prefix("#EXTPERPETUA-JINGLE:") {
                    let parts: Vec<&str> = rest.splitn(3, '|').collect();
                    if parts.len() == 3 {
                        if let (Ok(slot), Ok(dur)) =
                            (parts[0].parse::<usize>(), parts[1].parse::<u64>())
                        {
                            loaded_jingles.push((slot, dur, parts[2].to_string()));
                        }
                    }
                } else if let Some(rest) = line.strip_prefix("#EXTPERPETUA-META:") {
                    let parts: Vec<&str> = rest.splitn(3, '|').collect();
                    if parts.len() == 3 {
                        let intro = parts[0].parse::<u64>().unwrap_or(0);
                        let sw_dur = parts[1].parse::<u64>().unwrap_or(0);
                        let sw_path = Some(parts[2].trim())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string());
                        pending_meta = Some((intro, sw_dur, sw_path));
                    }
                } else if let Some(rest) = line.strip_prefix("#EXTINF:") {
                    if let Some((sec_str, title_part)) = rest.split_once(',') {
                        if let Ok(secs) = sec_str.trim().parse::<f64>() {
                            if secs > 0.0 {
                                pending_duration_ms = Some((secs * 1000.0) as u64);
                            }
                        }
                        if !title_part.trim().is_empty() {
                            pending_title = Some(title_part.trim().to_string());
                        }
                    }
                } else if !line.starts_with('#') {
                    if let Some(mut item) = Self::classify_playlist_entry(
                        line,
                        pending_title.take(),
                        pending_duration_ms.take(),
                    ) {
                        if let Some((intro, sw_dur, sw_path)) = pending_meta.take() {
                            if intro > 0 {
                                item.intro_ms = intro;
                            }
                            item.sweeper_duration_ms = sw_dur;
                            item.sweeper_path = sw_path;
                        }
                        loaded_tracks.push(item);
                    }
                }
            }

            let mut state = self.playlist.lock().unwrap();
            state.playback_mode = mode;
            state.crossfade_ms = crossfade_ms;
            state.tracks.clear();
            state.current_index = None;
            state.auto_advance_armed = false;
            state.crossfade_triggered_for_current = false;

            for mut t in loaded_tracks {
                state.hydrate_track_item(&mut t);
                state.next_id += 1;
                t.id = state.next_id;
                state.tracks.push(t);
            }

            for (slot, dur, jpath) in loaded_jingles {
                if (1..=9).contains(&slot) {
                    let jtitle = Path::new(&jpath)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&jpath)
                        .to_string();
                    if let Some(jslot) = state.jingles.get_mut(slot - 1) {
                        jslot.path = Some(jpath);
                        jslot.title = Some(jtitle);
                        jslot.duration_ms = dur;
                    }
                }
            }
        }

        if trigger_save {
            self.trigger_autosave();
        }
        Ok(())
    }

    pub fn get_status(&self) -> PlaylistStatus {
        let _guard = self.transition_lock.lock().unwrap();
        let mut player_status = self.player.get_status();
        let now = Local::now();

        enum TransitionAction {
            None,
            Crossfade { path: String, overlap_ms: u64 },
            PlayNext { path: String },
        }

        let action = {
            let mut state = self.playlist.lock().unwrap();
            let remaining_ms = player_status
                .duration_ms
                .saturating_sub(player_status.elapsed_ms);

            // Si hay un segmento sin crossfade (ej. locución encadenada .time) sonando o en cola,
            // desactivamos el crossfade para que las palabras ("son las", "14", "y 15") no se pisen entre sí.
            let gapless_sequence_active = state
                .on_air_event
                .as_ref()
                .map(|e| e.no_crossfade)
                .unwrap_or(false)
                || state
                    .pending_events
                    .front()
                    .map(|e| e.no_crossfade)
                    .unwrap_or(false);

            let effective_xfade = if gapless_sequence_active {
                0
            } else {
                state.crossfade_ms.min(player_status.duration_ms / 2)
            };

            let has_pending_event = !state.pending_events.is_empty();

            if state.auto_advance_armed
                && !player_status.is_paused_by_event
                && (state.playback_mode != PlaybackMode::Manual || has_pending_event)
                && player_status.is_playing
                && player_status.duration_ms > 0
                && player_status.elapsed_ms > 0
                && effective_xfade > 0
                && remaining_ms <= effective_xfade
                && !state.crossfade_triggered_for_current
            {
                if let Some(source) = state.take_next_source(false) {
                    match state.commit_source(source, now) {
                        Ok((path, disable_xfade)) => {
                            if disable_xfade {
                                TransitionAction::PlayNext { path }
                            } else {
                                TransitionAction::Crossfade {
                                    path,
                                    overlap_ms: effective_xfade,
                                }
                            }
                        }
                        Err(err) => {
                            state.last_event_message = Some(format!("⚠ {}", err));
                            TransitionAction::None
                        }
                    }
                } else {
                    state.crossfade_triggered_for_current = true;
                    TransitionAction::None
                }
            } else if state.auto_advance_armed
                && !player_status.is_paused_by_event
                && !player_status.is_playing
                && player_status.duration_ms > 0
            {
                if has_pending_event {
                    let source = state.take_next_source(false).unwrap();
                    match state.commit_source(source, now) {
                        Ok((path, _)) => TransitionAction::PlayNext { path },
                        Err(err) => {
                            state.last_event_message = Some(format!("⚠ {}", err));
                            TransitionAction::None
                        }
                    }
                } else if state.playback_mode == PlaybackMode::Manual {
                    state.auto_advance_armed = false;
                    state.crossfade_triggered_for_current = false;
                    state.on_air_event = None;
                    if let Some(next_idx) = state.compute_next_index(false) {
                        state.current_index = Some(next_idx);
                    }
                    TransitionAction::None
                } else if let Some(source) = state.take_next_source(false) {
                    match state.commit_source(source, now) {
                        Ok((path, _)) => TransitionAction::PlayNext { path },
                        Err(err) => {
                            state.last_event_message = Some(format!("⚠ {}", err));
                            TransitionAction::None
                        }
                    }
                } else {
                    state.auto_advance_armed = false;
                    state.crossfade_triggered_for_current = false;
                    state.on_air_event = None;
                    TransitionAction::None
                }
            } else {
                TransitionAction::None
            }
        };

        match action {
            TransitionAction::Crossfade { path, overlap_ms } => {
                if let Ok(new_status) = self.player.play_with_crossfade(&path, overlap_ms) {
                    player_status = new_status;
                    let sweeper_spec = {
                        let mut state = self.playlist.lock().unwrap();
                        state.resolve_sweeper_for_active_playback(&path)
                    };
                    if let Ok(updated) = self.player.schedule_sweeper(sweeper_spec) {
                        player_status = updated;
                    }
                }
            }
            TransitionAction::PlayNext { path } => {
                if let Ok(new_status) = self.player.play(&path) {
                    player_status = new_status;
                    let sweeper_spec = {
                        let mut state = self.playlist.lock().unwrap();
                        state.resolve_sweeper_for_active_playback(&path)
                    };
                    if let Ok(updated) = self.player.schedule_sweeper(sweeper_spec) {
                        player_status = updated;
                    }
                }
            }
            TransitionAction::None => {}
        }

        let state = self.playlist.lock().unwrap();
        Self::build_status(player_status, &state)
    }

    fn build_status(player: PlayerStatus, state: &PlaylistState) -> PlaylistStatus {
        let remaining_ms = player.duration_ms.saturating_sub(player.elapsed_ms);
        let jingles = state
            .jingles
            .iter()
            .map(|j| {
                let mut slot_clone = j.clone();
                slot_clone.is_playing = player.active_jingle_slots.contains(&j.slot);
                slot_clone
            })
            .collect();

        let now = Local::now();
        let next_event = state.scheduler.next_event(now);
        let scheduled_events = state.scheduler.events().to_vec();
        let pending_events: Vec<EventItem> = state.pending_events.iter().cloned().collect();

        PlaylistStatus {
            player,
            remaining_ms,
            playback_mode: state.playback_mode,
            crossfade_ms: state.crossfade_ms,
            playlist: state.tracks.clone(),
            current_index: state.current_index,
            jingles,
            scheduled_events,
            next_event,
            pending_events,
            on_air_event: state.on_air_event.clone(),
            last_event_message: state.last_event_message.clone(),
            clock: now.format("%H:%M:%S").to_string(),
            recent_played_history: state.anti_repeat.recent_tracks(),
            horas_dir: state.horas_dir.to_string_lossy().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_playlist_operations_modes_jingles_and_persistence() {
        let core = MinimalCore::new_isolated();
        let wav_path = format!(
            "{}/../perpetua-audio/test_tone.wav",
            env!("CARGO_MANIFEST_DIR")
        );

        let status = core.add_tracks(vec![wav_path.clone(), wav_path.clone()]);
        assert_eq!(status.playlist.len(), 2);
        assert_eq!(status.jingles.len(), 9);

        let _ = core.set_playback_mode(PlaybackMode::Loop);
        let _ = core.set_crossfade(3200);
        let _ = core
            .assign_jingle(2, Some(&wav_path))
            .expect("debe asignar jingle #2");

        // 1) Probar guardado y carga en formato .m3u8
        let temp_m3u8 = std::env::temp_dir().join("perpetua_test_playlist.m3u8");
        core.save_playlist(temp_m3u8.to_str().unwrap())
            .expect("debe guardar .m3u8");

        let core_m3u8 = MinimalCore::new_isolated();
        let loaded_m3u8 = core_m3u8
            .load_playlist(temp_m3u8.to_str().unwrap())
            .expect("debe cargar .m3u8");
        assert_eq!(loaded_m3u8.playlist.len(), 2);
        assert_eq!(loaded_m3u8.playback_mode, PlaybackMode::Loop);
        assert_eq!(loaded_m3u8.crossfade_ms, 3200);
        assert_eq!(
            loaded_m3u8.jingles[1].title.as_deref(),
            Some("test_tone.wav")
        );
        let _ = fs::remove_file(&temp_m3u8);

        // 2) Probar guardado y carga en formato .json estructurado
        let temp_json = std::env::temp_dir().join("perpetua_test_playlist.json");
        core.save_playlist(temp_json.to_str().unwrap())
            .expect("debe guardar .json");

        let core_json = MinimalCore::new_isolated();
        let loaded_json = core_json
            .load_playlist(temp_json.to_str().unwrap())
            .expect("debe cargar .json");
        assert_eq!(loaded_json.playlist.len(), 2);
        assert_eq!(loaded_json.playback_mode, PlaybackMode::Loop);
        assert_eq!(loaded_json.crossfade_ms, 3200);
        assert_eq!(
            loaded_json.jingles[1].title.as_deref(),
            Some("test_tone.wav")
        );
        let _ = fs::remove_file(&temp_json);
    }

    #[test]
    fn test_event_scheduler_non_destructive_modes_and_wait_injection() {
        let core = MinimalCore::new_isolated();
        let wav_path = format!(
            "{}/../perpetua-audio/test_tone.wav",
            env!("CARGO_MANIFEST_DIR")
        );

        let _ = core.add_tracks(vec![wav_path.clone(), wav_path.clone()]);

        // 1. Añadir evento en modo Atenuación (Ducking -70%) a las 15:30 todos los días
        let st = core
            .add_event_with_mode(
                "15:30".to_string(),
                vec![0, 1, 2, 3, 4, 5, 6],
                wav_path.clone(),
                EventInterruptionMode::Ducking,
            )
            .expect("debe añadir evento Ducking");
        assert_eq!(st.scheduled_events.len(), 1);
        assert_eq!(
            st.scheduled_events[0].interruption_mode,
            EventInterruptionMode::Ducking
        );
        assert!(st.next_event.is_some());

        // Simular que la Pista #0 está en el Main Deck y detonar el evento a las 15:30:00
        let dt_1530 = Local
            .with_ymd_and_hms(2026, 10, 21, 15, 30, 0)
            .single()
            .unwrap();
        {
            let mut state = core.playlist.lock().unwrap();
            state.current_index = Some(0);
        }
        core.scheduler_tick(dt_1530);

        let st_after = core.get_status();
        // Al ser un evento no destructivo (Ducking), current_index se mantiene en Some(0) sin destruir la pista
        assert_eq!(st_after.current_index, Some(0));
        assert!(st_after
            .last_event_message
            .as_deref()
            .unwrap_or("")
            .contains("Ducking -70%"));

        // 2. Probar cola de prioridad con evento 'Espera al Final' (WaitForCurrent)
        core.stop();
        let _ = core
            .add_event_with_mode(
                "16:00".to_string(),
                vec![0, 1, 2, 3, 4, 5, 6],
                wav_path.clone(),
                EventInterruptionMode::WaitForCurrent,
            )
            .expect("debe añadir evento WaitForCurrent");
        {
            let mut state = core.playlist.lock().unwrap();
            state.current_index = Some(0);
            state.pending_events.push_back(EventItem {
                event_id: 2,
                title: "Mención Horaria".to_string(),
                path: wav_path.clone(),
                wait_for_current: true,
                no_crossfade: false,
            });

            // El siguiente elemento en sonar DEBE ser el evento en espera (sin avanzar current_index)
            let src1 = state.take_next_source(false).expect("debe tomar el evento");
            let _ = state.commit_source(src1, dt_1530).unwrap();
            assert!(state.on_air_event.is_some());
            assert_eq!(state.current_index, Some(0));

            // Al terminar el evento, la siguiente fuente DEBE retomar la lista en la Pista #1
            let src2 = state
                .take_next_source(false)
                .expect("debe retomar la lista en la pista 1");
            let _ = state.commit_source(src2, dt_1530).unwrap();
            assert!(state.on_air_event.is_none());
            assert_eq!(state.current_index, Some(1));
        }

        // 3. Desactivar y eliminar eventos
        let st_toggled = core.toggle_event(1, false).expect("debe desactivar evento");
        assert!(!st_toggled.scheduled_events[0].enabled);

        let st_removed = core.remove_event(1).expect("debe eliminar evento 1");
        assert_eq!(st_removed.scheduled_events.len(), 1);
        let _ = core.stop();
    }

    #[test]
    fn test_random_folder_and_time_announcement_in_playlist() {
        let core = MinimalCore::new_isolated();
        let temp_folder = std::env::temp_dir().join("perpetua_test_playlist_folder_hito7");
        let temp_horas = std::env::temp_dir().join("perpetua_test_playlist_horas_hito7");
        let _ = fs::remove_dir_all(&temp_folder);
        let _ = fs::remove_dir_all(&temp_horas);
        fs::create_dir_all(&temp_folder).unwrap();
        fs::create_dir_all(&temp_horas).unwrap();

        core.set_horas_dir(&temp_horas);

        // Crear 4 pistas de audio en la carpeta aleatoria
        let src_wav = format!(
            "{}/../perpetua-audio/test_tone.wav",
            env!("CARGO_MANIFEST_DIR")
        );
        for i in 1..=4 {
            fs::copy(&src_wav, temp_folder.join(format!("cancion_{}.wav", i))).unwrap();
        }

        // Crear archivos de hora para las 14:15 en temp_horas
        fs::copy(&src_wav, temp_horas.join("son_las.mp3")).unwrap();
        fs::copy(&src_wav, temp_horas.join("14.mp3")).unwrap();
        fs::copy(&src_wav, temp_horas.join("y_15.mp3")).unwrap();

        // Añadir una Carpeta Aleatoria y un Comando Virtual .time a la Playlist
        let st = core
            .add_random_folder(temp_folder.to_str().unwrap())
            .expect("debe añadir carpeta aleatoria");
        assert_eq!(st.playlist.len(), 1);
        assert_eq!(st.playlist[0].kind, TrackKind::RandomFolder);

        // Inspeccionar visualmente los contenidos de la carpeta con get_folder_contents (sin expandir el slot en la lista)
        let folder_entries = core
            .get_folder_contents(temp_folder.to_str().unwrap())
            .expect("debe listar los archivos de la carpeta sin decodificarlos");
        assert_eq!(folder_entries.len(), 4);
        assert_eq!(core.get_status().playlist.len(), 1);

        let st = core.add_time_announcement();
        assert_eq!(st.playlist.len(), 2);
        assert_eq!(st.playlist[1].kind, TrackKind::TimeAnnouncement);
        assert_eq!(st.playlist[1].path, ".time");

        // Simular activación de la Carpeta Aleatoria (índice 0) 4 veces seguidas:
        // Debe elegir las 4 canciones sin repetir ninguna y registrarlas en el historial de 50
        let dt_1415 = Local
            .with_ymd_and_hms(2026, 10, 24, 14, 15, 0)
            .single()
            .unwrap();
        {
            let mut state = core.playlist.lock().unwrap();
            let mut chosen_set = std::collections::HashSet::new();
            for _ in 0..4 {
                let (chosen_path, disable_xfade) = state
                    .activate_playlist_index(0, dt_1415)
                    .expect("debe elegir canción de carpeta aleatoria");
                assert!(!disable_xfade);
                chosen_set.insert(chosen_path);
            }
            assert_eq!(chosen_set.len(), 4);
            assert_eq!(state.anti_repeat.recent_history.len(), 4);

            // Ahora activar el ítem .time (índice 1) a las 14:15:
            // Debe devolver "horas/son_las.mp3" como activo y encolar "14.mp3" y "y_15.mp3" sin pausas
            let (first_clip, disable_xfade) = state
                .activate_playlist_index(1, dt_1415)
                .expect("debe resolver comando .time");
            assert!(disable_xfade);
            assert!(first_clip.ends_with("son_las.mp3"));
            assert_eq!(state.pending_events.len(), 2);
            assert!(state.pending_events[0].path.ends_with("14.mp3"));
            assert!(state.pending_events[0].no_crossfade);
            assert!(state.pending_events[1].path.ends_with("y_15.mp3"));
            assert!(state.pending_events[1].no_crossfade);
        }

        let _ = fs::remove_dir_all(&temp_folder);
        let _ = fs::remove_dir_all(&temp_horas);
    }

    #[test]
    fn test_sweeper_intro_metadata_waveform_and_trigger_formula() {
        let core = MinimalCore::new_isolated();
        let wav_path = format!(
            "{}/../perpetua-audio/test_tone.wav",
            env!("CARGO_MANIFEST_DIR")
        );

        // 1. Añadir pista a la lista
        let st = core.add_tracks(vec![wav_path.clone()]);
        assert_eq!(st.playlist.len(), 1);
        assert_eq!(st.playlist[0].intro_ms, 0);

        // 2. Configurar Intro = 12.5s (12500 ms) y asignar Pisador (test_tone.wav dura 5000 ms)
        // La fórmula (Tiempo de Intro - Duración del Pisador) debe dar 12500 - 5000 = 7500 ms
        let st = core
            .set_track_markers(Some(0), &wav_path, 12_500, Some(&wav_path))
            .expect("debe guardar Intro y Pisador en SQLite y actualizar Playlist");
        assert_eq!(st.playlist[0].intro_ms, 12_500);
        assert_eq!(st.playlist[0].sweeper_duration_ms, 5_000);
        assert_eq!(st.playlist[0].sweeper_trigger_ms, Some(7_500));

        // 3. Verificar extracción de forma de onda y marcadores para el Editor Visual
        let wf = core
            .get_track_waveform(&wav_path, Some(120))
            .expect("debe extraer forma de onda y marcadores");
        assert_eq!(wf.peaks.len(), 120);
        assert_eq!(wf.intro_ms, 12_500);
        assert_eq!(wf.sweeper_duration_ms, 5_000);
        assert_eq!(wf.sweeper_trigger_ms, Some(7_500));

        // 4. Si limpiamos y volvemos a añadir la pista, debe recuperar el Intro y Pisador desde SQLite
        let mut state = core.playlist.lock().unwrap();
        let spec = state
            .resolve_sweeper_for_active_playback(&wav_path)
            .expect("debe resolver especificación del pisador");
        assert_eq!(spec.intro_ms, 12_500);
        assert_eq!(spec.sweeper_duration_ms, 5_000);
        assert_eq!(spec.trigger_at_ms, 7_500);
    }
}
