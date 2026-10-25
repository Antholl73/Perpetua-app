//! Planificador de eventos (Event Scheduler).
//!
//! Lógica pura y determinista (sin hilos ni audio) para poder probarla con relojes simulados.
//! El hilo de reloj que la evalúa cada segundo vive en `MinimalCore` (`lib.rs`).

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Modo de interrupción no destructiva cuando se detona un evento programado.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventInterruptionMode {
    /// Atenuación (Ducking / Talkover): Baja suavemente el volumen del Main Deck un 70% (ganancia 0.30),
    /// reproduce el evento en un canal concurrente y restaura el volumen al 100% al terminar.
    #[default]
    Ducking,
    /// Pausa y Reanudación: Pausa el Main Deck en el frame exacto, reproduce el evento en el canal
    /// superpuesto y reanuda la canción exactamente donde se quedó.
    PauseResume,
    /// Espera Máxima: Espera a que termine la pista actual de la lista antes de reproducir el evento.
    WaitForCurrent,
}

impl EventInterruptionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ducking => "ducking",
            Self::PauseResume => "pause_resume",
            Self::WaitForCurrent => "wait_for_current",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "pause_resume" | "pause_and_resume" | "pause" => Self::PauseResume,
            "wait_for_current" | "wait" => Self::WaitForCurrent,
            _ => Self::Ducking,
        }
    }
}

/// Evento programado.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledEvent {
    pub id: u64,
    /// Hora de ejecución en formato `HH:MM` (24 h).
    pub time: String,
    /// Días de la semana: `0` = lunes … `6` = domingo.
    pub days: Vec<u8>,
    /// Archivo de audio, carpeta, comando `.time` o lista `.m3u` / `.m3u8` a reproducir.
    pub path: String,
    pub title: String,
    /// `true` = esperar a que termine la pista actual (se encola como siguiente).
    /// `false` = reproducción inmediata no destructiva según `interruption_mode`.
    pub wait_for_current: bool,
    /// Modo de interrupción no destructiva (`ducking`, `pause_resume` o `wait_for_current`).
    #[serde(default)]
    pub interruption_mode: EventInterruptionMode,
    pub enabled: bool,
}

/// Información del próximo evento para el indicador de cabina.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NextEventInfo {
    pub event_id: u64,
    pub title: String,
    pub time: String,
    pub fires_at_epoch: i64,
    /// `HH:MM:SS` (o `ddd HH:MM:SS` si no es hoy).
    pub fires_at_display: String,
    pub seconds_until: i64,
    pub wait_for_current: bool,
    #[serde(default)]
    pub interruption_mode: EventInterruptionMode,
}

/// Convierte `HH:MM` en `(hora, minuto)` validados.
pub fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

/// Ordena, deduplica y descarta días fuera de rango.
pub fn normalize_days(days: &[u8]) -> Vec<u8> {
    let mut d: Vec<u8> = days.iter().copied().filter(|d| *d < 7).collect();
    d.sort_unstable();
    d.dedup();
    d
}

#[derive(Debug, Default)]
pub struct EventScheduler {
    events: Vec<ScheduledEvent>,
    next_id: u64,
    /// Última clave `AAAA-MM-DD HH:MM` en la que se disparó cada evento,
    /// para que cada evento se detone una sola vez por minuto aunque el tick se repita.
    last_fired: HashMap<u64, String>,
}

impl EventScheduler {
    pub fn from_events(events: Vec<ScheduledEvent>) -> Self {
        let next_id = events.iter().map(|e| e.id).max().unwrap_or(0);
        Self {
            events,
            next_id,
            last_fired: HashMap::new(),
        }
    }

    pub fn events(&self) -> &[ScheduledEvent] {
        &self.events
    }

    pub fn get(&self, id: u64) -> Option<&ScheduledEvent> {
        self.events.iter().find(|e| e.id == id)
    }

    pub fn add(
        &mut self,
        time: &str,
        days: &[u8],
        path: &str,
        title: &str,
        wait_for_current: bool,
    ) -> Result<ScheduledEvent, String> {
        let mode = if wait_for_current {
            EventInterruptionMode::WaitForCurrent
        } else {
            EventInterruptionMode::Ducking
        };
        self.add_with_mode(time, days, path, title, mode)
    }

    pub fn add_with_mode(
        &mut self,
        time: &str,
        days: &[u8],
        path: &str,
        title: &str,
        interruption_mode: EventInterruptionMode,
    ) -> Result<ScheduledEvent, String> {
        let (h, m) = parse_hhmm(time)
            .ok_or_else(|| format!("Hora inválida '{}': usa el formato HH:MM", time))?;
        let days = normalize_days(days);
        if days.is_empty() {
            return Err("Selecciona al menos un día de la semana para el evento.".to_string());
        }
        if path.trim().is_empty() {
            return Err("El evento necesita un archivo o lista a reproducir.".to_string());
        }
        self.next_id += 1;
        let wait_for_current = interruption_mode == EventInterruptionMode::WaitForCurrent;
        let ev = ScheduledEvent {
            id: self.next_id,
            time: format!("{:02}:{:02}", h, m),
            days,
            path: path.trim().to_string(),
            title: title.to_string(),
            wait_for_current,
            interruption_mode,
            enabled: true,
        };
        self.events.push(ev.clone());
        self.events
            .sort_by(|a, b| a.time.cmp(&b.time).then(a.id.cmp(&b.id)));
        Ok(ev)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.events.len();
        self.events.retain(|e| e.id != id);
        self.last_fired.remove(&id);
        self.events.len() != before
    }

    pub fn set_enabled(&mut self, id: u64, enabled: bool) -> bool {
        match self.events.iter_mut().find(|e| e.id == id) {
            Some(ev) => {
                ev.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// Devuelve los eventos que deben detonarse en el minuto actual (una sola vez por minuto y día).
    pub fn due_events(&mut self, now: DateTime<Local>) -> Vec<ScheduledEvent> {
        let key = now.format("%Y-%m-%d %H:%M").to_string();
        let weekday = now.weekday().num_days_from_monday() as u8;
        let (hour, minute) = (now.hour(), now.minute());

        let mut due = Vec::new();
        for ev in self.events.iter().filter(|e| e.enabled) {
            if !ev.days.contains(&weekday) || parse_hhmm(&ev.time) != Some((hour, minute)) {
                continue;
            }
            if self.last_fired.get(&ev.id) == Some(&key) {
                continue;
            }
            self.last_fired.insert(ev.id, key.clone());
            due.push(ev.clone());
        }
        due
    }

    /// Calcula el próximo evento estrictamente posterior a `now` (busca hasta 7 días adelante).
    pub fn next_event(&self, now: DateTime<Local>) -> Option<NextEventInfo> {
        let today = now.date_naive();
        let mut best: Option<(DateTime<Local>, &ScheduledEvent)> = None;

        for ev in self.events.iter().filter(|e| e.enabled) {
            let Some((h, m)) = parse_hhmm(&ev.time) else {
                continue;
            };
            for offset in 0..=7i64 {
                let date = today + chrono::Duration::days(offset);
                let wd = date.weekday().num_days_from_monday() as u8;
                if !ev.days.contains(&wd) {
                    continue;
                }
                let Some(naive) = date.and_hms_opt(h, m, 0) else {
                    continue;
                };
                // `single()` descarta horas inexistentes/ambiguas por cambio de horario.
                let Some(candidate) = Local.from_local_datetime(&naive).single() else {
                    continue;
                };
                if candidate <= now {
                    continue;
                }
                if best.map(|(t, _)| candidate < t).unwrap_or(true) {
                    best = Some((candidate, ev));
                }
                break;
            }
        }

        best.map(|(when, ev)| {
            let fires_at_display = if when.date_naive() == today {
                when.format("%H:%M:%S").to_string()
            } else {
                const DAYS: [&str; 7] = ["Lun", "Mar", "Mié", "Jue", "Vie", "Sáb", "Dom"];
                let wd = when.weekday().num_days_from_monday() as usize;
                format!("{} {}", DAYS[wd], when.format("%H:%M:%S"))
            };
            NextEventInfo {
                event_id: ev.id,
                title: ev.title.clone(),
                time: ev.time.clone(),
                fires_at_epoch: when.timestamp(),
                fires_at_display,
                seconds_until: (when - now).num_seconds(),
                wait_for_current: ev.wait_for_current,
                interruption_mode: ev.interruption_mode,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    #[test]
    fn test_parse_hhmm() {
        assert_eq!(parse_hhmm("15:30"), Some((15, 30)));
        assert_eq!(parse_hhmm("7:05"), Some((7, 5)));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("abc"), None);
    }

    #[test]
    fn test_due_event_fires_once_per_minute_and_respects_days() {
        // 2026-10-19 es lunes (día 0)
        let mut s = EventScheduler::default();
        let ev = s.add("15:30", &[0, 2], "/tmp/a.wav", "a.wav", true).unwrap();

        assert!(s.due_events(at(2026, 10, 19, 15, 29, 59)).is_empty());
        let fired = s.due_events(at(2026, 10, 19, 15, 30, 0));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].id, ev.id);
        // Ticks repetidos dentro del mismo minuto no vuelven a disparar
        assert!(s.due_events(at(2026, 10, 19, 15, 30, 1)).is_empty());
        assert!(s.due_events(at(2026, 10, 19, 15, 30, 59)).is_empty());
        // Martes (día 1) no está programado
        assert!(s.due_events(at(2026, 10, 20, 15, 30, 0)).is_empty());
        // Miércoles (día 2) sí
        assert_eq!(s.due_events(at(2026, 10, 21, 15, 30, 0)).len(), 1);
    }

    #[test]
    fn test_disabled_event_does_not_fire() {
        let mut s = EventScheduler::default();
        let ev = s.add("10:00", &[0], "/tmp/a.wav", "a", false).unwrap();
        s.set_enabled(ev.id, false);
        assert!(s.due_events(at(2026, 10, 19, 10, 0, 0)).is_empty());
        assert!(s.next_event(at(2026, 10, 19, 9, 0, 0)).is_none());
    }

    #[test]
    fn test_next_event_today_and_next_week() {
        let mut s = EventScheduler::default();
        s.add("15:30", &[0], "/tmp/a.wav", "lunes", true).unwrap();
        s.add("18:00", &[0], "/tmp/b.wav", "tarde", false).unwrap();

        let next = s.next_event(at(2026, 10, 19, 15, 0, 0)).unwrap();
        assert_eq!(next.title, "lunes");
        assert_eq!(next.seconds_until, 30 * 60);
        assert_eq!(next.fires_at_display, "15:30:00");

        let next = s.next_event(at(2026, 10, 19, 16, 0, 0)).unwrap();
        assert_eq!(next.title, "tarde");

        // Pasadas las 18:00 del lunes, el próximo es el lunes siguiente a las 15:30
        let next = s.next_event(at(2026, 10, 19, 19, 0, 0)).unwrap();
        assert_eq!(next.title, "lunes");
        assert!(next.fires_at_display.starts_with("Lun "));
    }

    #[test]
    fn test_add_validates_input() {
        let mut s = EventScheduler::default();
        assert!(s.add("99:00", &[0], "/tmp/a.wav", "a", true).is_err());
        assert!(s.add("10:00", &[], "/tmp/a.wav", "a", true).is_err());
        assert!(s.add("10:00", &[0], "  ", "a", true).is_err());
    }
}
