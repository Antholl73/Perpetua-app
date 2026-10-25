use crate::FormatError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Registro de la tabla `sweepers` en `sweepers.db` y archivos `.zsl`, descompilado de:
/// - `TablaPisa` (`tablapisa.cpp`, `0x005448d0..0x005477b0`)
/// - `DPisadores::OnGuardar` / `OnAbrir` (`0x0046ed50..0x0046fcc0`)
///
/// Esquema SQLite3 original:
/// `CREATE TABLE sweepers(file VARCHAR(560) PRIMARY KEY, pisa VARCHAR(560), time INT, enabled BOOL)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweeperRecord {
    /// Ruta de la canción sobre la que actúa el pisador, o `"*"` para cualquier canción.
    pub target_song_file: String,
    /// Ruta del archivo de audio del pisador / locución (`pisa`).
    pub sweeper_file: String,
    /// Segundos de introducción de la canción (`time`), tras los cuales debe concluir (o iniciar) el pisador.
    pub intro_seconds: i32,
    /// Si la asociación está habilitada (`enabled`).
    pub enabled: bool,
}

pub struct SweeperDatabase {
    conn: Connection,
}

impl SweeperDatabase {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FormatError> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.ensure_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self, FormatError> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.ensure_schema()?;
        Ok(db)
    }

    fn ensure_schema(&self) -> Result<(), FormatError> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS sweepers(
                file VARCHAR(560) PRIMARY KEY,
                pisa VARCHAR(560),
                time INT,
                enabled BOOL
            )",
            [],
        )?;
        Ok(())
    }

    pub fn upsert(&self, record: &SweeperRecord) -> Result<(), FormatError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sweepers VALUES (?1, ?2, ?3, ?4)",
            params![
                record.target_song_file,
                record.sweeper_file,
                record.intro_seconds,
                i32::from(record.enabled),
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, target_song_file: &str) -> Result<(), FormatError> {
        self.conn.execute(
            "DELETE FROM sweepers WHERE file = ?1",
            params![target_song_file],
        )?;
        Ok(())
    }

    pub fn list_all(&self) -> Result<Vec<SweeperRecord>, FormatError> {
        let mut stmt = self
            .conn
            .prepare("SELECT file, pisa, time, enabled FROM sweepers ORDER BY file ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(SweeperRecord {
                target_song_file: row.get(0)?,
                sweeper_file: row.get(1)?,
                intro_seconds: row.get(2)?,
                enabled: row.get::<_, i32>(3)? != 0,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Reconstrucción segura (con parámetros ligados) de la consulta original de `TablaPisa::GetSweeper`
    /// (`0x00546e00`):
    /// `"SELECT pisa,time FROM sweepers WHERE (file LIKE '%s' OR file = '*') AND enabled = 1 ORDER BY RANDOM() LIMIT 1"`
    pub fn pick_sweeper_for_track(
        &self,
        song_path: &str,
    ) -> Result<Option<SweeperRecord>, FormatError> {
        let mut stmt = self.conn.prepare(
            "SELECT file, pisa, time, enabled
             FROM sweepers
             WHERE (LOWER(file) = LOWER(?1) OR file = '*') AND enabled = 1
             ORDER BY CASE WHEN file = '*' THEN 1 ELSE 0 END ASC, RANDOM()
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![song_path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(SweeperRecord {
                target_song_file: row.get(0)?,
                sweeper_file: row.get(1)?,
                intro_seconds: row.get(2)?,
                enabled: row.get::<_, i32>(3)? != 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Exporta la tabla actual a un archivo `.zsl` compatible con ZaraRadio 1.6.2 (`0x0046ed50`).
    pub fn export_zsl(&self, target_path: impl AsRef<Path>) -> Result<(), FormatError> {
        let records = self.list_all()?;
        if target_path.as_ref().exists() {
            let _ = std::fs::remove_file(&target_path);
        }
        let target_db = Self::open(target_path)?;
        target_db.conn.execute("BEGIN TRANSACTION", [])?;
        for rec in &records {
            target_db.upsert(rec)?;
        }
        target_db.conn.execute("COMMIT TRANSACTION", [])?;
        Ok(())
    }
}
