use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use perpetua_audio::compute_sweeper_trigger_ms;

/// Registro de metadatos de mezcla y pisador (Hito 8) para una canción individual.
/// Guarda en SQLite local tanto el 'Tiempo de Intro' con precisión de milisegundos (ej. `12500` ms para `00:12.5`)
/// como el archivo de Pisador (`Sweeper`) asignado y su cálculo de disparo `(Intro - Duración Pisador)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TrackMetadataRecord {
    pub path: String,
    /// Tiempo de Intro en milisegundos (ej. `12500` ms = `00:12.5`).
    pub intro_ms: u64,
    /// Ruta del archivo de Pisador (`Sweeper`) asignado a esta canción.
    pub sweeper_path: Option<String>,
    /// Nombre legible del archivo de Pisador.
    pub sweeper_title: Option<String>,
    /// Duración del Pisador en milisegundos.
    pub sweeper_duration_ms: u64,
    /// Instante calculado de disparo en milisegundos: `(Tiempo de Intro) - (Duración del Pisador)`.
    pub trigger_at_ms: u64,
}

impl TrackMetadataRecord {
    pub fn from_raw(
        path: String,
        intro_ms: u64,
        sweeper_path: Option<String>,
        sweeper_duration_ms: u64,
    ) -> Self {
        let clean_sweeper = sweeper_path
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let sweeper_title = clean_sweeper.as_ref().map(|sp| {
            Path::new(sp)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(sp)
                .to_string()
        });
        let eff_dur = if clean_sweeper.is_some() {
            sweeper_duration_ms
        } else {
            0
        };
        let trigger_at_ms = if clean_sweeper.is_some() {
            compute_sweeper_trigger_ms(intro_ms, eff_dur)
        } else {
            0
        };

        Self {
            path,
            intro_ms,
            sweeper_path: clean_sweeper,
            sweeper_title,
            sweeper_duration_ms: eff_dur,
            trigger_at_ms,
        }
    }
}

/// Gestor de Metadatos en SQLite local para almacenar el 'Tiempo de Intro' (`intro_ms`)
/// y la asignación de Pisadores (`sweeper_path`, `sweeper_duration_ms`) por canción.
/// También mantiene sincronizada la tabla clásica `sweepers` de Perpetua.
pub struct TrackMetadataStore {
    conn: Connection,
}

impl std::fmt::Debug for TrackMetadataStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackMetadataStore").finish()
    }
}

impl Default for TrackMetadataStore {
    fn default() -> Self {
        Self::open_in_memory().expect("debe crear SQLite en memoria")
    }
}

impl TrackMetadataStore {
    pub fn open_or_memory(path: impl AsRef<Path>) -> Self {
        Self::open(path).unwrap_or_else(|_| Self::default())
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let conn = Connection::open(path.as_ref()).map_err(|e| {
            format!(
                "No se pudo abrir la base de datos SQLite de metadatos '{}': {}",
                path.as_ref().display(),
                e
            )
        })?;
        let store = Self { conn };
        store.ensure_schema()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("Error al crear SQLite en memoria: {}", e))?;
        let store = Self { conn };
        store.ensure_schema()?;
        Ok(store)
    }

    fn ensure_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS track_metadata (
                    file_path TEXT PRIMARY KEY,
                    intro_ms INTEGER NOT NULL DEFAULT 0,
                    sweeper_path TEXT,
                    sweeper_duration_ms INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS sweepers (
                    file VARCHAR(560) PRIMARY KEY,
                    pisa VARCHAR(560),
                    time INT,
                    enabled BOOL
                );",
            )
            .map_err(|e| format!("Error inicializando esquema SQLite de metadatos: {}", e))
    }

    pub fn get(&self, file_path: &str) -> Option<TrackMetadataRecord> {
        let trimmed = file_path.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_path, intro_ms, sweeper_path, sweeper_duration_ms
                 FROM track_metadata
                 WHERE file_path = ?1",
            )
            .ok()?;

        stmt.query_row(params![trimmed], |row| {
            let path: String = row.get(0)?;
            let intro_ms_i64: i64 = row.get(1)?;
            let sweeper_path: Option<String> = row.get(2)?;
            let sweeper_dur_i64: i64 = row.get(3)?;
            Ok(TrackMetadataRecord::from_raw(
                path,
                intro_ms_i64.max(0) as u64,
                sweeper_path,
                sweeper_dur_i64.max(0) as u64,
            ))
        })
        .ok()
    }

    pub fn upsert(&self, record: &TrackMetadataRecord) -> Result<TrackMetadataRecord, String> {
        let normalized = TrackMetadataRecord::from_raw(
            record.path.trim().to_string(),
            record.intro_ms,
            record.sweeper_path.clone(),
            record.sweeper_duration_ms,
        );

        self.conn
            .execute(
                "INSERT INTO track_metadata (file_path, intro_ms, sweeper_path, sweeper_duration_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(file_path) DO UPDATE SET
                    intro_ms = excluded.intro_ms,
                    sweeper_path = excluded.sweeper_path,
                    sweeper_duration_ms = excluded.sweeper_duration_ms",
                params![
                    normalized.path,
                    normalized.intro_ms as i64,
                    normalized.sweeper_path,
                    normalized.sweeper_duration_ms as i64,
                ],
            )
            .map_err(|e| format!("Error al guardar metadatos de pista en SQLite: {}", e))?;

        // Sincronizar también con la tabla clásica `sweepers` (time en segundos enteros)
        let intro_secs = ((normalized.intro_ms + 500) / 1000) as i32;
        if let Some(ref pisa) = normalized.sweeper_path {
            let _ = self.conn.execute(
                "INSERT OR REPLACE INTO sweepers (file, pisa, time, enabled) VALUES (?1, ?2, ?3, 1)",
                params![normalized.path, pisa, intro_secs],
            );
        } else {
            let _ = self.conn.execute(
                "DELETE FROM sweepers WHERE file = ?1",
                params![normalized.path],
            );
        }

        Ok(normalized)
    }

    pub fn set_intro_ms(
        &self,
        file_path: &str,
        intro_ms: u64,
    ) -> Result<TrackMetadataRecord, String> {
        let existing = self.get(file_path).unwrap_or_else(|| TrackMetadataRecord {
            path: file_path.trim().to_string(),
            ..Default::default()
        });
        let updated = TrackMetadataRecord::from_raw(
            existing.path,
            intro_ms,
            existing.sweeper_path,
            existing.sweeper_duration_ms,
        );
        self.upsert(&updated)
    }

    pub fn set_sweeper(
        &self,
        file_path: &str,
        sweeper_path: Option<&str>,
        sweeper_duration_ms: u64,
    ) -> Result<TrackMetadataRecord, String> {
        let existing = self.get(file_path).unwrap_or_else(|| TrackMetadataRecord {
            path: file_path.trim().to_string(),
            ..Default::default()
        });
        let updated = TrackMetadataRecord::from_raw(
            existing.path,
            existing.intro_ms,
            sweeper_path.map(|s| s.to_string()),
            sweeper_duration_ms,
        );
        self.upsert(&updated)
    }

    pub fn set_markers(
        &self,
        file_path: &str,
        intro_ms: u64,
        sweeper_path: Option<&str>,
        sweeper_duration_ms: u64,
    ) -> Result<TrackMetadataRecord, String> {
        let updated = TrackMetadataRecord::from_raw(
            file_path.trim().to_string(),
            intro_ms,
            sweeper_path.map(|s| s.to_string()),
            sweeper_duration_ms,
        );
        self.upsert(&updated)
    }

    pub fn list_all(&self) -> Vec<TrackMetadataRecord> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT file_path, intro_ms, sweeper_path, sweeper_duration_ms
             FROM track_metadata
             ORDER BY file_path ASC",
        ) else {
            return Vec::new();
        };

        let Ok(rows) = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let intro_ms_i64: i64 = row.get(1)?;
            let sweeper_path: Option<String> = row.get(2)?;
            let sweeper_dur_i64: i64 = row.get(3)?;
            Ok(TrackMetadataRecord::from_raw(
                path,
                intro_ms_i64.max(0) as u64,
                sweeper_path,
                sweeper_dur_i64.max(0) as u64,
            ))
        }) else {
            return Vec::new();
        };

        rows.flatten().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_track_metadata_intro_and_sweeper_trigger_calculation() {
        let store = TrackMetadataStore::open_in_memory().unwrap();

        // Guardar Tiempo de Intro = 00:12.5 (12_500 ms)
        let rec1 = store
            .set_intro_ms("/musica/cancion_pop.mp3", 12_500)
            .expect("debe guardar intro_ms en SQLite");
        assert_eq!(rec1.intro_ms, 12_500);
        assert!(rec1.sweeper_path.is_none());

        // Asignar Pisador de 4s (4_000 ms) -> disparo exacto en 12.5s - 4.0s = 8.5s (8_500 ms)
        let rec2 = store
            .set_sweeper("/musica/cancion_pop.mp3", Some("/pisadores/id_radio.wav"), 4_000)
            .expect("debe guardar pisador en SQLite");
        assert_eq!(rec2.intro_ms, 12_500);
        assert_eq!(
            rec2.sweeper_path.as_deref(),
            Some("/pisadores/id_radio.wav")
        );
        assert_eq!(rec2.sweeper_title.as_deref(), Some("id_radio.wav"));
        assert_eq!(rec2.sweeper_duration_ms, 4_000);
        assert_eq!(rec2.trigger_at_ms, 8_500);

        // Consultar de nuevo desde SQLite
        let fetched = store
            .get("/musica/cancion_pop.mp3")
            .expect("debe existir en SQLite");
        assert_eq!(fetched, rec2);
    }
}
