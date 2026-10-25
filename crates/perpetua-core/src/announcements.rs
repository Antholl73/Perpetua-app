use chrono::{DateTime, Local, Timelike};
use std::f32::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};

/// Identificador especial del comando virtual de Locución de Hora en la lista de reproducción.
pub const TIME_COMMAND_TOKEN: &str = ".time";

/// Extensiones buscadas por orden de prioridad dentro de la carpeta `horas/`
/// (si el usuario coloca sus propios `.mp3`, `.flac` u `.ogg`, tienen prioridad sobre el `.wav` de respaldo).
const TIME_AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "wav"];

/// Segmento individual de una locución de hora encadenada (ej. `horas/son_las.mp3`, `horas/14.mp3`, `horas/y_15.mp3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeAnnouncementClip {
    pub label: String,
    pub path: String,
}

/// Busca el primer archivo existente en `dir` que coincida con alguno de los nombres base (`stems`)
/// y cualquiera de las extensiones soportadas (`.mp3`, `.flac`, `.ogg`, `.wav`).
fn find_existing_clip(dir: &Path, stems: &[String]) -> Option<PathBuf> {
    for stem in stems {
        for ext in TIME_AUDIO_EXTS {
            let candidate = dir.join(format!("{}.{}", stem, ext));
            if candidate.exists() && candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Genera un archivo `.wav` PCM de 16 bits mono (44.1 kHz) con una señal horaria limpia de radio
/// para garantizar que la carpeta `horas/` funcione de inmediato aunque el locutor aún no haya
/// grabado sus propios `.mp3`.
fn write_synthesized_wav(path: &Path, freqs: &[(f32, u32)]) -> std::io::Result<()> {
    let sample_rate: u32 = 44_100;
    let mut samples: Vec<i16> = Vec::new();

    for &(freq_hz, duration_ms) in freqs {
        let num_samples = ((sample_rate as u64 * duration_ms as u64) / 1000) as usize;
        let fade_samples = (sample_rate as usize / 100).min(num_samples / 4).max(1);
        for i in 0..num_samples {
            let t = i as f32 / sample_rate as f32;
            let env = if i < fade_samples {
                i as f32 / fade_samples as f32
            } else if i + fade_samples > num_samples {
                (num_samples - i) as f32 / fade_samples as f32
            } else {
                1.0
            };
            let val = if freq_hz <= 0.0 {
                0.0
            } else {
                let fundamental = (2.0 * PI * freq_hz * t).sin();
                let harmonic = 0.25 * (2.0 * PI * (freq_hz * 2.0) * t).sin();
                (fundamental + harmonic) * 0.38 * env
            };
            let pcm = (val.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            samples.push(pcm);
        }
    }

    let data_bytes = (samples.len() * 2) as u32;
    let file_size_minus_8 = 36 + data_bytes;
    let mut buf = Vec::with_capacity(44 + data_bytes as usize);

    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size_minus_8.to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (PCM)
    buf.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat = 1 (PCM)
    buf.extend_from_slice(&1u16.to_le_bytes()); // NumChannels = 1
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // ByteRate
    buf.extend_from_slice(&2u16.to_le_bytes()); // BlockAlign
    buf.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());

    for s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }

    fs::write(path, buf)
}

/// Asegura que la carpeta `horas/` exista y que los archivos requeridos para la hora `hour:minute`
/// estén disponibles (respetando cualquier `.mp3`/`.wav`/`.ogg`/`.flac` del usuario y creando el `.wav`
/// por defecto únicamente si falta alguno de los 3 segmentos).
pub fn ensure_time_clips_for_hm(horas_dir: &Path, hour: u32, minute: u32) -> Result<(), String> {
    fs::create_dir_all(horas_dir).map_err(|e| {
        format!(
            "No se pudo crear o abrir la carpeta de locuciones '{}': {}",
            horas_dir.display(),
            e
        )
    })?;

    // 1. Intro: son_las / es_la
    let intro_stems = if hour == 1 {
        vec!["es_la".to_string(), "son_las".to_string()]
    } else {
        vec!["son_las".to_string()]
    };
    if find_existing_clip(horas_dir, &intro_stems).is_none() {
        let stem = if hour == 1 { "es_la" } else { "son_las" };
        let target = horas_dir.join(format!("{}.wav", stem));
        write_synthesized_wav(&target, &[(880.0, 140), (0.0, 30), (1174.66, 180)])
            .map_err(|e| format!("Error al generar '{}': {}", target.display(), e))?;
    }

    // 2. Hora: {HH} (ej. 14.mp3 / 14.wav / 08.mp3 / HRS14.mp3)
    let hour_stems = vec![
        format!("{:02}", hour),
        format!("{}", hour),
        format!("HRS{:02}", hour),
    ];
    if find_existing_clip(horas_dir, &hour_stems).is_none() {
        let target = horas_dir.join(format!("{:02}.wav", hour));
        let base_freq = 440.0 + (hour as f32 * 18.0);
        write_synthesized_wav(&target, &[(base_freq, 220), (base_freq * 1.25, 220)])
            .map_err(|e| format!("Error al generar '{}': {}", target.display(), e))?;
    }

    // 3. Minuto: y_{MM} (ej. y_15.mp3 / y_15.wav / en_punto.mp3 / MIN15.mp3)
    let min_stems = if minute == 0 {
        vec![
            "en_punto".to_string(),
            "y_00".to_string(),
            "00".to_string(),
            "MIN00".to_string(),
        ]
    } else {
        vec![
            format!("y_{:02}", minute),
            format!("y_{}", minute),
            format!("{:02}", minute),
            format!("MIN{:02}", minute),
        ]
    };
    if find_existing_clip(horas_dir, &min_stems).is_none() {
        let default_name = if minute == 0 {
            "y_00.wav".to_string()
        } else {
            format!("y_{:02}.wav", minute)
        };
        let target = horas_dir.join(default_name);
        let min_freq = 523.25 + (minute as f32 * 8.0);
        write_synthesized_wav(&target, &[(min_freq, 200), (1046.5, 260)])
            .map_err(|e| format!("Error al generar '{}': {}", target.display(), e))?;
    }

    Ok(())
}

/// Resuelve la secuencia ordenada de archivos locales de `horas/` para pronunciar la hora `now`
/// sin pausas entre ellos:
/// 1) `horas/son_las.{mp3|wav|...}` (o `es_la` si es la 01)
/// 2) `horas/{HH}.{mp3|wav|...}` (ej. `horas/14.mp3`)
/// 3) `horas/y_{MM}.{mp3|wav|...}` (ej. `horas/y_15.mp3` o `horas/en_punto.mp3`)
pub fn resolve_time_announcement_sequence(
    horas_dir: &Path,
    now: DateTime<Local>,
) -> Result<Vec<TimeAnnouncementClip>, String> {
    let hour = now.hour();
    let minute = now.minute();

    ensure_time_clips_for_hm(horas_dir, hour, minute)?;

    let mut sequence = Vec::with_capacity(3);

    // 1. Prefijo ("son_las" / "es_la")
    let intro_stems = if hour == 1 {
        vec!["es_la".to_string(), "son_las".to_string()]
    } else {
        vec!["son_las".to_string()]
    };
    if let Some(intro_path) = find_existing_clip(horas_dir, &intro_stems) {
        sequence.push(TimeAnnouncementClip {
            label: format!(
                "🕒 Locución Hora ({:02}:{:02}) [1/3 · {}]",
                hour,
                minute,
                intro_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("son_las")
            ),
            path: intro_path.to_string_lossy().to_string(),
        });
    }

    // 2. Hora ("14.mp3" / "08.mp3" / "HRS14.mp3")
    let hour_stems = vec![
        format!("{:02}", hour),
        format!("{}", hour),
        format!("HRS{:02}", hour),
    ];
    let hour_path = find_existing_clip(horas_dir, &hour_stems).ok_or_else(|| {
        format!(
            "No se encontró el audio de la hora {:02} en '{}'",
            hour,
            horas_dir.display()
        )
    })?;
    sequence.push(TimeAnnouncementClip {
        label: format!(
            "🕒 Locución Hora ({:02}:{:02}) [2/3 · {}]",
            hour,
            minute,
            hour_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("hora")
        ),
        path: hour_path.to_string_lossy().to_string(),
    });

    // 3. Minuto ("y_15.mp3" / "en_punto.mp3" / "y_00.mp3")
    let min_stems = if minute == 0 {
        vec![
            "en_punto".to_string(),
            "y_00".to_string(),
            "00".to_string(),
            "MIN00".to_string(),
        ]
    } else {
        vec![
            format!("y_{:02}", minute),
            format!("y_{}", minute),
            format!("{:02}", minute),
            format!("MIN{:02}", minute),
        ]
    };
    let min_path = find_existing_clip(horas_dir, &min_stems).ok_or_else(|| {
        format!(
            "No se encontró el audio del minuto {:02} en '{}'",
            minute,
            horas_dir.display()
        )
    })?;
    sequence.push(TimeAnnouncementClip {
        label: format!(
            "🕒 Locución Hora ({:02}:{:02}) [3/3 · {}]",
            hour,
            minute,
            min_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("minuto")
        ),
        path: min_path.to_string_lossy().to_string(),
    });

    Ok(sequence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_resolve_time_sequence_prefers_mp3_and_falls_back_to_wav() {
        let temp_horas = std::env::temp_dir().join("perpetua_test_horas_hito7");
        let _ = fs::remove_dir_all(&temp_horas);
        fs::create_dir_all(&temp_horas).unwrap();

        // Colocar archivos .mp3 simulados para las 14:15 como en el ejemplo del usuario:
        // horas/son_las.mp3, horas/14.mp3, horas/y_15.mp3
        fs::write(temp_horas.join("son_las.mp3"), b"ID3-son_las").unwrap();
        fs::write(temp_horas.join("14.mp3"), b"ID3-14").unwrap();
        fs::write(temp_horas.join("y_15.mp3"), b"ID3-y_15").unwrap();

        let dt = Local.with_ymd_and_hms(2026, 10, 24, 14, 15, 0).single().unwrap();
        let clips = resolve_time_announcement_sequence(&temp_horas, dt)
            .expect("debe resolver los 3 clips de las 14:15");

        assert_eq!(clips.len(), 3);
        assert!(clips[0].path.ends_with("son_las.mp3"));
        assert!(clips[1].path.ends_with("14.mp3"));
        assert!(clips[2].path.ends_with("y_15.mp3"));

        // Probar también otra hora (ej. 09:42) donde no existan .mp3 previos: debe autogenerar los .wav
        let dt2 = Local.with_ymd_and_hms(2026, 10, 24, 9, 42, 0).single().unwrap();
        let clips2 = resolve_time_announcement_sequence(&temp_horas, dt2)
            .expect("debe resolver clips para 09:42");
        assert_eq!(clips2.len(), 3);
        assert!(clips2[0].path.ends_with("son_las.mp3"));
        assert!(clips2[1].path.ends_with("09.wav"));
        assert!(clips2[2].path.ends_with("y_42.wav"));

        let _ = fs::remove_dir_all(&temp_horas);
    }
}
