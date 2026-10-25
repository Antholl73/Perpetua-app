use crate::{FormatError, TrackItem};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Archivo de Rotación `.rot` o lista `.pls` (`inifiles.cpp` `0x00493fe0..0x004945f0`):
/// Sección `[playlist]`, claves `NumberOfEntries`, `File0..N`, `Length0..N`, `NextIndex`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RotationPlaylist {
    pub entries: Vec<TrackItem>,
    pub next_index: usize,
}

impl RotationPlaylist {
    pub fn parse_ini(content: &str) -> Self {
        let map = parse_ini_section(content, "playlist");
        let count = map
            .get("numberofentries")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let next_index = map
            .get("nextindex")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);

        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let file_key = format!("file{}", i);
            let len_key = format!("length{}", i);
            if let Some(raw_spec) = map.get(&file_key) {
                let dur = map
                    .get(&len_key)
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0);
                entries.push(TrackItem::from_raw_spec(dur, raw_spec));
            }
        }
        let resolved_next_index = if entries.is_empty() {
            0
        } else {
            next_index % entries.len()
        };
        Self {
            entries,
            next_index: resolved_next_index,
        }
    }

    /// Serializa a formato `.rot` (`File%d`, `Length%d`, `NextIndex`) o `.seq` (`file%d`, `length%d`).
    pub fn serialize_ini(&self, lowercase_keys: bool) -> String {
        let mut out = String::from("[playlist]\r\n");
        out.push_str(&format!("NumberOfEntries={}\r\n", self.entries.len()));
        for (i, item) in self.entries.iter().enumerate() {
            if lowercase_keys {
                out.push_str(&format!("file{}={}\r\n", i, item.to_raw_spec()));
                out.push_str(&format!("length{}={}\r\n", i, item.duration_ms));
            } else {
                out.push_str(&format!("File{}={}\r\n", i, item.to_raw_spec()));
                out.push_str(&format!("Length{}={}\r\n", i, item.duration_ms));
            }
        }
        if !lowercase_keys {
            out.push_str(&format!("NextIndex={}\r\n", self.next_index));
        }
        out
    }

    /// Obtiene la pista actual de la rotación y avanza circularmente `next_index`.
    pub fn next_track(&mut self) -> Option<TrackItem> {
        if self.entries.is_empty() {
            return None;
        }
        let idx = self.next_index % self.entries.len();
        let item = self.entries[idx].clone();
        self.next_index = (idx + 1) % self.entries.len();
        Some(item)
    }
}

/// Slot individual dentro de la paleta de Jingles (`.zsd` / Registro `Jingles/Page%d`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JingleSlot {
    /// Índice global `0..89` (10 páginas × 9 teclas `1..9`)
    pub index: usize,
    pub title: String,
    pub file_path: String,
    pub loop_enabled: bool,
    /// `Sweeper%d`: atenúa el reproductor principal a `Fader/FadePisa` mientras suena el jingle
    pub sweeper_ducking: bool,
}

/// Paleta completa de 90 cuñas instantáneas (10 páginas × 9 botones) reconstruida de
/// `PanelJingles::Load` (`0x004b0fd0`) y `PanelJingles::Save` (`0x004b15d0`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JinglePalette {
    pub page_names: [String; 10],
    pub slots: Vec<JingleSlot>,
    pub active_page: usize,
}

impl Default for JinglePalette {
    fn default() -> Self {
        let page_names = std::array::from_fn(|i| format!("Página {}", i + 1));
        let mut slots = Vec::with_capacity(90);
        for idx in 0..90 {
            slots.push(JingleSlot {
                index: idx,
                title: format!("Cuña {}", (idx % 9) + 1),
                file_path: String::new(),
                loop_enabled: false,
                sweeper_ducking: false,
            });
        }
        Self {
            page_names,
            slots,
            active_page: 0,
        }
    }
}

impl JinglePalette {
    pub fn parse_zsd(content: &str) -> Self {
        let mut palette = Self::default();
        let map = parse_ini_section(content, "jingles");
        for p in 0..10 {
            if let Some(name) = map.get(&format!("page{}", p)) {
                palette.page_names[p] = name.clone();
            }
        }
        for idx in 0..90 {
            if let Some(path) = map.get(&format!("jingle{}", idx)) {
                let loop_enabled = map
                    .get(&format!("loop{}", idx))
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                let sweeper_ducking = map
                    .get(&format!("sweeper{}", idx))
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                let title = path
                    .rsplit(['\\', '/'])
                    .next()
                    .and_then(|f| f.rsplit_once('.').map(|(s, _)| s))
                    .unwrap_or(path)
                    .to_string();
                palette.slots[idx] = JingleSlot {
                    index: idx,
                    title: if title.is_empty() {
                        format!("Cuña {}", (idx % 9) + 1)
                    } else {
                        title
                    },
                    file_path: path.clone(),
                    loop_enabled,
                    sweeper_ducking,
                };
            }
        }
        palette
    }

    pub fn serialize_zsd(&self) -> String {
        let mut out = String::from("[Jingles]\r\n");
        for (p, name) in self.page_names.iter().enumerate() {
            out.push_str(&format!("Page{}={}\r\n", p, name));
        }
        for slot in &self.slots {
            out.push_str(&format!("Jingle{}={}\r\n", slot.index, slot.file_path));
            out.push_str(&format!(
                "Loop{}={}\r\n",
                slot.index,
                u8::from(slot.loop_enabled)
            ));
            out.push_str(&format!(
                "Sweeper{}={}\r\n",
                slot.index,
                u8::from(slot.sweeper_ducking)
            ));
        }
        out
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, FormatError> {
        let content = fs::read_to_string(path)?;
        Ok(Self::parse_zsd(&content))
    }

    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<(), FormatError> {
        fs::write(path, self.serialize_zsd())?;
        Ok(())
    }
}

/// Parser y escritor para listas `.m3u` extendidas (`#EXTM3U` / `#EXTINF:%d,`, `0x0045c550`).
pub struct M3uFormat;

impl M3uFormat {
    pub fn parse_str(content: &str) -> Vec<TrackItem> {
        let mut items = Vec::new();
        let mut pending_duration_ms: i64 = 0;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("#EXTM3U") {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("#EXTINF:") {
                if let Some((sec_str, _title)) = rest.split_once(',') {
                    let secs = sec_str.trim().parse::<i64>().unwrap_or(0);
                    pending_duration_ms = secs * 1000;
                }
            } else if !trimmed.starts_with('#') {
                items.push(TrackItem::from_raw_spec(pending_duration_ms, trimmed));
                pending_duration_ms = 0;
            }
        }
        items
    }

    pub fn serialize_to_string(items: &[TrackItem]) -> String {
        let mut out = String::from("#EXTM3U\r\n");
        for item in items {
            let secs = (item.duration_ms / 1000).max(0);
            out.push_str(&format!("#EXTINF:{},{}\r\n", secs, item.display_title));
            out.push_str(&format!("{}\r\n", item.to_raw_spec()));
        }
        out
    }
}

fn parse_ini_section(content: &str, target_section: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut in_section = false;
    let target_lower = target_section.to_lowercase();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let sec = trimmed[1..trimmed.len() - 1].trim().to_lowercase();
            in_section = sec == target_lower;
            continue;
        }
        if in_section {
            if let Some((k, v)) = trimmed.split_once('=') {
                map.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
    }
    map
}
