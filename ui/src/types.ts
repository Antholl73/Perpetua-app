export type PlaybackMode = 'normal' | 'repeat' | 'random' | 'shuffle' | 'manual';
export type TransportStatus = 'playing' | 'paused' | 'stopped';
export type EventPriority = 'high' | 'low';
export type FadeCurve = 'linear_classic' | 'equal_power' | 'logarithmic';

export type TrackKind =
  | { type: 'audio_file'; path: string }
  | { type: 'random_directory'; dir_path: string }
  | { type: 'time_announcement' }
  | { type: 'temperature_announcement' }
  | { type: 'humidity_announcement' }
  | { type: 'pause'; seconds: number }
  | { type: 'satellite_connection'; seconds: number }
  | { type: 'satellite_connect_cmd' }
  | { type: 'satellite_disconnect_cmd' }
  | { type: 'dtmf_enable_cmd' }
  | { type: 'dtmf_disable_cmd' }
  | { type: 'stop_command' }
  | { type: 'play_command' }
  | { type: 'internet_stream'; url: string; seconds: number }
  | { type: 'rotation'; path: string }
  | { type: 'sequence'; path: string };

export interface TrackItem {
  id: string;
  duration_ms: number;
  kind: TrackKind;
  custom_overlap_sec: number | null;
  display_title: string;
  loudness_lufs: number | null;
}

export interface PlaylistRowView {
  index: number;
  item: TrackItem;
  estimated_air_time: string;
  is_playing: boolean;
  is_next: boolean;
}

export interface ScheduledEvent {
  id: string;
  immediate: boolean;
  day_hour_sec: number;
  day_mask: [boolean, boolean, boolean, boolean, boolean, boolean, boolean];
  track: TrackItem;
  start_date: string;
  expiration_date: string;
  expires: boolean;
  priority: EventPriority;
  hour_bits: number;
  enabled: boolean;
  max_wait_minutes: number;
}

export interface QueuedEvent {
  event: ScheduledEvent;
  triggered_at_epoch_sec: number;
  triggered_at_display: string;
}

export interface JingleSlot {
  index: number;
  title: string;
  file_path: string;
  loop_enabled: boolean;
  sweeper_ducking: boolean;
}

export interface JinglePalette {
  page_names: string[];
  slots: JingleSlot[];
  active_page: number;
}

export interface SweeperRecord {
  target_song_file: string;
  sweeper_file: string;
  intro_seconds: number;
  enabled: boolean;
}

export interface FaderSettings {
  fade_length_ms: number;
  fade_overlap_ms: number;
  sweeper_duck_volume_pct: number;
  silence_detector_enabled: boolean;
  silence_threshold_db: number;
  agc_compressor_enabled: boolean;
  target_lufs: number;
  curve: FadeCurve;
}

export interface AudioRoutingConfig {
  air_device: string;
  cue_device: string;
  sweeper_device: string;
  jingles_device: string;
  satellite_input_device: string;
  aux_devices: string[];
}

export interface WeatherTimeSettings {
  base_resources_dir: string;
  time_mode: 'twenty_four_hours' | 'twelve_hours';
  temperature_celsius: number;
  humidity_percent: number;
  weather_html_path: string | null;
}

export interface VuMeterTelemetry {
  left_db: number;
  right_db: number;
  peak_left_db: number;
  peak_right_db: number;
  agc_gain_reduction_db: number;
  integrated_lufs: number;
}

export interface AuditLogEntry {
  timestamp: string;
  level: string;
  message: string;
}

export interface StudioSnapshot {
  station_name: string;
  clock_display: string;
  date_display: string;
  transport: TransportStatus;
  mode: PlaybackMode;
  stop_after_current: boolean;
  events_enabled: boolean;
  sweepers_enabled: boolean;
  dtmf_enabled: boolean;
  satellite_connected: boolean;
  current_track: TrackItem | null;
  current_index: number | null;
  elapsed_ms: number;
  remaining_ms: number;
  active_sweeper: string | null;
  next_track: TrackItem | null;
  playlist: PlaylistRowView[];
  programmed_events: ScheduledEvent[];
  immediate_queue: QueuedEvent[];
  delayed_queue: QueuedEvent[];
  jingles: JinglePalette;
  sweepers: SweeperRecord[];
  fader_settings: FaderSettings;
  routing_config: AudioRoutingConfig;
  weather_time: WeatherTimeSettings;
  vu_telemetry: VuMeterTelemetry;
  recent_logs: AuditLogEntry[];
}
