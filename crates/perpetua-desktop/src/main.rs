#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Emitter;
use perpetua_core::{
    CueTelemetry, EventInterruptionMode, FolderAudioEntry, MinimalCore, PlaybackMode,
    PlaylistStatus, WaveformEditorData,
};

#[tauri::command]
async fn play(
    path: Option<String>,
    index: Option<usize>,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.play(path.as_deref(), index))
        .await
        .map_err(|e| format!("Error en tarea asíncrona play: {}", e))?
}

#[tauri::command]
fn stop(core: tauri::State<'_, MinimalCore>) -> Result<PlaylistStatus, String> {
    Ok(core.stop())
}

#[tauri::command]
fn get_status(core: tauri::State<'_, MinimalCore>) -> Result<PlaylistStatus, String> {
    Ok(core.get_status())
}

#[tauri::command]
async fn add_tracks(
    paths: Vec<String>,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.add_tracks(paths))
        .await
        .map_err(|e| format!("Error en tarea asíncrona add_tracks: {}", e))
}

#[tauri::command]
fn remove_track(
    index: usize,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    core.remove_track(index)
}

#[tauri::command]
fn clear_playlist(core: tauri::State<'_, MinimalCore>) -> Result<PlaylistStatus, String> {
    Ok(core.clear_playlist())
}

#[tauri::command]
async fn next_track(core: tauri::State<'_, MinimalCore>) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.next_track())
        .await
        .map_err(|e| format!("Error en tarea asíncrona next_track: {}", e))?
}

#[tauri::command]
fn set_playback_mode(
    mode: PlaybackMode,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    Ok(core.set_playback_mode(mode))
}

#[tauri::command]
fn set_crossfade(
    crossfade_ms: u64,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    Ok(core.set_crossfade(crossfade_ms))
}

#[tauri::command]
async fn assign_jingle(
    slot: usize,
    path: Option<String>,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.assign_jingle(slot, path.as_deref()))
        .await
        .map_err(|e| format!("Error en tarea asíncrona assign_jingle: {}", e))?
}

#[tauri::command]
async fn play_jingle(
    slot: usize,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.play_jingle(slot))
        .await
        .map_err(|e| format!("Error en tarea asíncrona play_jingle: {}", e))?
}

#[tauri::command]
async fn play_cue(
    path: String,
    app: tauri::AppHandle,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    let st = tauri::async_runtime::spawn_blocking(move || core_clone.play_cue(&path))
        .await
        .map_err(|e| format!("Error en tarea asíncrona play_cue: {}", e))??;
    let _ = app.emit("cue-telemetry", &core.get_cue_telemetry());
    Ok(st)
}

#[tauri::command]
async fn seek_cue_deck(
    path: Option<String>,
    position_ms: u64,
    app: tauri::AppHandle,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    let st = tauri::async_runtime::spawn_blocking(move || {
        core_clone.seek_cue_deck(path.as_deref(), position_ms)
    })
    .await
    .map_err(|e| format!("Error en tarea asíncrona seek_cue_deck: {}", e))??;
    let _ = app.emit("cue-telemetry", &core.get_cue_telemetry());
    Ok(st)
}

#[tauri::command]
fn get_cue_telemetry(core: tauri::State<'_, MinimalCore>) -> Result<CueTelemetry, String> {
    Ok(core.get_cue_telemetry())
}

#[tauri::command]
fn stop_cue(
    app: tauri::AppHandle,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let st = core.stop_cue();
    let _ = app.emit("cue-telemetry", &core.get_cue_telemetry());
    Ok(st)
}

#[tauri::command]
async fn set_audio_routing(
    air_device: String,
    cue_device: String,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core_clone.set_audio_routing(air_device, cue_device)
    })
    .await
    .map_err(|e| format!("Error en tarea asíncrona set_audio_routing: {}", e))?
}

#[tauri::command]
async fn refresh_devices(core: tauri::State<'_, MinimalCore>) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.refresh_devices())
        .await
        .map_err(|e| format!("Error en tarea asíncrona refresh_devices: {}", e))
}

#[tauri::command]
async fn save_playlist(
    path: String,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.save_playlist(&path))
        .await
        .map_err(|e| format!("Error en tarea asíncrona save_playlist: {}", e))?
}

#[tauri::command]
async fn load_playlist(
    path: String,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.load_playlist(&path))
        .await
        .map_err(|e| format!("Error en tarea asíncrona load_playlist: {}", e))?
}

#[tauri::command]
async fn add_event(
    time: String,
    days: Vec<u8>,
    path: String,
    wait_for_current: Option<bool>,
    interruption_mode: Option<EventInterruptionMode>,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    let mode = interruption_mode.unwrap_or_else(|| {
        if wait_for_current.unwrap_or(false) {
            EventInterruptionMode::WaitForCurrent
        } else {
            EventInterruptionMode::Ducking
        }
    });
    tauri::async_runtime::spawn_blocking(move || {
        core_clone.add_event_with_mode(time, days, path, mode)
    })
    .await
    .map_err(|e| format!("Error en tarea asíncrona add_event: {}", e))?
}

#[tauri::command]
fn remove_event(id: u64, core: tauri::State<'_, MinimalCore>) -> Result<PlaylistStatus, String> {
    core.remove_event(id)
}

#[tauri::command]
fn toggle_event(
    id: u64,
    enabled: bool,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    core.toggle_event(id, enabled)
}

#[tauri::command]
async fn trigger_event_now(
    id: u64,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.trigger_event_now(id))
        .await
        .map_err(|e| format!("Error en tarea asíncrona trigger_event_now: {}", e))?
}

#[tauri::command]
async fn add_random_folder(
    path: String,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.add_random_folder(&path))
        .await
        .map_err(|e| format!("Error en tarea asíncrona add_random_folder: {}", e))?
}

#[tauri::command]
fn add_time_announcement(core: tauri::State<'_, MinimalCore>) -> Result<PlaylistStatus, String> {
    Ok(core.add_time_announcement())
}

#[tauri::command]
async fn get_folder_contents(
    path: String,
    core: tauri::State<'_, MinimalCore>,
) -> Result<Vec<FolderAudioEntry>, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.get_folder_contents(&path))
        .await
        .map_err(|e| format!("Error en tarea asíncrona get_folder_contents: {}", e))?
}

#[tauri::command]
async fn set_track_intro(
    path: String,
    intro_ms: u64,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.set_track_intro(&path, intro_ms))
        .await
        .map_err(|e| format!("Error en tarea asíncrona set_track_intro: {}", e))?
}

#[tauri::command]
async fn set_track_sweeper(
    index: Option<usize>,
    path: Option<String>,
    sweeper_path: Option<String>,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core_clone.set_track_sweeper(index, path.as_deref(), sweeper_path.as_deref())
    })
    .await
    .map_err(|e| format!("Error en tarea asíncrona set_track_sweeper: {}", e))?
}

#[tauri::command]
async fn set_track_markers(
    index: Option<usize>,
    path: String,
    intro_ms: u64,
    sweeper_path: Option<String>,
    core: tauri::State<'_, MinimalCore>,
) -> Result<PlaylistStatus, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core_clone.set_track_markers(index, &path, intro_ms, sweeper_path.as_deref())
    })
    .await
    .map_err(|e| format!("Error en tarea asíncrona set_track_markers: {}", e))?
}

#[tauri::command]
async fn get_track_waveform(
    path: String,
    buckets: Option<usize>,
    core: tauri::State<'_, MinimalCore>,
) -> Result<WaveformEditorData, String> {
    let core_clone = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core_clone.get_track_waveform(&path, buckets))
        .await
        .map_err(|e| format!("Error en tarea asíncrona get_track_waveform: {}", e))?
}

fn main() {
    let core = MinimalCore::new();
    let core_for_telemetry = core.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(core)
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let telemetry_core = core_for_telemetry.clone();
            std::thread::Builder::new()
                .name("perpetua-cue-telemetry".to_string())
                .spawn(move || {
                    let mut was_playing = false;
                    let mut last_pos_ms = u64::MAX;
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        let telemetry = telemetry_core.get_cue_telemetry();
                        // Emitir cada ~20ms exclusivamente cuando el Cue Deck está sonando
                        // o en el instante de transición de parada / salto (Seek), sin saturar IPC en reposo
                        if telemetry.is_playing
                            || was_playing
                            || (telemetry.cue_file.is_some()
                                && telemetry.current_position_ms != last_pos_ms)
                        {
                            let _ = app_handle.emit("cue-telemetry", &telemetry);
                            was_playing = telemetry.is_playing;
                            last_pos_ms = telemetry.current_position_ms;
                        }
                    }
                })
                .expect("No se pudo iniciar el hilo de telemetría del Cue Deck");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            play,
            stop,
            get_status,
            add_tracks,
            remove_track,
            clear_playlist,
            next_track,
            set_playback_mode,
            set_crossfade,
            assign_jingle,
            play_jingle,
            play_cue,
            seek_cue_deck,
            get_cue_telemetry,
            stop_cue,
            set_audio_routing,
            refresh_devices,
            save_playlist,
            load_playlist,
            add_event,
            remove_event,
            toggle_event,
            trigger_event_now,
            add_random_folder,
            add_time_announcement,
            get_folder_contents,
            set_track_intro,
            set_track_sweeper,
            set_track_markers,
            get_track_waveform
        ])
        .run(tauri::generate_context!())
        .expect("error al ejecutar la aplicación Tauri v2 de Perpetua");
}
