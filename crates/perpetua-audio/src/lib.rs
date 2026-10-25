pub mod dsp;
pub mod dtmf;
pub mod fader;
pub mod player;

pub use player::{
    compute_sweeper_trigger_ms, decode_file_with_symphonia, extract_waveform_peaks,
    list_output_devices, probe_duration_ms, probe_track_metadata, soft_limit_sample, AudioPlayer,
    CueTelemetry, EventOverlayMode, PlayerStatus, SpscRingBuffer, SweeperScheduleSpec,
    TrackProbeInfo, WaveformPeaks,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioRoutingConfig {
    pub air_device: String,
    pub cue_device: String,
    pub sweeper_device: String,
    pub jingles_device: String,
    pub satellite_input_device: String,
    pub aux_devices: [String; 4],
}

impl Default for AudioRoutingConfig {
    fn default() -> Self {
        Self {
            air_device: "Default Output".to_string(),
            cue_device: "Default Output".to_string(),
            sweeper_device: "Default Output".to_string(),
            jingles_device: "Default Output".to_string(),
            satellite_input_device: "Default Input".to_string(),
            aux_devices: std::array::from_fn(|_| "Default Output".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaderSettings {
    pub fade_length_ms: u32,
    pub fade_overlap_ms: u32,
    pub sweeper_duck_volume_pct: u8,
    pub silence_detector_enabled: bool,
    pub silence_threshold_db: f32,
    pub agc_compressor_enabled: bool,
    pub target_lufs: f32,
    pub curve: fader::FadeCurve,
}

impl Default for FaderSettings {
    fn default() -> Self {
        Self {
            fade_length_ms: 4000,
            fade_overlap_ms: 2500,
            sweeper_duck_volume_pct: 25,
            silence_detector_enabled: true,
            silence_threshold_db: -42.0,
            agc_compressor_enabled: false,
            target_lufs: -16.0,
            curve: fader::FadeCurve::EqualPower,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VuMeterTelemetry {
    pub left_db: f32,
    pub right_db: f32,
    pub peak_left_db: f32,
    pub peak_right_db: f32,
    pub agc_gain_reduction_db: f32,
    pub integrated_lufs: f32,
}

impl Default for VuMeterTelemetry {
    fn default() -> Self {
        Self {
            left_db: -60.0,
            right_db: -60.0,
            peak_left_db: -60.0,
            peak_right_db: -60.0,
            agc_gain_reduction_db: 0.0,
            integrated_lufs: -16.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    /// Prueba de vida de Audio en Rust:
    /// Carga `test_tone.wav` local con `symphonia` en modo Streaming JIT y lo reproduce durante 3 segundos
    /// por la salida de audio por defecto usando `cpal`.
    #[test]
    fn test_play_local_audio_3_seconds() {
        let wav_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_tone.wav");
        assert!(
            wav_path.exists(),
            "El archivo de prueba {:?} debe existir",
            wav_path
        );

        let player = AudioPlayer::new();
        let initial_status = player
            .play(&wav_path)
            .expect("Debe iniciar la reproducción por la salida por defecto con cpal/symphonia");

        println!(
            "[perpetua-audio] Reproduciendo '{}' en dispositivo '{}' ({} Hz, {} canales, {} frames iniciales en ringbuf)...",
            initial_status.current_file.as_deref().unwrap_or(""),
            initial_status.device_name,
            initial_status.sample_rate,
            initial_status.channels,
            initial_status.decoded_frames
        );
        assert!(initial_status.is_playing);
        assert!(initial_status.decoded_frames > 0);

        // Reproducir durante 3 segundos reales
        std::thread::sleep(Duration::from_secs(3));

        let mid_status = player.get_status();
        println!(
            "[perpetua-audio] Estado tras 3s -> is_playing: {}, elapsed_ms: {} ms / {} ms",
            mid_status.is_playing, mid_status.elapsed_ms, mid_status.duration_ms
        );
        assert!(
            mid_status.elapsed_ms >= 2500,
            "Se esperaban al menos 2500 ms reproducidos, pero elapsed_ms = {}",
            mid_status.elapsed_ms
        );

        let stopped_status = player.stop();
        assert!(!stopped_status.is_playing);
        println!("[perpetua-audio] Reproducción detenida limpiamente.");
    }
}
