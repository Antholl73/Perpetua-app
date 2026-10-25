pub mod evt;
pub mod ini_formats;
pub mod lst;
pub mod sweeper_db;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("Error de E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cabecera o formato inválido: {0}")]
    InvalidFormat(String),
    #[error("Error de base de datos SQLite (.zsl / sweepers.db): {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Representa las 13+ variantes polimórficas de `File` descubiertas por REA en `file.cpp`
/// (`0x008c612c` .. `0x008f23fc`), más el sufijo de solapamiento `~N` (`0x004843d0`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TrackKind {
    /// `NormalFile` (MP3, OGG, WAV, WMA, FLAC, Opus, AAC, pistas CD `.cda`)
    AudioFile { path: String },
    /// `DirRandomFile` (`.dir`) — elige una pista aleatoria sin repetir usando `\random\`
    RandomDirectory { dir_path: String },
    /// `TimeFile` (`.time`) — locución horaria `Time\HRS%02d.mp3` + `MIN%02d.mp3`
    TimeAnnouncement,
    /// `TempFile` (`.temperature`) — locución de temperatura `Temperature\TMP%03d.mp3` / `TMPN%03d.mp3`
    TemperatureAnnouncement,
    /// `HumidityFile` (`.humidity`) — locución de humedad `Humidity\HUM%03d.mp3`
    HumidityAnnouncement,
    /// `PauseFile` (`%d.pause`) — pausa en segundos
    Pause { seconds: u32 },
    /// `SatFile` (`%d.satellite`) — conexión temporal a línea de satélite por `seconds`
    SatelliteConnection { seconds: u32 },
    /// `SatConFile` (`.satcon`) — desmutea línea de entrada de satélite indefinidamente
    SatelliteConnectCmd,
    /// `SatDisFile` (`.satdis`) — mutea línea de entrada de satélite
    SatelliteDisconnectCmd,
    /// `DtmfOnFile` (`.dtmfon`) — activa el detector DTMF por FFT
    DtmfEnableCmd,
    /// `DtmfOffFile` (`.dtmfoff`) — desactiva el detector DTMF
    DtmfDisableCmd,
    /// `StopFile` (`.stop`) — comando de parada de emisión en lista/evento
    StopCommand,
    /// `PlayFile` (`.play`) — comando de inicio de emisión en evento
    PlayCommand,
    /// `StreamFile` (`%s*%d.stream`) — stream de Internet reproducido durante `seconds`
    InternetStream { url: String, seconds: u32 },
    /// `RotationFile` (`.rot`) — rotación circular persistente en INI
    Rotation { path: String },
    /// `SequenceFile` (`.seq`) — bloque/secuencia expandible en INI
    Sequence { path: String },
}

/// Elemento individual dentro de una lista de reproducción (`.lst`, `.m3u`, `.pls`, `.seq`, `.rot`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackItem {
    pub id: String,
    pub duration_ms: i64,
    pub kind: TrackKind,
    /// Solapamiento personalizado en segundos extraído del sufijo `~%d` (`0x004843d0`).
    pub custom_overlap_sec: Option<u32>,
    /// Etiqueta legible para mostrar en la interfaz.
    pub display_title: String,
    /// Nivel integrado EBU R128 en LUFS (nueva funcionalidad moderna).
    pub loudness_lufs: Option<f32>,
}

impl TrackItem {
    /// Parsea la cadena cruda de recurso tal como la almacena ZaraRadio 1.6.2
    /// en `.lst`, `.evt`, `.rot` o `.seq` (incluyendo sufijo `~N`).
    pub fn from_raw_spec(duration_ms: i64, raw_spec: &str) -> Self {
        let (clean_path, custom_overlap_sec) = Self::split_overlap_suffix(raw_spec.trim());
        let kind = Self::classify_spec(clean_path);
        let display_title = Self::compute_display_title(&kind, clean_path);
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            duration_ms,
            kind,
            custom_overlap_sec,
            display_title,
            loudness_lufs: None,
        }
    }

    /// Reconstruye la cadena de ruta/recurso compatible con ZaraRadio 1.6.2 (incluyendo `~N`).
    pub fn to_raw_spec(&self) -> String {
        let base = match &self.kind {
            TrackKind::AudioFile { path } => path.clone(),
            TrackKind::RandomDirectory { dir_path } => {
                if dir_path.to_lowercase().ends_with(".dir") {
                    dir_path.clone()
                } else {
                    format!("{}.dir", dir_path)
                }
            }
            TrackKind::TimeAnnouncement => ".time".to_string(),
            TrackKind::TemperatureAnnouncement => ".temperature".to_string(),
            TrackKind::HumidityAnnouncement => ".humidity".to_string(),
            TrackKind::Pause { seconds } => format!("{}.pause", seconds),
            TrackKind::SatelliteConnection { seconds } => format!("{}.satellite", seconds),
            TrackKind::SatelliteConnectCmd => ".satcon".to_string(),
            TrackKind::SatelliteDisconnectCmd => ".satdis".to_string(),
            TrackKind::DtmfEnableCmd => ".dtmfon".to_string(),
            TrackKind::DtmfDisableCmd => ".dtmfoff".to_string(),
            TrackKind::StopCommand => ".stop".to_string(),
            TrackKind::PlayCommand => ".play".to_string(),
            TrackKind::InternetStream { url, seconds } => format!("{}*{}.stream", url, seconds),
            TrackKind::Rotation { path } => path.clone(),
            TrackKind::Sequence { path } => path.clone(),
        };
        if let Some(ov) = self.custom_overlap_sec {
            format!("{}~{}", base, ov)
        } else {
            base
        }
    }

    /// Reconstrucción de `File::SetOverlap` (`0x004843d0`): busca el último `'~'` (`0x7e`)
    /// y si el sufijo es un entero positivo, lo separa como tiempo de solapamiento en segundos.
    pub fn split_overlap_suffix(raw: &str) -> (&str, Option<u32>) {
        if let Some(idx) = raw.rfind('~') {
            if let Ok(sec) = raw[idx + 1..].parse::<u32>() {
                return (&raw[..idx], Some(sec));
            }
        }
        (raw, None)
    }

    pub fn classify_spec(spec: &str) -> TrackKind {
        let lower = spec.to_lowercase();
        match lower.as_str() {
            ".time" => TrackKind::TimeAnnouncement,
            ".temperature" => TrackKind::TemperatureAnnouncement,
            ".humidity" => TrackKind::HumidityAnnouncement,
            ".satcon" => TrackKind::SatelliteConnectCmd,
            ".satdis" => TrackKind::SatelliteDisconnectCmd,
            ".dtmfon" => TrackKind::DtmfEnableCmd,
            ".dtmfoff" => TrackKind::DtmfDisableCmd,
            ".stop" => TrackKind::StopCommand,
            ".play" => TrackKind::PlayCommand,
            _ if lower.ends_with(".pause") => {
                let num_part = &spec[..spec.len() - ".pause".len()];
                let seconds = num_part.parse::<u32>().unwrap_or(1);
                TrackKind::Pause { seconds }
            }
            _ if lower.ends_with(".satellite") => {
                let num_part = &spec[..spec.len() - ".satellite".len()];
                let seconds = num_part.parse::<u32>().unwrap_or(60);
                TrackKind::SatelliteConnection { seconds }
            }
            _ if lower.ends_with(".stream") => {
                let body = &spec[..spec.len() - ".stream".len()];
                if let Some(star) = body.rfind('*') {
                    let url = body[..star].to_string();
                    let seconds = body[star + 1..].parse::<u32>().unwrap_or(0);
                    TrackKind::InternetStream { url, seconds }
                } else {
                    TrackKind::InternetStream {
                        url: body.to_string(),
                        seconds: 0,
                    }
                }
            }
            _ if lower.ends_with(".dir") => {
                let dir_path = spec[..spec.len() - ".dir".len()].to_string();
                TrackKind::RandomDirectory { dir_path }
            }
            _ if lower.ends_with(".rot") => TrackKind::Rotation {
                path: spec.to_string(),
            },
            _ if lower.ends_with(".seq") => TrackKind::Sequence {
                path: spec.to_string(),
            },
            _ => TrackKind::AudioFile {
                path: spec.to_string(),
            },
        }
    }

    fn compute_display_title(kind: &TrackKind, raw: &str) -> String {
        match kind {
            TrackKind::TimeAnnouncement => "Locución de Hora (.time)".to_string(),
            TrackKind::TemperatureAnnouncement => "Locución de Temperatura (.temperature)".to_string(),
            TrackKind::HumidityAnnouncement => "Locución de Humedad (.humidity)".to_string(),
            TrackKind::Pause { seconds } => format!("Pausa ({} s)", seconds),
            TrackKind::SatelliteConnection { seconds } => {
                format!("Conexión Satélite ({} s)", seconds)
            }
            TrackKind::SatelliteConnectCmd => "Conectar Satélite (.satcon)".to_string(),
            TrackKind::SatelliteDisconnectCmd => "Desconectar Satélite (.satdis)".to_string(),
            TrackKind::DtmfEnableCmd => "Activar Detector DTMF (.dtmfon)".to_string(),
            TrackKind::DtmfDisableCmd => "Desactivar Detector DTMF (.dtmfoff)".to_string(),
            TrackKind::StopCommand => "Comando Detener Reproducción (.stop)".to_string(),
            TrackKind::PlayCommand => "Comando Iniciar Reproducción (.play)".to_string(),
            TrackKind::InternetStream { url, seconds } => {
                format!("Stream: {} ({} s)", url, seconds)
            }
            TrackKind::RandomDirectory { dir_path } => {
                let folder = dir_path
                    .rsplit(['\\', '/'])
                    .find(|s| !s.is_empty())
                    .unwrap_or(dir_path);
                format!("Aleatoria: {}", folder)
            }
            TrackKind::Rotation { path } | TrackKind::Sequence { path } | TrackKind::AudioFile { path } => {
                let filename = path.rsplit(['\\', '/']).next().unwrap_or(raw);
                filename
                    .rsplit_once('.')
                    .map(|(stem, _)| stem.to_string())
                    .unwrap_or_else(|| filename.to_string())
            }
        }
    }
}
