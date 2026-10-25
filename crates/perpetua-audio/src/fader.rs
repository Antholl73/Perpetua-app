use serde::{Deserialize, Serialize};

/// Tipo de curva de fundido: incluye la atenuación lineal clásica de `Fader::Notify` (`0x00483c8d`)
/// y las nuevas curvas modernas de potencia constante y logarítmica.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeCurve {
    /// Ecuación exacta de `ZaraRadio.exe` (`0x00483c96..0x00483ca4`):
    /// `vol = initial_vol - (initial_vol * elapsed_ms) / target_duration_ms`
    LinearClassic,
    /// Potencia constante (coseno / seno) sin caída de energía en el centro del cruce
    EqualPower,
    /// Atenuación cuadrática / logarítmica suave
    Logarithmic,
}

/// Calcula la ganancia instantánea `(ganancia_saliente, ganancia_entrante)` para un progreso
/// normalizado `t` en `[0.0, 1.0]` libre de discontinuidades o chasquidos.
#[inline]
pub fn compute_crossfade_gains(progress: f32, curve: FadeCurve) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
    match curve {
        FadeCurve::LinearClassic => (1.0 - t, t),
        FadeCurve::EqualPower => {
            let angle = t * std::f32::consts::FRAC_PI_2;
            (angle.cos(), angle.sin())
        }
        FadeCurve::Logarithmic => {
            let out_gain = (1.0 - t).powi(2);
            let in_gain = 1.0 - (1.0 - t).powi(2);
            (out_gain, in_gain)
        }
    }
}

/// Reconstrucción de la clase `Fader` (`fader.cpp`, `0x00483810..0x00483ce0`)
/// mejorada con resolución arbitraria de tiempo (en vez de saltos fijos de 100 ms).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaderState {
    pub active: bool,
    /// Equivalente a `this->waitBeforeStop` (`+0x3d` en `Fader`):
    /// `true` = mantiene el volumen durante `target_duration_ms` y corta de golpe al final;
    /// `false` = atenúa progresivamente desde `initial_volume` hasta `0.0`.
    pub wait_before_hard_cut: bool,
    pub elapsed_ms: u32,
    pub target_duration_ms: u32,
    pub initial_volume: f32,
    pub current_volume: f32,
    pub curve: FadeCurve,
}

impl Default for FaderState {
    fn default() -> Self {
        Self {
            active: false,
            wait_before_hard_cut: false,
            elapsed_ms: 0,
            target_duration_ms: 4000,
            initial_volume: 1.0,
            current_volume: 1.0,
            curve: FadeCurve::EqualPower,
        }
    }
}

impl FaderState {
    /// Equivalente a `Fader::StartFade` (`0x00483810`).
    pub fn start(
        &mut self,
        initial_volume: f32,
        target_duration_ms: u32,
        wait_before_hard_cut: bool,
        curve: FadeCurve,
    ) {
        self.active = true;
        self.wait_before_hard_cut = wait_before_hard_cut;
        self.elapsed_ms = 0;
        self.target_duration_ms = target_duration_ms.max(1);
        self.initial_volume = initial_volume.clamp(0.0, 1.0);
        self.current_volume = self.initial_volume;
        self.curve = curve;
    }

    /// Avanza el fundido `delta_ms` milisegundos (en ZaraRadio original `delta_ms = 100` en `0x00483c20`).
    /// Devuelve `true` cuando el fundido ha concluido y el stream debe cerrarse (`FinishAndDestroyFile`).
    pub fn tick(&mut self, delta_ms: u32) -> bool {
        if !self.active {
            return false;
        }
        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);

        if self.wait_before_hard_cut {
            if self.elapsed_ms > self.target_duration_ms {
                self.current_volume = 0.0;
                self.active = false;
                return true;
            }
            return false;
        }

        if self.elapsed_ms >= self.target_duration_ms {
            self.current_volume = 0.0;
            self.active = false;
            return true;
        }

        let t = (self.elapsed_ms as f32 / self.target_duration_ms as f32).clamp(0.0, 1.0);
        let (out_gain, _) = compute_crossfade_gains(t, self.curve);
        self.current_volume = self.initial_volume * out_gain;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equal_power_crossfade_gains_continuity() {
        let (out_start, in_start) = compute_crossfade_gains(0.0, FadeCurve::EqualPower);
        assert!((out_start - 1.0).abs() < 1e-6);
        assert!(in_start.abs() < 1e-6);

        let (out_mid, in_mid) = compute_crossfade_gains(0.5, FadeCurve::EqualPower);
        // En potencia constante, cos^2(pi/4) + sin^2(pi/4) == 1.0 (~0.7071 cada uno)
        assert!((out_mid - in_mid).abs() < 1e-5);
        assert!(((out_mid * out_mid + in_mid * in_mid) - 1.0).abs() < 1e-5);

        let (out_end, in_end) = compute_crossfade_gains(1.0, FadeCurve::EqualPower);
        assert!(out_end.abs() < 1e-6);
        assert!((in_end - 1.0).abs() < 1e-6);
    }
}
