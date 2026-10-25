/// Procesador DSP de Control Automático de Ganancia (AGC / Compresor) y Detector de Silencio,
/// reconstruido a partir del callback `DSPProc` (`0x00483410`) y `Player::CheckSilence` (`0x0052e3a0`).
#[derive(Debug, Clone)]
pub struct BroadcastDspProcessor {
    pub agc_enabled: bool,
    pub current_gain: f32,
    pub target_rms: f32,
    pub max_gain: f32,
    pub min_gain: f32,
    pub attack_coeff: f32,
    pub release_coeff: f32,
    pub silence_detector_enabled: bool,
    pub silence_threshold_db: f32,
}

impl Default for BroadcastDspProcessor {
    fn default() -> Self {
        Self {
            agc_enabled: false,
            current_gain: 1.0,
            target_rms: 0.25, // ~ -12 dBFS
            max_gain: 3.16,   // +10 dB máx
            min_gain: 0.25,   // -12 dB mín
            attack_coeff: 0.05,
            release_coeff: 0.005,
            silence_detector_enabled: true,
            silence_threshold_db: -42.0,
        }
    }
}

impl BroadcastDspProcessor {
    /// Procesa un bloque entrelazado estéreo (`[L, R, L, R, ...]`) in-place, aplicando AGC
    /// y devolviendo `(left_db, right_db, gain_reduction_db)`.
    pub fn process_stereo_buffer(&mut self, samples: &mut [f32]) -> (f32, f32, f32) {
        if samples.is_empty() {
            return (-60.0, -60.0, 0.0);
        }

        let mut peak_l = 0.0_f32;
        let mut peak_r = 0.0_f32;
        let mut sum_sq = 0.0_f32;

        for frame in samples.chunks_exact(2) {
            let l = frame[0].abs();
            let r = frame[1].abs();
            sum_sq += 0.5 * (l * l + r * r);
        }

        let frames_count = (samples.len() / 2).max(1) as f32;
        let rms = (sum_sq / frames_count).sqrt();

        if self.agc_enabled && rms > 0.0005 {
            let desired_gain = (self.target_rms / rms).clamp(self.min_gain, self.max_gain);
            let coeff = if desired_gain < self.current_gain {
                self.attack_coeff
            } else {
                self.release_coeff
            };
            self.current_gain += coeff * (desired_gain - self.current_gain);
        } else if !self.agc_enabled {
            self.current_gain = 1.0;
        }

        for frame in samples.chunks_exact_mut(2) {
            // Limitador suave tanh en el techo de 0 dBFS para evitar clipping digital
            let out_l = (frame[0] * self.current_gain).clamp(-1.0, 1.0);
            let out_r = (frame[1] * self.current_gain).clamp(-1.0, 1.0);
            frame[0] = out_l;
            frame[1] = out_r;
            peak_l = peak_l.max(out_l.abs());
            peak_r = peak_r.max(out_r.abs());
        }

        let left_db = linear_to_db(peak_l);
        let right_db = linear_to_db(peak_r);
        let gain_reduction_db = if self.current_gain < 1.0 {
            linear_to_db(self.current_gain)
        } else {
            0.0
        };

        (left_db, right_db, gain_reduction_db)
    }

    /// Reconstrucción de `Player::DetectarSilencio` (`0x0052e3a0..0x0052e550`):
    /// Cuando la pista está en la ventana final de cola (p. ej. últimos 15 segundos) y el nivel
    /// cae por debajo de `silence_threshold_db`, dispara `"Silence detected"` y adelanta el cruce.
    pub fn should_skip_tail_silence(
        &self,
        remaining_ms: i64,
        tail_window_ms: i64,
        left_db: f32,
        right_db: f32,
    ) -> bool {
        if !self.silence_detector_enabled || remaining_ms <= 0 || remaining_ms > tail_window_ms {
            return false;
        }
        let max_ch_db = left_db.max(right_db);
        max_ch_db < self.silence_threshold_db
    }
}

pub fn linear_to_db(linear: f32) -> f32 {
    if linear <= 0.001 {
        -60.0
    } else {
        (20.0 * linear.log10()).clamp(-60.0, 6.0)
    }
}
