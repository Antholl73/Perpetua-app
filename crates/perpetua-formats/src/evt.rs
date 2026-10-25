use crate::{FormatError, TrackItem};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

/// Versión del formato binario `.evt` verificada en `GestionEventos::Load` (`0x00481f00..0x00481f27`)
/// y `Evento::Load` (`0x0047d6ef..0x0047d705`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvtVersion {
    /// `"EVT01"` — `enabled` por defecto `true`, `max_wait` por defecto `-1`
    V1,
    /// `"EVT02"` — incluye byte `enabled` (`+0x50`), `max_wait` por defecto `-1`
    V2,
    /// `"EVT03"` — incluye byte `enabled` (`+0x50`) e `int32` `max_wait` (`+0x54`)
    V3,
}

impl EvtVersion {
    pub fn magic_bytes(self) -> &'static [u8; 5] {
        match self {
            EvtVersion::V1 => b"EVT01",
            EvtVersion::V2 => b"EVT02",
            EvtVersion::V3 => b"EVT03",
        }
    }
}

/// Prioridad del evento (`+0x44` en `Evento`, `0x0047d23c`): `0` = Alta, `1` = Baja.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventPriority {
    High = 0,
    Low = 1,
}

/// Estructura de un evento programado reconstruida byte a byte desde:
/// - `Evento::Save` (`0x0047d040..0x0047d2c4`)
/// - `Evento::Load` (`0x0047d380..0x0047d750`)
/// - `GestionEventos::SaveRegistry` / `LoadRegistry` (`0x0047e390..0x0047f800`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledEvent {
    pub id: String,
    /// Offset `+0x00`: `true` = Inmediato (`1`), `false` = Retardado (`0`)
    pub immediate: bool,
    /// Offset `+0x04`: `dayHour` — Segundo del día (`0..86399`) si `hour_bits == 0` ("Cada día"),
    /// o segundo dentro de la hora (`0..3599`) si `hour_bits != 0` ("Todas las horas").
    pub day_hour_sec: i32,
    /// Offset `+0x08..+0x0E`: `dayMask[7]` — 7 booleanos (Dom, Lun, Mar, Mié, Jue, Vie, Sáb)
    pub day_mask: [bool; 7],
    /// Offset `+0x1C`: `fichero` — Recurso o pista asociada al evento
    pub track: TrackItem,
    /// Offset `+0x30`: `startDate` en formato `"%m/%d/%y %H:%M:%S"` (`0x008889d0`)
    pub start_date: String,
    /// Offset `+0x38`: `expirationDate` en formato `"%m/%d/%y %H:%M:%S"`
    pub expiration_date: String,
    /// Offset `+0x40`: `expires` — `true` si caduca en `expiration_date`
    pub expires: bool,
    /// Offset `+0x44`: `prioridad` — `0` = Alta, `1` = Baja
    pub priority: EventPriority,
    /// Offset `+0x18`: `hourBits` — Máscara de 24 bits (`bits 0..23`) para "Todas las horas" (`0` = "Cada día")
    pub hour_bits: u32,
    /// Offset `+0x50` (añadido en `EVT02`): `enabled`
    pub enabled: bool,
    /// Offset `+0x54` (añadido en `EVT03`): `maxWait` en minutos (`-1` = sin vencimiento en cola)
    pub max_wait_minutes: i32,
}

pub struct EvtFormat;

impl EvtFormat {
    /// Deserializa un flujo binario `.evt` (`EVT01`, `EVT02` o `EVT03`).
    pub fn parse_bytes(bytes: &[u8]) -> Result<(EvtVersion, Vec<ScheduledEvent>), FormatError> {
        if bytes.len() < 5 {
            return Err(FormatError::InvalidFormat(
                "Archivo .evt demasiado corto (< 5 bytes)".to_string(),
            ));
        }

        // Validación exacta de los 5 bytes mágicos en 0x00481f00..0x00481f27:
        // 'E' (0x45), 'V' (0x56), 'T' (0x54), '0' (0x30), '1'..'3' (0x31..0x33)
        let version = match &bytes[0..5] {
            b"EVT01" => EvtVersion::V1,
            b"EVT02" => EvtVersion::V2,
            b"EVT03" => EvtVersion::V3,
            other => {
                return Err(FormatError::InvalidFormat(format!(
                    "Cabecera mágica .evt desconocida: {:?}",
                    String::from_utf8_lossy(other)
                )))
            }
        };

        let mut cursor = Cursor::new(&bytes[5..]);
        let mut events = Vec::new();

        while (cursor.position() as usize) < bytes.len() - 5 {
            let immediate = read_u8(&mut cursor)? != 0;
            let day_hour_sec = read_i32_le(&mut cursor)?;
            let mut day_mask = [false; 7];
            for day in &mut day_mask {
                *day = read_u8(&mut cursor)? != 0;
            }
            let fichero = read_wx_string(&mut cursor)?;
            let start_date = read_wx_string(&mut cursor)?;
            let expiration_date = read_wx_string(&mut cursor)?;
            let expires = read_u8(&mut cursor)? != 0;
            let prio_raw = read_i32_le(&mut cursor)?;
            let priority = if prio_raw == 0 {
                EventPriority::High
            } else {
                EventPriority::Low
            };
            let hour_bits = read_i32_le(&mut cursor)? as u32;

            // Compatibilidad entre versiones EVT01, EVT02 y EVT03 (0x0047d6ef..0x0047d74c)
            let enabled = match version {
                EvtVersion::V1 => true,
                EvtVersion::V2 | EvtVersion::V3 => read_u8(&mut cursor)? != 0,
            };
            let max_wait_minutes = match version {
                EvtVersion::V1 | EvtVersion::V2 => -1,
                EvtVersion::V3 => read_i32_le(&mut cursor)?,
            };

            events.push(ScheduledEvent {
                id: uuid::Uuid::new_v4().to_string(),
                immediate,
                day_hour_sec,
                day_mask,
                track: TrackItem::from_raw_spec(0, &fichero),
                start_date,
                expiration_date,
                expires,
                priority,
                hour_bits,
                enabled,
                max_wait_minutes,
            });
        }

        Ok((version, events))
    }

    /// Serializa la lista de eventos en formato binario `EVT03` (`Evento::Save` en `0x0047d040`).
    pub fn serialize_bytes(events: &[ScheduledEvent]) -> Result<Vec<u8>, FormatError> {
        let mut out = Vec::new();
        out.write_all(EvtVersion::V3.magic_bytes())?;

        for ev in events {
            write_u8(&mut out, u8::from(ev.immediate))?;
            write_i32_le(&mut out, ev.day_hour_sec)?;
            for &day in &ev.day_mask {
                write_u8(&mut out, u8::from(day))?;
            }
            write_wx_string(&mut out, &ev.track.to_raw_spec())?;
            write_wx_string(&mut out, &ev.start_date)?;
            write_wx_string(&mut out, &ev.expiration_date)?;
            write_u8(&mut out, u8::from(ev.expires))?;
            write_i32_le(&mut out, ev.priority as i32)?;
            write_i32_le(&mut out, ev.hour_bits as i32)?;
            write_u8(&mut out, u8::from(ev.enabled))?;
            write_i32_le(&mut out, ev.max_wait_minutes)?;
        }

        Ok(out)
    }

    pub fn load_from_file(
        path: impl AsRef<Path>,
    ) -> Result<(EvtVersion, Vec<ScheduledEvent>), FormatError> {
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Self::parse_bytes(&bytes)
    }

    pub fn save_to_file(
        path: impl AsRef<Path>,
        events: &[ScheduledEvent],
    ) -> Result<(), FormatError> {
        let bytes = Self::serialize_bytes(events)?;
        let mut file = File::create(path)?;
        file.write_all(&bytes)?;
        file.flush()?;
        Ok(())
    }
}

fn read_u8(r: &mut impl Read) -> Result<u8, FormatError> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_i32_le(r: &mut impl Read) -> Result<i32, FormatError> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(i32::from_le_bytes(b))
}

/// `wxDataInputStream::ReadString` (`0x0075341e`): lee longitud `uint32` LE seguida de `len` bytes.
fn read_wx_string(r: &mut impl Read) -> Result<String, FormatError> {
    let len = read_i32_le(r)?;
    if !(0..=65536).contains(&len) {
        return Err(FormatError::InvalidFormat(format!(
            "Longitud de cadena wxString fuera de rango en .evt: {}",
            len
        )));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(match String::from_utf8(buf.clone()) {
        Ok(s) => s,
        Err(_) => buf.into_iter().map(|b| b as char).collect(),
    })
}

fn write_u8(w: &mut impl Write, v: u8) -> Result<(), FormatError> {
    w.write_all(&[v])?;
    Ok(())
}

fn write_i32_le(w: &mut impl Write, v: i32) -> Result<(), FormatError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

/// `wxDataOutputStream::WriteString` (`0x00753d9a`): escribe longitud `uint32` LE + bytes.
fn write_wx_string(w: &mut impl Write, s: &str) -> Result<(), FormatError> {
    let bytes = s.as_bytes();
    write_i32_le(w, bytes.len() as i32)?;
    w.write_all(bytes)?;
    Ok(())
}
