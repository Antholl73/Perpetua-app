use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tower_http::{cors::CorsLayer, services::ServeDir};
use perpetua_core::{
    CueTelemetry, EventInterruptionMode, FolderAudioEntry, MinimalCore, PlaybackMode,
    PlaylistStatus, WaveformEditorData,
};

#[derive(Deserialize, Default)]
pub struct PlayRequest {
    pub path: Option<String>,
    pub index: Option<usize>,
}

#[derive(Deserialize)]
pub struct AddTracksRequest {
    pub paths: Vec<String>,
}

#[derive(Deserialize)]
pub struct RemoveTrackRequest {
    pub index: usize,
}

#[derive(Deserialize)]
pub struct SetModeRequest {
    pub mode: PlaybackMode,
}

#[derive(Deserialize)]
pub struct SetCrossfadeRequest {
    pub crossfade_ms: u64,
}

#[derive(Deserialize)]
pub struct AssignJingleRequest {
    pub slot: usize,
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub struct PlayJingleRequest {
    pub slot: usize,
}

#[derive(Deserialize)]
pub struct FilePathRequest {
    pub path: String,
}

#[derive(Deserialize)]
pub struct SeekCueDeckRequest {
    #[serde(default)]
    pub path: Option<String>,
    pub position_ms: u64,
}

#[derive(Deserialize)]
pub struct AudioRoutingRequest {
    pub air_device: String,
    pub cue_device: String,
}

#[derive(Deserialize)]
pub struct AddEventRequest {
    pub time: String,
    pub days: Vec<u8>,
    pub path: String,
    #[serde(default)]
    pub wait_for_current: Option<bool>,
    #[serde(default)]
    pub interruption_mode: Option<EventInterruptionMode>,
}

#[derive(Deserialize)]
pub struct EventIdRequest {
    pub id: u64,
}

#[derive(Deserialize)]
pub struct ToggleEventRequest {
    pub id: u64,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct SetTrackIntroRequest {
    pub path: String,
    pub intro_ms: u64,
}

#[derive(Deserialize)]
pub struct SetTrackSweeperRequest {
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sweeper_path: Option<String>,
}

#[derive(Deserialize)]
pub struct SetTrackMarkersRequest {
    #[serde(default)]
    pub index: Option<usize>,
    pub path: String,
    pub intro_ms: u64,
    #[serde(default)]
    pub sweeper_path: Option<String>,
}

#[derive(Deserialize)]
pub struct GetTrackWaveformRequest {
    pub path: String,
    #[serde(default)]
    pub buckets: Option<usize>,
}

pub fn build_router(core: MinimalCore) -> Router {
    let dist_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    Router::new()
        .route("/api/play", post(play_handler))
        .route("/api/stop", post(stop_handler))
        .route("/api/status", get(get_status_handler))
        .route("/api/add_tracks", post(add_tracks_handler))
        .route("/api/remove_track", post(remove_track_handler))
        .route("/api/clear_playlist", post(clear_playlist_handler))
        .route("/api/next_track", post(next_track_handler))
        .route("/api/set_playback_mode", post(set_playback_mode_handler))
        .route("/api/set_crossfade", post(set_crossfade_handler))
        .route("/api/assign_jingle", post(assign_jingle_handler))
        .route("/api/play_jingle", post(play_jingle_handler))
        .route("/api/play_cue", post(play_cue_handler))
        .route("/api/seek_cue_deck", post(seek_cue_deck_handler))
        .route("/api/cue_telemetry", get(get_cue_telemetry_handler))
        .route("/api/stop_cue", post(stop_cue_handler))
        .route("/api/set_audio_routing", post(set_audio_routing_handler))
        .route("/api/refresh_devices", post(refresh_devices_handler))
        .route("/api/save_playlist", post(save_playlist_handler))
        .route("/api/load_playlist", post(load_playlist_handler))
        .route("/api/add_event", post(add_event_handler))
        .route("/api/remove_event", post(remove_event_handler))
        .route("/api/toggle_event", post(toggle_event_handler))
        .route("/api/trigger_event_now", post(trigger_event_now_handler))
        .route("/api/add_random_folder", post(add_random_folder_handler))
        .route("/api/add_time_announcement", post(add_time_announcement_handler))
        .route("/api/get_folder_contents", post(get_folder_contents_handler))
        .route("/api/set_track_intro", post(set_track_intro_handler))
        .route("/api/set_track_sweeper", post(set_track_sweeper_handler))
        .route("/api/set_track_markers", post(set_track_markers_handler))
        .route("/api/get_track_waveform", post(get_track_waveform_handler))
        .fallback_service(ServeDir::new(dist_dir))
        .layer(CorsLayer::permissive())
        .with_state(core)
}

async fn play_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<PlayRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.play(req.path.as_deref(), req.index))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn stop_handler(State(core): State<MinimalCore>) -> Json<PlaylistStatus> {
    Json(core.stop())
}

async fn get_status_handler(State(core): State<MinimalCore>) -> Json<PlaylistStatus> {
    Json(core.get_status())
}

async fn add_tracks_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<AddTracksRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.add_tracks(req.paths))
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn remove_track_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<RemoveTrackRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    core.remove_track(req.index)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn clear_playlist_handler(State(core): State<MinimalCore>) -> Json<PlaylistStatus> {
    Json(core.clear_playlist())
}

async fn next_track_handler(
    State(core): State<MinimalCore>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.next_track())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn set_playback_mode_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<SetModeRequest>,
) -> Json<PlaylistStatus> {
    Json(core.set_playback_mode(req.mode))
}

async fn set_crossfade_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<SetCrossfadeRequest>,
) -> Json<PlaylistStatus> {
    Json(core.set_crossfade(req.crossfade_ms))
}

async fn assign_jingle_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<AssignJingleRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.assign_jingle(req.slot, req.path.as_deref()))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn play_jingle_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<PlayJingleRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.play_jingle(req.slot))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn play_cue_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<FilePathRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.play_cue(&req.path))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn seek_cue_deck_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<SeekCueDeckRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.seek_cue_deck(req.path.as_deref(), req.position_ms))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn get_cue_telemetry_handler(State(core): State<MinimalCore>) -> Json<CueTelemetry> {
    Json(core.get_cue_telemetry())
}

async fn stop_cue_handler(State(core): State<MinimalCore>) -> Json<PlaylistStatus> {
    Json(core.stop_cue())
}

async fn set_audio_routing_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<AudioRoutingRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.set_audio_routing(req.air_device, req.cue_device))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn refresh_devices_handler(
    State(core): State<MinimalCore>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.refresh_devices())
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn save_playlist_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<FilePathRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.save_playlist(&req.path))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn load_playlist_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<FilePathRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.load_playlist(&req.path))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn add_event_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<AddEventRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    let mode = req.interruption_mode.unwrap_or_else(|| {
        if req.wait_for_current.unwrap_or(false) {
            EventInterruptionMode::WaitForCurrent
        } else {
            EventInterruptionMode::Ducking
        }
    });
    tokio::task::spawn_blocking(move || {
        core.add_event_with_mode(req.time, req.days, req.path, mode)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map(Json)
    .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn remove_event_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<EventIdRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    core.remove_event(req.id)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn toggle_event_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<ToggleEventRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    core.toggle_event(req.id, req.enabled)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn trigger_event_now_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<EventIdRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.trigger_event_now(req.id))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn add_random_folder_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<FilePathRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.add_random_folder(&req.path))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn add_time_announcement_handler(State(core): State<MinimalCore>) -> Json<PlaylistStatus> {
    Json(core.add_time_announcement())
}

async fn get_folder_contents_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<FilePathRequest>,
) -> Result<Json<Vec<FolderAudioEntry>>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.get_folder_contents(&req.path))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn set_track_intro_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<SetTrackIntroRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.set_track_intro(&req.path, req.intro_ms))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn set_track_sweeper_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<SetTrackSweeperRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        core.set_track_sweeper(
            req.index,
            req.path.as_deref(),
            req.sweeper_path.as_deref(),
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map(Json)
    .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn set_track_markers_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<SetTrackMarkersRequest>,
) -> Result<Json<PlaylistStatus>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        core.set_track_markers(
            req.index,
            &req.path,
            req.intro_ms,
            req.sweeper_path.as_deref(),
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map(Json)
    .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn get_track_waveform_handler(
    State(core): State<MinimalCore>,
    Json(req): Json<GetTrackWaveformRequest>,
) -> Result<Json<WaveformEditorData>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || core.get_track_waveform(&req.path, req.buckets))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}
