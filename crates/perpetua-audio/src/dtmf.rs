use rustfft::{num_complex::Complex, FftPlanner};

/// Matriz DTMF 4x4 exacta inicializada en la pila de `DtmfTimerProc` (`0x004704f2..0x00470575`):
/// Filas: `[697, 770, 852, 941]` Hz | Columnas: `[1209, 1336, 1477, 1633]` Hz
pub const DTMF_MATRIX: [[char; 4]; 4] = [
    ['1', '2', '3', 'A'],
    ['4', '5', '6', 'B'],
    ['7', '8', '9', 'C'],
    ['*', '0', '#', 'D'],
];

/// Reconstrucción del decodificador DTMF por FFT de 2048 muestras (`utils.cpp`, `0x00470200..0x00470775`).
/// Incluye el filtro anti-rebote (*debounce*) de 2 lecturas consecutivas idénticas (`0x87f011` / `0x87f010`).
pub struct DtmfDetector {
    pub enabled: bool,
    pub threshold_factor: f32,
    prev_sample_tone: char,
    emitted_tone: char,
}

impl Default for DtmfDetector {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_factor: 4.5,
            prev_sample_tone: ' ',
            emitted_tone: ' ',
        }
    }
}

impl DtmfDetector {
    /// Analiza un bloque de 1024 magnitudes de FFT (equivalente a `BASS_DATA_FFT2048` a 44100 Hz)
    /// siguiendo exactamente el algoritmo de `0x00470295..0x004706e8`.
    pub fn process_fft1024_magnitudes(&mut self, fft: &[f32; 1024]) -> Option<char> {
        if !self.enabled {
            return None;
        }

        let mut sum_band: f64 = 0.0;
        let mut max_val: f64 = 0.0;
        let mut max_bin: usize = 0;

        for (i, &val) in fft.iter().enumerate() {
            let v = val as f64;
            if (32..=76).contains(&i) {
                sum_band += v;
            }
            if v > max_val {
                max_val = v;
                max_bin = i;
            }
        }

        // Ventanas exactas de bins en 0x004702f9..0x004703a8 (2048 FFT @ 44100 Hz = 21.533 Hz/bin)
        let in_dtmf_bin = matches!(
            max_bin,
            0x20..=0x21
                | 0x23..=0x24
                | 0x27..=0x28
                | 0x2b..=0x2d
                | 0x37..=0x39
                | 0x3d..=0x3f
                | 0x44..=0x45
                | 0x4b..=0x4d
        );

        let rows = [fft[32], fft[36], fft[40], fft[44]];
        let cols = [fft[56], fft[62], fft[69], fft[76]];

        for &r in &rows {
            sum_band -= r as f64;
        }
        for &c in &cols {
            sum_band -= c as f64;
        }

        // 0x0047045c: fdivs 37.0 (45 bins entre 32..76 menos los 8 picos = 37 bins de piso de ruido)
        let threshold = (sum_band / 37.0) * (self.threshold_factor as f64);

        let mut row_idx: i32 = -1;
        for (r, &val) in rows.iter().enumerate() {
            if (val as f64) > threshold {
                row_idx = if row_idx == -1 { r as i32 } else { -2 };
            }
        }

        let mut col_idx: i32 = -1;
        for (c, &val) in cols.iter().enumerate() {
            if (val as f64) > threshold {
                col_idx = if col_idx == -1 { c as i32 } else { -2 };
            }
        }

        if row_idx >= 0 && col_idx >= 0 && in_dtmf_bin {
            let tone = DTMF_MATRIX[row_idx as usize][col_idx as usize];
            if tone == self.prev_sample_tone && tone != self.emitted_tone {
                self.emitted_tone = tone;
                self.prev_sample_tone = tone;
                return Some(tone);
            }
            self.prev_sample_tone = tone;
            None
        } else {
            self.prev_sample_tone = ' ';
            self.emitted_tone = ' ';
            None
        }
    }

    /// Calcula la FFT de 2048 muestras PCM mono en punto flotante y ejecuta el detector DTMF.
    pub fn process_pcm_window(&mut self, pcm_2048: &[f32; 2048]) -> Option<char> {
        if !self.enabled {
            return None;
        }
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(2048);
        let mut buffer: Vec<Complex<f32>> = pcm_2048
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                // Ventana Hann
                let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / 2047.0).cos());
                Complex {
                    re: s * w,
                    im: 0.0,
                }
            })
            .collect();
        fft.process(&mut buffer);

        let mut mags = [0.0_f32; 1024];
        for i in 0..1024 {
            mags[i] = buffer[i].norm() / 1024.0;
        }
        self.process_fft1024_magnitudes(&mags)
    }
}
