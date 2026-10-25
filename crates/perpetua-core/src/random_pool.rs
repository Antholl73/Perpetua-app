use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

/// Límite máximo del historial global de pistas reproducidas para la regla de No Repetición (Hito 7).
pub const MAX_ANTI_REPEAT_HISTORY: usize = 50;

/// Extensiones de audio soportadas al escanear una Carpeta Aleatoria (`Random Folder`).
pub const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "flac", "ogg", "opus", "m4a", "aac", "aiff", "wma",
];

/// Motor de Selección Aleatoria con Regla de No Repetición (Hito 7):
/// - Mantiene un registro circular de los últimos 50 archivos reproducidos (`recent_history`).
/// - Al evaluar una pista de tipo Carpeta (`RandomFolder`), escanea el directorio dinámicamente,
///   filtra los candidatos para evitar elegir cualquier archivo presente en los últimos 50 reproducidos
///   (y nunca repetir la misma canción dos veces seguidas incluso en carpetas pequeñas),
///   y registra la pista elegida en el historial.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntiRepeatPool {
    /// Cola FIFO con los últimos `MAX_ANTI_REPEAT_HISTORY` (50) archivos reproducidos (el más reciente al final).
    pub recent_history: VecDeque<String>,
    /// Historial por carpeta para garantizar rotación completa dentro de cada directorio antes de reiniciar su ciclo.
    #[serde(default)]
    pub folder_cycle_played: HashMap<String, HashSet<String>>,
    /// Último archivo elegido por cada carpeta para impedir repetición consecutiva inmediata incluso si la carpeta tiene 2 archivos.
    #[serde(default)]
    pub last_picked_per_folder: HashMap<String, String>,
}

impl AntiRepeatPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra un archivo reproducido en el historial global de los últimos 50 audios.
    pub fn record_played(&mut self, file_path: &str) {
        let normalized = file_path.trim().to_string();
        if normalized.is_empty() {
            return;
        }
        // Si ya estaba en la cola, lo movemos al final como el más reciente
        if let Some(pos) = self.recent_history.iter().position(|p| p == &normalized) {
            self.recent_history.remove(pos);
        }
        if self.recent_history.len() >= MAX_ANTI_REPEAT_HISTORY {
            self.recent_history.pop_front();
        }
        self.recent_history.push_back(normalized);
    }

    /// Devuelve la lista de los últimos archivos reproducidos (del más reciente al más antiguo).
    pub fn recent_tracks(&self) -> Vec<String> {
        self.recent_history.iter().rev().cloned().collect()
    }

    /// Escanea `dir_path` y elige un archivo de audio al azar respetando:
    /// 1) No repetir ninguna pista presente en los últimos 50 archivos reproducidos (`recent_history`).
    /// 2) No repetir pistas ya sonadas en el ciclo actual de esa carpeta hasta agotar todas sus pistas.
    /// 3) Si la carpeta tiene menos de 50 archivos y todos están en `recent_history`, jamás repetir
    ///    el último archivo reproducido (evitando 100% que suene dos veces seguidas).
    pub fn pick_non_repeating_track(&mut self, dir_path: &str) -> Result<String, String> {
        let root = Path::new(dir_path.trim());
        if !root.exists() {
            return Err(format!("La carpeta aleatoria no existe: {}", dir_path));
        }
        if !root.is_dir() {
            return Err(format!(
                "La ruta especificada no es un directorio: {}",
                dir_path
            ));
        }

        let all_tracks = scan_audio_files_recursive(root);
        if all_tracks.is_empty() {
            return Err(format!(
                "La carpeta '{}' no contiene archivos de audio compatibles (.wav, .mp3, .flac, .ogg).",
                dir_path
            ));
        }

        let folder_key = root.to_string_lossy().to_string();
        let last_global = self.recent_history.back().cloned();
        let last_in_folder = self.last_picked_per_folder.get(&folder_key).cloned();

        // Si la carpeta solo tiene 1 archivo, es el único candidato posible
        if all_tracks.len() == 1 {
            let only = all_tracks[0].clone();
            self.record_played(&only);
            self.last_picked_per_folder
                .insert(folder_key, only.clone());
            return Ok(only);
        }

        let recent_set: HashSet<&str> = self.recent_history.iter().map(|s| s.as_str()).collect();
        let cycle_set = self
            .folder_cycle_played
            .entry(folder_key.clone())
            .or_default();

        // Nivel 1 (óptimo): No está en los últimos 50 globales NI en el ciclo actual de la carpeta
        let mut candidates: Vec<&String> = all_tracks
            .iter()
            .filter(|t| !recent_set.contains(t.as_str()) && !cycle_set.contains(*t))
            .collect();

        // Nivel 2: Si se agotó el ciclo de la carpeta, reiniciamos el ciclo pero seguimos respetando los últimos 50 globales
        if candidates.is_empty() {
            cycle_set.clear();
            candidates = all_tracks
                .iter()
                .filter(|t| !recent_set.contains(t.as_str()))
                .collect();
        }

        // Nivel 3: Si la carpeta es pequeña (ej. 5 canciones) y todas están dentro de los últimos 50 globales,
        // usamos el ciclo interno de la carpeta evitando estrictamente el último archivo reproducido.
        if candidates.is_empty() {
            candidates = all_tracks
                .iter()
                .filter(|t| {
                    !cycle_set.contains(*t)
                        && Some(*t) != last_global.as_ref()
                        && Some(*t) != last_in_folder.as_ref()
                })
                .collect();
        }

        // Nivel 4: Si el ciclo interno solo dejaba la última canción reproducida, reiniciamos ciclo y excluimos la última reproducida
        if candidates.is_empty() {
            cycle_set.clear();
            candidates = all_tracks
                .iter()
                .filter(|t| {
                    Some(*t) != last_global.as_ref() && Some(*t) != last_in_folder.as_ref()
                })
                .collect();
        }

        // Nivel 5 (fallback final para carpeta con 2 canciones idénticas a last_global/last_in_folder):
        if candidates.is_empty() {
            candidates = all_tracks
                .iter()
                .filter(|t| Some(*t) != last_global.as_ref())
                .collect();
        }

        let mut rng = rand::thread_rng();
        let chosen = (*candidates
            .choose(&mut rng)
            .unwrap_or(&&all_tracks[0]))
        .clone();

        cycle_set.insert(chosen.clone());
        self.last_picked_per_folder
            .insert(folder_key, chosen.clone());
        self.record_played(&chosen);

        Ok(chosen)
    }
}

/// Escanea recursivamente un directorio y devuelve todas las rutas de archivos de audio soportados.
pub fn scan_audio_files_recursive(root: &Path) -> Vec<String> {
    let mut result = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if SUPPORTED_AUDIO_EXTENSIONS
                        .iter()
                        .any(|&allowed| allowed.eq_ignore_ascii_case(ext))
                    {
                        result.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    result.sort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_caps_at_50_and_orders_most_recent() {
        let mut pool = AntiRepeatPool::new();
        for i in 1..=65 {
            pool.record_played(&format!("/music/song_{:03}.mp3", i));
        }
        assert_eq!(pool.recent_history.len(), MAX_ANTI_REPEAT_HISTORY);
        assert_eq!(
            pool.recent_history.front().map(String::as_str),
            Some("/music/song_016.mp3")
        );
        assert_eq!(
            pool.recent_history.back().map(String::as_str),
            Some("/music/song_065.mp3")
        );
    }

    #[test]
    fn test_random_folder_never_repeats_consecutively_or_within_50() {
        let temp_dir = std::env::temp_dir().join("perpetua_test_random_folder_hito7");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Crear 10 archivos de audio simulados en la carpeta
        for i in 1..=10 {
            let p = temp_dir.join(format!("track_{:02}.wav", i));
            fs::write(&p, b"RIFF0000WAVE").unwrap();
        }

        let mut pool = AntiRepeatPool::new();
        let dir_str = temp_dir.to_str().unwrap();

        // 1) Las primeras 10 selecciones deben recorrer los 10 archivos sin repetir ninguno
        let mut first_cycle = HashSet::new();
        let mut prev_pick = String::new();
        for _ in 0..10 {
            let pick = pool
                .pick_non_repeating_track(dir_str)
                .expect("debe elegir pista de la carpeta");
            assert_ne!(pick, prev_pick, "nunca debe repetir dos veces seguidas");
            assert!(
                first_cycle.insert(pick.clone()),
                "no debe repetir pista antes de agotar la carpeta"
            );
            prev_pick = pick;
        }
        assert_eq!(first_cycle.len(), 10);

        // 2) En las siguientes 30 selecciones (incluso con los 10 archivos dentro del historial de 50),
        // jamás debe elegir la misma canción dos veces seguidas.
        for _ in 0..30 {
            let pick = pool
                .pick_non_repeating_track(dir_str)
                .expect("debe elegir pista en ciclos subsiguientes");
            assert_ne!(
                pick, prev_pick,
                "jamás debe elegir la misma canción dos veces seguidas"
            );
            prev_pick = pick;
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
