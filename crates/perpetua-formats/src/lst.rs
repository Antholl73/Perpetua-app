use crate::{FormatError, TrackItem};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

/// Lector y escritor del formato `.lst` de ZaraRadio 1.6.2, reconstruido a partir de:
/// - `VentanaPrincipal::GuardarLista` (`0x004598d0`..`0x00459b60`)
/// - `VentanaPrincipal::AbrirLista` (`0x00459c50`..`0x0045a150`)
///
/// Formato exacto:
/// - Línea 1: `<count>\n` (`"%d\n"` en `0x00888330`)
/// - Líneas `2..count+1`: `<duration_ms>\t<raw_spec>\n` (`"%d\t"` en `0x00888334`)
pub struct LstFormat;

impl LstFormat {
    pub fn parse_str(content: &str) -> Result<Vec<TrackItem>, FormatError> {
        let mut lines = content.lines();
        let first_line = match lines.next() {
            Some(l) => l.trim(),
            None => return Ok(Vec::new()),
        };
        if first_line.is_empty() {
            return Ok(Vec::new());
        }

        let expected_count: usize = first_line
            .split([' ', '\t'])
            .next()
            .unwrap_or("0")
            .parse()
            .map_err(|_| {
                FormatError::InvalidFormat(format!(
                    "Cabecera .lst inválida (se esperaba número de entradas): {}",
                    first_line
                ))
            })?;

        let mut items = Vec::with_capacity(expected_count.min(65536));
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                continue;
            }
            if let Some((dur_str, raw_path)) = trimmed.split_once('\t') {
                let duration_ms = dur_str.trim().parse::<i64>().unwrap_or(0);
                items.push(TrackItem::from_raw_spec(duration_ms, raw_path));
            }
        }
        Ok(items)
    }

    pub fn serialize_to_string(items: &[TrackItem]) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}\n", items.len()));
        for item in items {
            out.push_str(&format!("{}\t{}\n", item.duration_ms, item.to_raw_spec()));
        }
        out
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Vec<TrackItem>, FormatError> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut buf)?;
        // ZaraRadio 1.6.2 usa ANSI/Latin-1 (Windows-1252); soportamos UTF-8 y fallback Latin-1
        let text = match String::from_utf8(buf.clone()) {
            Ok(s) => s,
            Err(_) => buf.into_iter().map(|b| b as char).collect(),
        };
        Self::parse_str(&text)
    }

    pub fn save_to_file(path: impl AsRef<Path>, items: &[TrackItem]) -> Result<(), FormatError> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        let content = Self::serialize_to_string(items);
        writer.write_all(content.as_bytes())?;
        writer.flush()?;
        Ok(())
    }
}
