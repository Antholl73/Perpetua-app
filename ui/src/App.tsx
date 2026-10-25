import React, { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';

export type PlaybackMode = 'auto' | 'manual' | 'loop' | 'shuffle';

export type TrackKind = 'file' | 'random_folder' | 'time_announcement';

export type EventInterruptionMode = 'ducking' | 'pause_resume' | 'wait_for_current';

export interface FolderAudioEntry {
  path: string;
  title: string;
  extension: string;
  size_bytes: number;
  in_recent_history: boolean;
  intro_ms?: number;
  sweeper_path?: string | null;
  sweeper_title?: string | null;
  sweeper_duration_ms?: number;
}

export interface TrackItem {
  id: number;
  path: string;
  title: string;
  duration_ms: number;
  sample_rate?: number;
  kind?: TrackKind;
  last_resolved_file?: string | null;
  intro_ms?: number;
  sweeper_path?: string | null;
  sweeper_title?: string | null;
  sweeper_duration_ms?: number;
  sweeper_trigger_ms?: number | null;
}

export interface WaveformEditorData {
  path: string;
  title: string;
  duration_ms: number;
  sample_rate: number;
  peaks: number[];
  intro_ms: number;
  sweeper_path: string | null;
  sweeper_title: string | null;
  sweeper_duration_ms: number;
  sweeper_trigger_ms: number | null;
}

export interface CueTelemetry {
  is_playing: boolean;
  current_position_ms: number;
  duration_ms: number;
  cue_file: string | null;
}

export interface JingleSlot {
  slot: number;
  path: string | null;
  title: string | null;
  duration_ms: number;
  is_playing: boolean;
}

export interface ScheduledEvent {
  id: number;
  time: string;
  days: number[];
  path: string;
  title: string;
  wait_for_current: boolean;
  interruption_mode?: EventInterruptionMode;
  enabled: boolean;
}

export interface NextEventInfo {
  event_id: number;
  title: string;
  time: string;
  fires_at_epoch: number;
  fires_at_display: string;
  seconds_until: number;
  wait_for_current: boolean;
  interruption_mode?: EventInterruptionMode;
}

export interface EventItem {
  event_id: number;
  title: string;
  path: string;
  wait_for_current: boolean;
  no_crossfade?: boolean;
}

export interface PlaylistStatus {
  is_playing: boolean;
  is_crossfading: boolean;
  is_ducking?: boolean;
  is_paused_by_event?: boolean;
  overlay_is_playing?: boolean;
  overlay_file?: string | null;
  sweeper_armed?: boolean;
  sweeper_is_playing?: boolean;
  sweeper_file?: string | null;
  sweeper_intro_ms?: number;
  sweeper_trigger_ms?: number;
  sweeper_duration_ms?: number;
  current_file: string | null;
  elapsed_ms: number;
  duration_ms: number;
  remaining_ms: number;
  playback_mode: PlaybackMode;
  crossfade_ms: number;
  device_name: string;
  sample_rate: number;
  channels: number;
  decoded_frames: number;
  last_error: string | null;
  active_jingle_slots?: number[];
  cue_is_playing: boolean;
  cue_file: string | null;
  cue_elapsed_ms: number;
  cue_duration_ms: number;
  cue_device_name: string;
  air_device_selected: string;
  cue_device_selected: string;
  available_devices: string[];
  playlist: TrackItem[];
  current_index: number | null;
  jingles: JingleSlot[];
  scheduled_events: ScheduledEvent[];
  next_event: NextEventInfo | null;
  pending_events: EventItem[];
  on_air_event: EventItem | null;
  last_event_message: string | null;
  clock: string;
  recent_played_history?: string[];
  horas_dir?: string;
}

type IpcCommand =
  | 'play'
  | 'stop'
  | 'get_status'
  | 'add_tracks'
  | 'remove_track'
  | 'clear_playlist'
  | 'next_track'
  | 'set_playback_mode'
  | 'set_crossfade'
  | 'assign_jingle'
  | 'play_jingle'
  | 'play_cue'
  | 'seek_cue_deck'
  | 'stop_cue'
  | 'set_audio_routing'
  | 'refresh_devices'
  | 'save_playlist'
  | 'load_playlist'
  | 'add_event'
  | 'remove_event'
  | 'toggle_event'
  | 'trigger_event_now'
  | 'add_random_folder'
  | 'add_time_announcement'
  | 'set_track_intro'
  | 'set_track_sweeper'
  | 'set_track_markers';

interface ContextMenuState {
  x: number;
  y: number;
  type: 'track' | 'jingle';
  trackIndex?: number;
  track?: TrackItem;
  jingle?: JingleSlot;
}

declare global {
  interface Window {
    __TAURI__?: {
      core?: {
        invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
      };
    };
    __TAURI_INTERNALS__?: {
      invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
    };
  }
}

async function invokeCommand(
  cmd: IpcCommand,
  args?: Record<string, unknown>,
): Promise<PlaylistStatus> {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke<PlaylistStatus>(cmd, args);
  }
  if (window.__TAURI_INTERNALS__?.invoke) {
    return window.__TAURI_INTERNALS__.invoke<PlaylistStatus>(cmd, args);
  }

  const endpointMap: Record<IpcCommand, { url: string; method: 'GET' | 'POST' }> = {
    play: { url: 'http://127.0.0.1:4850/api/play', method: 'POST' },
    stop: { url: 'http://127.0.0.1:4850/api/stop', method: 'POST' },
    get_status: { url: 'http://127.0.0.1:4850/api/status', method: 'GET' },
    add_tracks: { url: 'http://127.0.0.1:4850/api/add_tracks', method: 'POST' },
    remove_track: { url: 'http://127.0.0.1:4850/api/remove_track', method: 'POST' },
    clear_playlist: { url: 'http://127.0.0.1:4850/api/clear_playlist', method: 'POST' },
    next_track: { url: 'http://127.0.0.1:4850/api/next_track', method: 'POST' },
    set_playback_mode: { url: 'http://127.0.0.1:4850/api/set_playback_mode', method: 'POST' },
    set_crossfade: { url: 'http://127.0.0.1:4850/api/set_crossfade', method: 'POST' },
    assign_jingle: { url: 'http://127.0.0.1:4850/api/assign_jingle', method: 'POST' },
    play_jingle: { url: 'http://127.0.0.1:4850/api/play_jingle', method: 'POST' },
    play_cue: { url: 'http://127.0.0.1:4850/api/play_cue', method: 'POST' },
    seek_cue_deck: { url: 'http://127.0.0.1:4850/api/seek_cue_deck', method: 'POST' },
    stop_cue: { url: 'http://127.0.0.1:4850/api/stop_cue', method: 'POST' },
    set_audio_routing: { url: 'http://127.0.0.1:4850/api/set_audio_routing', method: 'POST' },
    refresh_devices: { url: 'http://127.0.0.1:4850/api/refresh_devices', method: 'POST' },
    save_playlist: { url: 'http://127.0.0.1:4850/api/save_playlist', method: 'POST' },
    load_playlist: { url: 'http://127.0.0.1:4850/api/load_playlist', method: 'POST' },
    add_event: { url: 'http://127.0.0.1:4850/api/add_event', method: 'POST' },
    remove_event: { url: 'http://127.0.0.1:4850/api/remove_event', method: 'POST' },
    toggle_event: { url: 'http://127.0.0.1:4850/api/toggle_event', method: 'POST' },
    trigger_event_now: { url: 'http://127.0.0.1:4850/api/trigger_event_now', method: 'POST' },
    add_random_folder: { url: 'http://127.0.0.1:4850/api/add_random_folder', method: 'POST' },
    add_time_announcement: { url: 'http://127.0.0.1:4850/api/add_time_announcement', method: 'POST' },
    set_track_intro: { url: 'http://127.0.0.1:4850/api/set_track_intro', method: 'POST' },
    set_track_sweeper: { url: 'http://127.0.0.1:4850/api/set_track_sweeper', method: 'POST' },
    set_track_markers: { url: 'http://127.0.0.1:4850/api/set_track_markers', method: 'POST' },
  };

  const cfg = endpointMap[cmd];
  const res = await fetch(cfg.url, {
    method: cfg.method,
    headers: cfg.method === 'POST' ? { 'Content-Type': 'application/json' } : undefined,
    body: cfg.method === 'POST' ? JSON.stringify(args ?? {}) : undefined,
  });

  if (!res.ok) {
    throw new Error(await res.text());
  }
  return res.json();
}

async function invokeGetCueTelemetry(): Promise<CueTelemetry> {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke<CueTelemetry>('get_cue_telemetry');
  }
  if (window.__TAURI_INTERNALS__?.invoke) {
    return window.__TAURI_INTERNALS__.invoke<CueTelemetry>('get_cue_telemetry');
  }
  const res = await fetch('http://127.0.0.1:4850/api/cue_telemetry', {
    method: 'GET',
  });
  if (!res.ok) {
    throw new Error(await res.text());
  }
  return res.json();
}

async function invokeGetFolderContents(path: string): Promise<FolderAudioEntry[]> {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke<FolderAudioEntry[]>('get_folder_contents', { path });
  }
  if (window.__TAURI_INTERNALS__?.invoke) {
    return window.__TAURI_INTERNALS__.invoke<FolderAudioEntry[]>('get_folder_contents', { path });
  }
  const res = await fetch('http://127.0.0.1:4850/api/get_folder_contents', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path }),
  });
  if (!res.ok) {
    throw new Error(await res.text());
  }
  return res.json();
}

async function invokeGetTrackWaveform(
  path: string,
  buckets = 240,
): Promise<WaveformEditorData> {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke<WaveformEditorData>('get_track_waveform', {
      path,
      buckets,
    });
  }
  if (window.__TAURI_INTERNALS__?.invoke) {
    return window.__TAURI_INTERNALS__.invoke<WaveformEditorData>('get_track_waveform', {
      path,
      buckets,
    });
  }
  const res = await fetch('http://127.0.0.1:4850/api/get_track_waveform', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path, buckets }),
  });
  if (!res.ok) {
    throw new Error(await res.text());
  }
  return res.json();
}

function formatEventInterruptionLabel(
  mode?: EventInterruptionMode,
  waitForCurrent?: boolean,
): string {
  const effective: EventInterruptionMode =
    mode ?? (waitForCurrent ? 'wait_for_current' : 'ducking');
  switch (effective) {
    case 'pause_resume':
      return '⏸ Pausa y Reanuda';
    case 'wait_for_current':
      return '⏳ Espera al Final';
    case 'ducking':
    default:
      return '🔉 Ducking -70%';
  }
}

function formatBytesKB(bytes: number): string {
  if (!bytes || bytes <= 0) return '';
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

const DEFAULT_TEST_WAV = 'crates/perpetua-audio/test_tone.wav';

const INITIAL_JINGLES: JingleSlot[] = Array.from({ length: 9 }, (_, idx) => ({
  slot: idx + 1,
  path: null,
  title: null,
  duration_ms: 0,
  is_playing: false,
}));

const WEEKDAYS: Array<{ day: number; short: string; full: string }> = [
  { day: 0, short: 'Lun', full: 'Lunes' },
  { day: 1, short: 'Mar', full: 'Martes' },
  { day: 2, short: 'Mié', full: 'Miércoles' },
  { day: 3, short: 'Jue', full: 'Jueves' },
  { day: 4, short: 'Vie', full: 'Viernes' },
  { day: 5, short: 'Sáb', full: 'Sábado' },
  { day: 6, short: 'Dom', full: 'Domingo' },
];

function formatDaysList(days: number[]): string {
  if (!days || days.length === 0) return 'Ninguno';
  if (days.length === 7) return 'Todos los días';
  const sorted = [...days].sort((a, b) => a - b);
  if (
    sorted.length === 5 &&
    sorted[0] === 0 &&
    sorted[4] === 4
  ) {
    return 'Lun–Vie';
  }
  if (sorted.length === 2 && sorted[0] === 5 && sorted[1] === 6) {
    return 'Sáb–Dom';
  }
  return sorted
    .map((d) => WEEKDAYS.find((w) => w.day === d)?.short ?? String(d))
    .join(', ');
}

function formatSecondsUntil(sec: number): string {
  if (!Number.isFinite(sec) || sec <= 0) return 'ahora';
  const hours = Math.floor(sec / 3600);
  const mins = Math.floor((sec % 3600) / 60);
  const secs = sec % 60;
  if (hours > 0) {
    return `${hours}h ${String(mins).padStart(2, '0')}m ${String(secs).padStart(2, '0')}s`;
  }
  if (mins > 0) {
    return `${String(mins).padStart(2, '0')}m ${String(secs).padStart(2, '0')}s`;
  }
  return `${secs}s`;
}

function getNextMinuteHHMM(): string {
  const d = new Date(Date.now() + 60_000);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

function formatTimeMs(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) {
    return '00:00.0';
  }
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  const tenths = Math.floor((ms % 1000) / 100);
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}.${tenths}`;
}

function formatCountdownMMSS(remainingMs: number): string {
  if (!Number.isFinite(remainingMs) || remainingMs <= 0) {
    return '-00:00';
  }
  const totalSeconds = Math.ceil(remainingMs / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `-${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
}

const PLAYBACK_MODES: Array<{ id: PlaybackMode; label: string; desc: string }> = [
  {
    id: 'auto',
    label: '▶ Continuo / Auto',
    desc: 'Reproduce toda la lista en orden hasta el final',
  },
  {
    id: 'manual',
    label: '✋ Manual',
    desc: 'Se detiene al terminar la pista actual y espera Play',
  },
  {
    id: 'loop',
    label: '🔁 Loop (Repetir)',
    desc: 'Repite toda la lista de reproducción continuamente',
  },
  {
    id: 'shuffle',
    label: '🔀 Shuffle (Aleatorio)',
    desc: 'Selecciona la siguiente canción al azar',
  },
];

export function App() {
  const [status, setStatus] = useState<PlaylistStatus>({
    is_playing: false,
    is_crossfading: false,
    current_file: null,
    elapsed_ms: 0,
    duration_ms: 0,
    remaining_ms: 0,
    playback_mode: 'auto',
    crossfade_ms: 2000,
    device_name: 'Default Output',
    sample_rate: 0,
    channels: 0,
    decoded_frames: 0,
    last_error: null,
    cue_is_playing: false,
    cue_file: null,
    cue_elapsed_ms: 0,
    cue_duration_ms: 0,
    cue_device_name: 'Default Output',
    air_device_selected: 'Default Output',
    cue_device_selected: 'Default Output',
    available_devices: ['Default Output'],
    playlist: [],
    current_index: null,
    jingles: INITIAL_JINGLES,
    scheduled_events: [],
    next_event: null,
    pending_events: [],
    on_air_event: null,
    last_event_message: null,
    clock: '00:00:00',
  });
  const [uiError, setUiError] = useState<string | null>(null);
  const [uiNotice, setUiNotice] = useState<string | null>(null);
  const [flashedSlot, setFlashedSlot] = useState<number | null>(null);

  // Modal de Configuración de Ruteo de Audio
  const [showRoutingModal, setShowRoutingModal] = useState<boolean>(false);
  const [selectedAirDevice, setSelectedAirDevice] = useState<string>('Default Output');
  const [selectedCueDevice, setSelectedCueDevice] = useState<string>('Default Output');

  // Hito 6: Modal / Panel del Planificador de Eventos (Event Scheduler)
  const [showEventsModal, setShowEventsModal] = useState<boolean>(false);
  const [showNewEventForm, setShowNewEventForm] = useState<boolean>(false);
  const [newEventTime, setNewEventTime] = useState<string>(() => getNextMinuteHHMM());
  const [newEventDays, setNewEventDays] = useState<number[]>([0, 1, 2, 3, 4, 5, 6]);
  const [newEventPath, setNewEventPath] = useState<string>('');
  const [newEventInterruptionMode, setNewEventInterruptionMode] =
    useState<EventInterruptionMode>('ducking');

  // Hito 7 Refinamiento: Carpetas Desplegables en la UI (solo visualización, sin expandir el slot en Rust)
  const [expandedFolders, setExpandedFolders] = useState<Record<number, boolean>>({});
  const [folderContentsByTrackId, setFolderContentsByTrackId] = useState<
    Record<number, FolderAudioEntry[]>
  >({});
  const [loadingFolderTrackId, setLoadingFolderTrackId] = useState<number | null>(null);

  // Hito 8: Editor Visual de Puntos de Mezcla (Forma de Onda + Marcador de Intro + Pisador + Playhead Cue)
  const [showMarkerModal, setShowMarkerModal] = useState<boolean>(false);
  const [markerTargetIndex, setMarkerTargetIndex] = useState<number | null>(null);
  const [markerData, setMarkerData] = useState<WaveformEditorData | null>(null);
  const [markerIntroMs, setMarkerIntroMs] = useState<number>(0);
  const [markerSweeperPath, setMarkerSweeperPath] = useState<string | null>(null);
  const [markerSweeperTitle, setMarkerSweeperTitle] = useState<string | null>(null);
  const [markerSweeperDurationMs, setMarkerSweeperDurationMs] = useState<number>(0);
  const [isLoadingWaveform, setIsLoadingWaveform] = useState<boolean>(false);
  const [isDraggingMarker, setIsDraggingMarker] = useState<boolean>(false);
  const [isScrubbingPlayhead, setIsScrubbingPlayhead] = useState<boolean>(false);
  const [cuePlayheadMs, setCuePlayheadMs] = useState<number>(0);
  const [cueModalIsPlaying, setCueModalIsPlaying] = useState<boolean>(false);
  const waveformCanvasRef = React.useRef<HTMLCanvasElement | null>(null);
  const cueTelemetryRef = React.useRef<{
    is_playing: boolean;
    current_position_ms: number;
    duration_ms: number;
    cue_file: string | null;
    syncedAtPerfMs: number;
  }>({
    is_playing: false,
    current_position_ms: 0,
    duration_ms: 0,
    cue_file: null,
    syncedAtPerfMs: performance.now(),
  });

  // Menú contextual (clic derecho en pista o Jingle)
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);

  async function refreshStatus() {
    try {
      const st = await invokeCommand('get_status');
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  useEffect(() => {
    refreshStatus();
    const timer = window.setInterval(refreshStatus, 100);
    return () => window.clearInterval(timer);
  }, []);

  // 1. Telemetría en Tiempo Real del Cue Deck (cada ~20ms vía evento de Tauri 'cue-telemetry' + fallback HTTP)
  useEffect(() => {
    let unlistenFn: (() => void) | null = null;
    let cancelled = false;

    const applyTelemetry = (tel: CueTelemetry) => {
      if (cancelled) return;
      cueTelemetryRef.current = {
        is_playing: tel.is_playing,
        current_position_ms: tel.current_position_ms,
        duration_ms: tel.duration_ms,
        cue_file: tel.cue_file,
        syncedAtPerfMs: performance.now(),
      };
      setCuePlayheadMs(tel.current_position_ms);
      setCueModalIsPlaying(tel.is_playing);
    };

    if (window.__TAURI_INTERNALS__ || window.__TAURI__) {
      listen<CueTelemetry>('cue-telemetry', (ev) => {
        if (ev?.payload) {
          applyTelemetry(ev.payload);
        }
      })
        .then((unlisten) => {
          if (cancelled) {
            unlisten();
          } else {
            unlistenFn = unlisten;
          }
        })
        .catch(() => {
          // Fallback silencioso si el bridge de eventos no está disponible
        });
    }

    return () => {
      cancelled = true;
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, []);

  // Sincronización adicional de telemetría cuando se abre el modal en modo navegador HTTP o cambia status
  useEffect(() => {
    if (!showMarkerModal) return;
    if (window.__TAURI_INTERNALS__ || window.__TAURI__) {
      invokeGetCueTelemetry()
        .then((tel) => {
          cueTelemetryRef.current = {
            ...tel,
            syncedAtPerfMs: performance.now(),
          };
          setCuePlayheadMs(tel.current_position_ms);
          setCueModalIsPlaying(tel.is_playing);
        })
        .catch(() => {});
      return;
    }

    const pollTimer = window.setInterval(() => {
      invokeGetCueTelemetry()
        .then((tel) => {
          cueTelemetryRef.current = {
            ...tel,
            syncedAtPerfMs: performance.now(),
          };
          setCuePlayheadMs(tel.current_position_ms);
          setCueModalIsPlaying(tel.is_playing);
        })
        .catch(() => {});
    }, 25);

    return () => window.clearInterval(pollTimer);
  }, [showMarkerModal]);

  // Cerrar el menú contextual al hacer clic en cualquier parte o pulsar Escape
  useEffect(() => {
    function closeMenu() {
      setContextMenu(null);
    }
    function onEsc(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        setContextMenu(null);
        setShowRoutingModal(false);
        setShowEventsModal(false);
        setShowMarkerModal(false);
      }
    }
    window.addEventListener('click', closeMenu);
    window.addEventListener('keydown', onEsc);
    return () => {
      window.removeEventListener('click', closeMenu);
      window.removeEventListener('keydown', onEsc);
    };
  }, []);

  // Atajos de Teclado Globales (Teclas 1 al 9 para Jingles)
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.tagName === 'SELECT' ||
          target.isContentEditable)
      ) {
        return;
      }
      if (e.ctrlKey || e.altKey || e.metaKey || e.repeat) {
        return;
      }

      if (e.key >= '1' && e.key <= '9') {
        e.preventDefault();
        const slotNum = Number(e.key);
        setFlashedSlot(slotNum);
        window.setTimeout(() => setFlashedSlot(null), 220);

        invokeCommand('play_jingle', { slot: slotNum })
          .then((st) => {
            setUiError(null);
            setStatus(st);
          })
          .catch((err) => {
            setUiError(String(err));
          });
      }
    }

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  function showBriefNotice(msg: string) {
    setUiNotice(msg);
    window.setTimeout(() => {
      setUiNotice((curr) => (curr === msg ? null : curr));
    }, 3500);
  }

  async function handleSelectFiles() {
    setUiError(null);
    try {
      const selected = await open({
        multiple: true,
        title: 'Seleccionar archivos de audio para la lista de reproducción',
        filters: [
          {
            name: 'Archivos de Audio (.wav, .mp3, .flac, .ogg)',
            extensions: ['wav', 'mp3', 'flac', 'ogg'],
          },
        ],
      });

      if (!selected) {
        return;
      }

      const paths = Array.isArray(selected) ? selected : [selected];
      if (paths.length > 0) {
        const st = await invokeCommand('add_tracks', { paths });
        setStatus(st);
      }
    } catch (err) {
      setUiError(String(err));
    }
  }

  /**
   * Hito 7 (1 & 4): Añadir una Carpeta Aleatoria (Random Folder) desde el explorador
   */
  async function handleSelectRandomFolder() {
    setUiError(null);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Seleccionar Carpeta de Música Aleatoria (Random Folder - Anti-Repeat 50)',
      });

      if (!selected) {
        return;
      }

      const folderPath = Array.isArray(selected) ? selected[0] : selected;
      if (folderPath) {
        const st = await invokeCommand('add_random_folder', { path: folderPath });
        setStatus(st);
        showBriefNotice(`Carpeta Aleatoria añadida: ${folderPath}`);
      }
    } catch (err) {
      setUiError(String(err));
    }
  }

  /**
   * Hito 7 Refinamiento (1): Desplegar / colapsar visualmente los archivos de una Carpeta Aleatoria
   * usando get_folder_contents(path) sin cargar audio en memoria.
   */
  async function handleToggleFolderExpand(track: TrackItem, e?: React.MouseEvent) {
    if (e) {
      e.stopPropagation();
    }
    const currentlyOpen = Boolean(expandedFolders[track.id]);
    if (currentlyOpen) {
      setExpandedFolders((prev) => ({ ...prev, [track.id]: false }));
      return;
    }

    setExpandedFolders((prev) => ({ ...prev, [track.id]: true }));
    setLoadingFolderTrackId(track.id);
    try {
      const entries = await invokeGetFolderContents(track.path);
      setFolderContentsByTrackId((prev) => ({ ...prev, [track.id]: entries }));
    } catch (err) {
      setUiError(String(err));
    } finally {
      setLoadingFolderTrackId((curr) => (curr === track.id ? null : curr));
    }
  }

  /**
   * Hito 7 (3 & 4): Añadir Comando Virtual de Hora (.time) a la lista de reproducción
   */
  async function handleAddTimeAnnouncement() {
    setUiError(null);
    try {
      const st = await invokeCommand('add_time_announcement');
      setStatus(st);
      showBriefNotice('Comando virtual de Locución de Hora (.time) añadido a la lista');
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleAddTestTone() {
    setUiError(null);
    try {
      const st = await invokeCommand('add_tracks', { paths: [DEFAULT_TEST_WAV] });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  /**
   * 3. Guardar Lista de Reproducción (.m3u8 o .json) con diálogo nativo
   */
  async function handleSavePlaylist() {
    setUiError(null);
    try {
      const targetPath = await save({
        title: 'Guardar Lista de Reproducción Perpetua',
        defaultPath: 'programacion_perpetua.m3u8',
        filters: [
          {
            name: 'Lista M3U8 Estándar (.m3u8)',
            extensions: ['m3u8', 'm3u'],
          },
          {
            name: 'Lista Estructurada Perpetua (.json)',
            extensions: ['json'],
          },
        ],
      });

      if (!targetPath) {
        return;
      }

      const st = await invokeCommand('save_playlist', { path: targetPath });
      setStatus(st);
      showBriefNotice(`Lista guardada en: ${targetPath}`);
    } catch (err) {
      setUiError(String(err));
    }
  }

  /**
   * 3. Abrir / Cargar Lista de Reproducción (.m3u8, .m3u o .json) con diálogo nativo
   */
  async function handleLoadPlaylist() {
    setUiError(null);
    try {
      const selected = await open({
        multiple: false,
        title: 'Abrir Lista de Reproducción (.m3u8, .m3u, .json)',
        filters: [
          {
            name: 'Listas de Reproducción (.m3u8, .m3u, .json)',
            extensions: ['m3u8', 'm3u', 'json'],
          },
        ],
      });

      if (!selected) {
        return;
      }

      const filePath = Array.isArray(selected) ? selected[0] : selected;
      if (filePath) {
        const st = await invokeCommand('load_playlist', { path: filePath });
        setStatus(st);
        showBriefNotice(`Lista cargada: ${filePath.split('/').pop()}`);
      }
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handlePlay() {
    setUiError(null);
    try {
      const st = await invokeCommand('play', {});
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handlePlayTrack(index: number) {
    setUiError(null);
    try {
      const st = await invokeCommand('play', { index });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleStop() {
    setUiError(null);
    try {
      const st = await invokeCommand('stop');
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleNextTrack() {
    setUiError(null);
    try {
      const st = await invokeCommand('next_track');
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleRemoveTrack(index: number, e?: React.MouseEvent) {
    if (e) {
      e.stopPropagation();
    }
    setUiError(null);
    try {
      const st = await invokeCommand('remove_track', { index });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleClearPlaylist() {
    setUiError(null);
    try {
      const st = await invokeCommand('clear_playlist');
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleModeChange(mode: PlaybackMode) {
    setUiError(null);
    try {
      const st = await invokeCommand('set_playback_mode', { mode });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleCrossfadeChange(crossfadeMs: number) {
    setUiError(null);
    try {
      const st = await invokeCommand('set_crossfade', {
        crossfadeMs,
        crossfade_ms: crossfadeMs,
      });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  /**
   * 2. Reproductor de Preescucha (Cue Player)
   */
  async function handlePlayCuePath(path: string) {
    setUiError(null);
    try {
      const st = await invokeCommand('play_cue', { path });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handlePickAndPlayCueFile() {
    setUiError(null);
    try {
      const selected = await open({
        multiple: false,
        title: 'Seleccionar archivo de audio para Preescucha (Cue)',
        filters: [
          {
            name: 'Archivos de Audio (.wav, .mp3, .flac, .ogg)',
            extensions: ['wav', 'mp3', 'flac', 'ogg'],
          },
        ],
      });
      if (!selected) {
        return;
      }
      const filePath = Array.isArray(selected) ? selected[0] : selected;
      if (filePath) {
        await handlePlayCuePath(filePath);
      }
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleStopCue() {
    setUiError(null);
    try {
      const st = await invokeCommand('stop_cue');
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  /**
   * 1. Modal de Configuración de Ruteo de Audio (Main / Aire y Cue / Preescucha)
   */
  async function handleOpenRoutingModal() {
    setUiError(null);
    try {
      const st = await invokeCommand('refresh_devices');
      setStatus(st);
      setSelectedAirDevice(st.air_device_selected || 'Default Output');
      setSelectedCueDevice(st.cue_device_selected || 'Default Output');
      setShowRoutingModal(true);
    } catch (err) {
      setUiError(String(err));
      setShowRoutingModal(true);
    }
  }

  async function handleApplyAudioRouting() {
    setUiError(null);
    try {
      const st = await invokeCommand('set_audio_routing', {
        airDevice: selectedAirDevice,
        cueDevice: selectedCueDevice,
        air_device: selectedAirDevice,
        cue_device: selectedCueDevice,
      });
      setStatus(st);
      setShowRoutingModal(false);
      showBriefNotice(
        `Ruteo actualizado — Aire: ${selectedAirDevice} | Cue: ${selectedCueDevice}`,
      );
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleAssignJingleFile(slot: number, e?: React.MouseEvent) {
    if (e) {
      e.preventDefault();
      e.stopPropagation();
    }
    setUiError(null);
    try {
      const selected = await open({
        multiple: false,
        title: `Asignar archivo de audio al Jingle #${slot} (Tecla ${slot})`,
        filters: [
          {
            name: 'Archivos de Audio (.wav, .mp3, .flac, .ogg)',
            extensions: ['wav', 'mp3', 'flac', 'ogg'],
          },
        ],
      });

      if (!selected) {
        return;
      }

      const filePath = Array.isArray(selected) ? selected[0] : selected;
      if (filePath) {
        const st = await invokeCommand('assign_jingle', {
          slot,
          path: filePath,
        });
        setStatus(st);
      }
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleClearJingleSlot(slot: number, e?: React.MouseEvent) {
    if (e) {
      e.preventDefault();
      e.stopPropagation();
    }
    setUiError(null);
    try {
      const st = await invokeCommand('assign_jingle', { slot, path: null });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleAssignTestToneToSlot1() {
    setUiError(null);
    try {
      const st = await invokeCommand('assign_jingle', {
        slot: 1,
        path: DEFAULT_TEST_WAV,
      });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleTriggerJingle(jingle: JingleSlot) {
    if (!jingle.path) {
      await handleAssignJingleFile(jingle.slot);
      return;
    }
    setUiError(null);
    setFlashedSlot(jingle.slot);
    window.setTimeout(() => setFlashedSlot(null), 200);
    try {
      const st = await invokeCommand('play_jingle', { slot: jingle.slot });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  function handleTrackContextMenu(
    e: React.MouseEvent,
    track: TrackItem,
    trackIndex: number,
  ) {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      type: 'track',
      track,
      trackIndex,
    });
  }

  function handleJingleContextMenu(e: React.MouseEvent, jingle: JingleSlot) {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      type: 'jingle',
      jingle,
    });
  }

  /**
   * Hito 6: Handlers del Planificador de Eventos (Event Scheduler)
   */
  function handleToggleEventDay(day: number) {
    setNewEventDays((prev) =>
      prev.includes(day)
        ? prev.filter((d) => d !== day)
        : [...prev, day].sort((a, b) => a - b),
    );
  }

  async function handlePickEventFile() {
    setUiError(null);
    try {
      const selected = await open({
        multiple: false,
        title: 'Seleccionar Archivo de Audio o Lista para el Evento Programado',
        filters: [
          {
            name: 'Audio o Lista (.wav, .mp3, .flac, .ogg, .m3u8, .json)',
            extensions: ['wav', 'mp3', 'flac', 'ogg', 'm3u8', 'm3u', 'json'],
          },
        ],
      });
      if (!selected) {
        return;
      }
      const filePath = Array.isArray(selected) ? selected[0] : selected;
      if (filePath) {
        setNewEventPath(filePath);
      }
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleCreateEvent(e: React.FormEvent) {
    e.preventDefault();
    setUiError(null);
    if (!newEventPath.trim()) {
      setUiError('Selecciona un archivo de audio, carpeta, lista o .time para el evento.');
      return;
    }
    if (newEventDays.length === 0) {
      setUiError('Selecciona al menos un día de la semana para el evento.');
      return;
    }
    const waitFlag = newEventInterruptionMode === 'wait_for_current';
    try {
      const st = await invokeCommand('add_event', {
        time: newEventTime,
        days: newEventDays,
        path: newEventPath.trim(),
        waitForCurrent: waitFlag,
        wait_for_current: waitFlag,
        interruptionMode: newEventInterruptionMode,
        interruption_mode: newEventInterruptionMode,
      });
      setStatus(st);
      setShowNewEventForm(false);
      showBriefNotice(
        `Evento programado a las ${newEventTime}:00 (${formatEventInterruptionLabel(
          newEventInterruptionMode,
        )})`,
      );
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleRemoveEvent(id: number) {
    setUiError(null);
    try {
      const st = await invokeCommand('remove_event', { id });
      setStatus(st);
      showBriefNotice(`Evento #${id} eliminado`);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleToggleEventEnabled(id: number, enabled: boolean) {
    setUiError(null);
    try {
      const st = await invokeCommand('toggle_event', { id, enabled });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleTriggerEventNow(id: number) {
    setUiError(null);
    try {
      const st = await invokeCommand('trigger_event_now', { id });
      setStatus(st);
      showBriefNotice(`Evento #${id} disparado hacia el Main Deck`);
    } catch (err) {
      setUiError(String(err));
    }
  }

  /**
   * Hito 8 (3 & 4): Editor Visual de Puntos de Mezcla (Forma de Onda + Intro + Pisadores)
   */
  async function handleOpenMarkerEditor(
    filePath: string,
    playlistIndex: number | null = null,
  ) {
    setUiError(null);
    setShowMarkerModal(true);
    setMarkerTargetIndex(playlistIndex);
    setIsLoadingWaveform(true);
    if (status.cue_file === filePath) {
      setCuePlayheadMs(status.cue_elapsed_ms || 0);
      setCueModalIsPlaying(Boolean(status.cue_is_playing));
    } else {
      setCuePlayheadMs(0);
      setCueModalIsPlaying(false);
    }
    try {
      const wf = await invokeGetTrackWaveform(filePath, 240);
      setMarkerData(wf);
      setMarkerIntroMs(wf.intro_ms ?? 0);
      setMarkerSweeperPath(wf.sweeper_path ?? null);
      setMarkerSweeperTitle(wf.sweeper_title ?? null);
      setMarkerSweeperDurationMs(wf.sweeper_duration_ms ?? 0);
    } catch (err) {
      setUiError(String(err));
    } finally {
      setIsLoadingWaveform(false);
    }
  }

  async function handleAssignTrackSweeperQuick(
    index: number | null,
    trackPath: string,
    sweeperPath: string | null,
  ) {
    setUiError(null);
    try {
      const st = await invokeCommand('set_track_sweeper', {
        index: index ?? undefined,
        path: trackPath,
        sweeperPath,
        sweeper_path: sweeperPath,
      });
      setStatus(st);
      if (sweeperPath) {
        showBriefNotice(
          `Pisador asignado: ${sweeperPath.split('/').pop()} → ${trackPath.split('/').pop()}`,
        );
      } else {
        showBriefNotice(`Pisador quitado de ${trackPath.split('/').pop()}`);
      }
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleAssignTrackSweeperFromFile(
    index: number | null,
    trackPath: string,
  ) {
    setUiError(null);
    try {
      const selected = await open({
        multiple: false,
        title: `Seleccionar audio de Pisador (Sweeper) para ${trackPath.split('/').pop()}`,
        filters: [
          {
            name: 'Archivos de Audio (.wav, .mp3, .flac, .ogg)',
            extensions: ['wav', 'mp3', 'flac', 'ogg'],
          },
        ],
      });
      if (!selected) {
        return;
      }
      const sweeperFile = Array.isArray(selected) ? selected[0] : selected;
      if (sweeperFile) {
        await handleAssignTrackSweeperQuick(index, trackPath, sweeperFile);
      }
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handlePickSweeperInModal() {
    setUiError(null);
    try {
      const selected = await open({
        multiple: false,
        title: 'Seleccionar archivo de Pisador (Sweeper) para el Intro',
        filters: [
          {
            name: 'Archivos de Audio (.wav, .mp3, .flac, .ogg)',
            extensions: ['wav', 'mp3', 'flac', 'ogg'],
          },
        ],
      });
      if (!selected) {
        return;
      }
      const sweeperFile = Array.isArray(selected) ? selected[0] : selected;
      if (sweeperFile) {
        setMarkerSweeperPath(sweeperFile);
        setMarkerSweeperTitle(sweeperFile.split('/').pop() ?? sweeperFile);
        try {
          const swWf = await invokeGetTrackWaveform(sweeperFile, 32);
          setMarkerSweeperDurationMs(swWf.duration_ms);
        } catch {
          // Si falla la inspección rápida, se resolverá al guardar
        }
      }
    } catch (err) {
      setUiError(String(err));
    }
  }

  async function handleSelectJingleAsSweeperInModal(jingle: JingleSlot) {
    if (!jingle.path) return;
    setMarkerSweeperPath(jingle.path);
    setMarkerSweeperTitle(
      jingle.title ?? jingle.path.split('/').pop() ?? `Jingle #${jingle.slot}`,
    );
    setMarkerSweeperDurationMs(jingle.duration_ms || 0);
  }

  async function handleSaveTrackMarkers() {
    if (!markerData) return;
    setUiError(null);
    try {
      const clampedIntro = Math.max(
        0,
        Math.min(markerData.duration_ms || 600_000, Math.round(markerIntroMs)),
      );
      const st = await invokeCommand('set_track_markers', {
        index: markerTargetIndex ?? undefined,
        path: markerData.path,
        introMs: clampedIntro,
        intro_ms: clampedIntro,
        sweeperPath: markerSweeperPath,
        sweeper_path: markerSweeperPath,
      });
      setStatus(st);
      setShowMarkerModal(false);
      const triggerMs = Math.max(0, clampedIntro - markerSweeperDurationMs);
      showBriefNotice(
        markerSweeperPath
          ? `Marcadores guardados — Intro: ${formatTimeMs(clampedIntro)} | Pisador dispara en ${formatTimeMs(triggerMs)}`
          : `Tiempo de Intro guardado en SQLite: ${formatTimeMs(clampedIntro)}`,
      );
    } catch (err) {
      setUiError(String(err));
    }
  }

  /**
   * Calcula la posición exacta interpolada en vivo del Cabezal de Reproducción (Playhead)
   * del Cue Deck para la pista abierta actualmente en el modal.
   */
  function computeLiveModalPlayheadMs(trackPath: string, durationMs: number): number {
    const tel = cueTelemetryRef.current;
    const isSameFile = tel.cue_file === trackPath || status.cue_file === trackPath;
    if (!isSameFile) {
      return Math.max(0, Math.min(durationMs, cuePlayheadMs));
    }
    const baseMs =
      tel.cue_file === trackPath ? tel.current_position_ms : status.cue_elapsed_ms;
    const playingNow =
      tel.cue_file === trackPath ? tel.is_playing : status.cue_is_playing;
    if (!playingNow) {
      return Math.max(0, Math.min(durationMs, baseMs));
    }
    const deltaMs = Math.max(0, performance.now() - tel.syncedAtPerfMs);
    return Math.max(0, Math.min(durationMs, Math.round(baseMs + deltaMs)));
  }

  /**
   * 3. Clic para Saltar (Seek): Salta inmediatamente en el Cue Deck al milisegundo exacto
   * correspondiente al píxel donde hizo clic el usuario sobre la forma de onda.
   */
  async function handleSeekCueDeck(targetMs: number) {
    if (!markerData) return;
    const clampedMs = Math.max(
      0,
      Math.min(markerData.duration_ms || 600_000, Math.round(targetMs)),
    );
    cueTelemetryRef.current = {
      is_playing: true,
      current_position_ms: clampedMs,
      duration_ms: markerData.duration_ms,
      cue_file: markerData.path,
      syncedAtPerfMs: performance.now(),
    };
    setCuePlayheadMs(clampedMs);
    setCueModalIsPlaying(true);

    try {
      const st = await invokeCommand('seek_cue_deck', {
        path: markerData.path,
        positionMs: clampedMs,
        position_ms: clampedMs,
      });
      setStatus(st);
    } catch (err) {
      setUiError(String(err));
    }
  }

  /**
   * 4. Botón / Atajo 'Fijar marcador aquí' (Tecla M o Intro/Enter):
   * Fija el marcador amarillo de 'Intro' exactamente en el milisegundo donde se encuentra
   * el cabezal de reproducción (Playhead) en ese instante.
   */
  function handleSetIntroAtPlayhead() {
    if (!markerData) return;
    const liveMs = computeLiveModalPlayheadMs(
      markerData.path,
      markerData.duration_ms || 600_000,
    );
    const exactMs = Math.max(
      0,
      Math.min(markerData.duration_ms || 600_000, Math.round(liveMs)),
    );
    setMarkerIntroMs(exactMs);
    showBriefNotice(
      `🚩 Marcador de Intro fijado en el Playhead: ${formatTimeMs(exactMs)}`,
    );
  }

  // Atajo de teclado en el modal 'Editar Marcadores': Tecla 'M' o 'Enter' (Intro) fija el marcador en el Playhead
  useEffect(() => {
    if (!showMarkerModal || !markerData) return;

    function onModalKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.tagName === 'SELECT' ||
          target.isContentEditable)
      ) {
        return;
      }
      if (e.ctrlKey || e.altKey || e.metaKey || e.repeat) {
        return;
      }

      if (e.key === 'm' || e.key === 'M' || e.key === 'Enter') {
        e.preventDefault();
        handleSetIntroAtPlayhead();
      } else if (e.key === ' ') {
        e.preventDefault();
        const isCurrentPlaying =
          (cueTelemetryRef.current.cue_file === markerData?.path &&
            cueTelemetryRef.current.is_playing) ||
          (status.cue_file === markerData?.path && status.cue_is_playing);
        if (isCurrentPlaying) {
          handleStopCue();
        } else if (markerData) {
          handleSeekCueDeck(
            computeLiveModalPlayheadMs(markerData.path, markerData.duration_ms),
          );
        }
      }
    }

    window.addEventListener('keydown', onModalKeyDown);
    return () => window.removeEventListener('keydown', onModalKeyDown);
  }, [showMarkerModal, markerData, status.cue_file, status.cue_is_playing, cuePlayheadMs]);

  function getCanvasTimeMsFromClientX(clientX: number): number {
    const canvas = waveformCanvasRef.current;
    if (!canvas || !markerData || markerData.duration_ms <= 0) return 0;
    const rect = canvas.getBoundingClientRect();
    const relX = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    return Math.round(relX * markerData.duration_ms);
  }

  function isPointerOnIntroFlagHandle(clientX: number, clientY: number): boolean {
    const canvas = waveformCanvasRef.current;
    if (!canvas || !markerData || markerData.duration_ms <= 0) return false;
    const rect = canvas.getBoundingClientRect();
    const localX = clientX - rect.left;
    const localY = clientY - rect.top;
    const introX = (markerIntroMs / Math.max(1, markerData.duration_ms)) * rect.width;
    // La bandera amarilla '🚩 INTRO / VOZ' está en la franja superior (y <= 42px) o muy cerca de la línea amarilla
    if (localY <= 42 && Math.abs(localX - introX) <= 68) {
      return true;
    }
    return Math.abs(localX - introX) <= 10;
  }

  function updateIntroFromCanvasPointer(clientX: number) {
    const canvas = waveformCanvasRef.current;
    if (!canvas || !markerData || markerData.duration_ms <= 0) return;
    const rawMs = getCanvasTimeMsFromClientX(clientX);
    // Redondear a pasos de 100 ms (décimas de segundo, ej. 00:12.5)
    const snappedMs = Math.round(rawMs / 100) * 100;
    setMarkerIntroMs(Math.max(0, Math.min(markerData.duration_ms, snappedMs)));
  }

  // 2. Dibujar la forma de onda, marcadores de Intro/Pisador y el Cabezal de Reproducción (Playhead) en tiempo real (60fps)
  useEffect(() => {
    if (!showMarkerModal || !markerData) return;
    let rafId = 0;

    const renderFrame = () => {
      const canvas = waveformCanvasRef.current;
      if (!canvas) return;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;

      const dpr = window.devicePixelRatio || 1;
      const cssWidth = canvas.clientWidth || 760;
      const cssHeight = canvas.clientHeight || 210;
      const targetW = Math.floor(cssWidth * dpr);
      const targetH = Math.floor(cssHeight * dpr);
      if (canvas.width !== targetW || canvas.height !== targetH) {
        canvas.width = targetW;
        canvas.height = targetH;
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      const durationMs = Math.max(1, markerData.duration_ms);
      const introMs = Math.max(0, Math.min(durationMs, markerIntroMs));
      const introX = (introMs / durationMs) * cssWidth;
      const midY = cssHeight * 0.54;
      const maxWaveHalfHeight = cssHeight * 0.36;

      const livePlayheadMs = computeLiveModalPlayheadMs(markerData.path, durationMs);
      const playheadX = (livePlayheadMs / durationMs) * cssWidth;
      const isCueActiveForThisTrack =
        (cueTelemetryRef.current.cue_file === markerData.path &&
          cueTelemetryRef.current.is_playing) ||
        (status.cue_file === markerData.path && status.cue_is_playing);

      // 1. Fondo oscuro estilo consola broadcast
      ctx.fillStyle = '#090d16';
      ctx.fillRect(0, 0, cssWidth, cssHeight);

      // 2. Cuadrícula y regla de tiempo superior
      ctx.strokeStyle = 'rgba(148, 163, 184, 0.12)';
      ctx.lineWidth = 1;
      const stepSec =
        durationMs <= 20_000
          ? 1
          : durationMs <= 60_000
            ? 5
            : durationMs <= 180_000
              ? 15
              : 30;
      ctx.fillStyle = '#64748b';
      ctx.font = '10px monospace';
      for (let s = 0; s * 1000 <= durationMs; s += stepSec) {
        const x = ((s * 1000) / durationMs) * cssWidth;
        ctx.beginPath();
        ctx.moveTo(x, 18);
        ctx.lineTo(x, cssHeight);
        ctx.stroke();
        const mm = String(Math.floor(s / 60)).padStart(2, '0');
        const ss = String(s % 60).padStart(2, '0');
        ctx.fillText(`${mm}:${ss}`, x + 3, 12);
      }

      // Línea central 0 dB
      ctx.strokeStyle = 'rgba(148, 163, 184, 0.22)';
      ctx.beginPath();
      ctx.moveTo(0, midY);
      ctx.lineTo(cssWidth, midY);
      ctx.stroke();

      // 3. Zona sombreada de Intro (00:00.0 -> Intro)
      if (introMs > 0) {
        ctx.fillStyle = 'rgba(245, 158, 11, 0.14)';
        ctx.fillRect(0, 18, introX, cssHeight - 18);
      }

      // 4. Zona de Pisador (Sweeper): desde (Intro - Duración Pisador) hasta Intro
      const hasSweeper = Boolean(markerSweeperPath) && markerSweeperDurationMs > 0;
      const sweeperTriggerMs = hasSweeper
        ? Math.max(0, introMs - markerSweeperDurationMs)
        : 0;
      const sweeperEndMs = hasSweeper
        ? Math.min(durationMs, sweeperTriggerMs + markerSweeperDurationMs)
        : 0;

      if (hasSweeper) {
        const swStartX = (sweeperTriggerMs / durationMs) * cssWidth;
        const swEndX = (sweeperEndMs / durationMs) * cssWidth;
        const swWidth = Math.max(4, swEndX - swStartX);

        ctx.fillStyle = 'rgba(168, 85, 247, 0.28)';
        ctx.fillRect(swStartX, 22, swWidth, cssHeight - 26);

        ctx.strokeStyle = '#c084fc';
        ctx.lineWidth = 1.5;
        ctx.strokeRect(swStartX, 22, swWidth, cssHeight - 26);

        ctx.fillStyle = '#f3e8ff';
        ctx.font = 'bold 11px sans-serif';
        const labelText = `🎙 PISADOR (${(markerSweeperDurationMs / 1000).toFixed(1)}s) · Disparo: ${formatTimeMs(
          sweeperTriggerMs,
        )}`;
        const textX = Math.max(6, Math.min(cssWidth - 230, swStartX + 6));
        ctx.fillText(labelText, textX, cssHeight - 10);
      }

      // 5. Barras de Picos PCM (Forma de Onda)
      const peaks =
        markerData.peaks && markerData.peaks.length > 0 ? markerData.peaks : [0.2];
      const barCount = peaks.length;
      const slotWidth = cssWidth / barCount;
      const barWidth = Math.max(1.5, slotWidth * 0.72);

      for (let i = 0; i < barCount; i++) {
        const x = i * slotWidth;
        const barTimeMs = ((i + 0.5) / barCount) * durationMs;
        const amp = Math.max(0.04, Math.min(1, peaks[i]));
        const h = amp * maxWaveHalfHeight;

        if (hasSweeper && barTimeMs >= sweeperTriggerMs && barTimeMs <= sweeperEndMs) {
          ctx.fillStyle = barTimeMs <= livePlayheadMs ? '#e9d5ff' : '#d8b4fe';
        } else if (barTimeMs <= introMs) {
          ctx.fillStyle = barTimeMs <= livePlayheadMs ? '#fde047' : '#fbbf24';
        } else if (barTimeMs <= livePlayheadMs && livePlayheadMs > 0) {
          ctx.fillStyle = '#34d399';
        } else {
          ctx.fillStyle = '#38bdf8';
        }

        ctx.fillRect(x, midY - h, barWidth, h * 2);
      }

      // 6. Marcador vertical arrastrable de Fin de Intro / Entrada de Voz
      ctx.strokeStyle = '#f59e0b';
      ctx.lineWidth = 2.5;
      ctx.beginPath();
      ctx.moveTo(introX, 16);
      ctx.lineTo(introX, cssHeight);
      ctx.stroke();

      const badgeText = `🚩 INTRO / VOZ: ${formatTimeMs(introMs)}`;
      ctx.font = 'bold 11px monospace';
      const badgeW = ctx.measureText(badgeText).width + 14;
      const badgeX = Math.max(
        4,
        Math.min(cssWidth - badgeW - 4, introX - badgeW / 2),
      );
      ctx.fillStyle = '#f59e0b';
      ctx.fillRect(badgeX, 18, badgeW, 20);
      ctx.fillStyle = '#090d16';
      ctx.fillText(badgeText, badgeX + 7, 32);

      // 7. Cabezal de Reproducción / Playhead en Tiempo Real (Cue Deck)
      ctx.save();
      ctx.strokeStyle = isCueActiveForThisTrack ? '#22c55e' : '#e2e8f0';
      ctx.lineWidth = 2.2;
      ctx.shadowColor = isCueActiveForThisTrack
        ? 'rgba(34, 197, 94, 0.85)'
        : 'rgba(226, 232, 240, 0.45)';
      ctx.shadowBlur = isCueActiveForThisTrack ? 8 : 3;
      ctx.beginPath();
      ctx.moveTo(playheadX, 14);
      ctx.lineTo(playheadX, cssHeight);
      ctx.stroke();

      // Triángulo superior del Playhead
      ctx.fillStyle = isCueActiveForThisTrack ? '#22c55e' : '#e2e8f0';
      ctx.beginPath();
      ctx.moveTo(playheadX - 6, 14);
      ctx.lineTo(playheadX + 6, 14);
      ctx.lineTo(playheadX, 23);
      ctx.closePath();
      ctx.fill();

      // Pastilla inferior/intermedia con el tiempo exacto del Playhead
      const phLabel = `${isCueActiveForThisTrack ? '▶ CUE' : '⏸ CUE'}: ${formatTimeMs(
        livePlayheadMs,
      )}`;
      ctx.font = 'bold 11px monospace';
      const phW = ctx.measureText(phLabel).width + 12;
      const phX = Math.max(4, Math.min(cssWidth - phW - 4, playheadX - phW / 2));
      const phY = cssHeight - 24;
      ctx.fillStyle = isCueActiveForThisTrack ? '#16a34a' : '#334155';
      ctx.fillRect(phX, phY, phW, 19);
      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = 1;
      ctx.strokeRect(phX, phY, phW, 19);
      ctx.fillStyle = '#ffffff';
      ctx.fillText(phLabel, phX + 6, phY + 13);
      ctx.restore();

      if (isCueActiveForThisTrack) {
        rafId = window.requestAnimationFrame(renderFrame);
      }
    };

    renderFrame();

    return () => {
      if (rafId) {
        window.cancelAnimationFrame(rafId);
      }
    };
  }, [
    showMarkerModal,
    markerData,
    markerIntroMs,
    markerSweeperPath,
    markerSweeperDurationMs,
    cuePlayheadMs,
    cueModalIsPlaying,
    status.cue_is_playing,
    status.cue_file,
    status.cue_elapsed_ms,
  ]);

  const remainingMs =
    status.remaining_ms ??
    Math.max(0, status.duration_ms - status.elapsed_ms);

  const progressPercent =
    status.duration_ms > 0
      ? Math.min(100, Math.max(0, (status.elapsed_ms / status.duration_ms) * 100))
      : 0;

  const cueProgressPercent =
    status.cue_duration_ms > 0
      ? Math.min(
          100,
          Math.max(0, (status.cue_elapsed_ms / status.cue_duration_ms) * 100),
        )
      : 0;

  let countdownStateClass = 'countdown-normal';
  if (status.is_playing && status.duration_ms > 0 && remainingMs <= 5000) {
    countdownStateClass =
      remainingMs <= 2000 ? 'countdown-critical' : 'countdown-warning';
  }

  const jingleSlots =
    status.jingles && status.jingles.length === 9
      ? status.jingles
      : INITIAL_JINGLES;

  const deviceOptions =
    status.available_devices && status.available_devices.length > 0
      ? status.available_devices
      : ['Default Output'];

  const scheduledEvents = status.scheduled_events ?? [];
  const pendingEvents = status.pending_events ?? [];

  return (
    <div className="player-container">
      {/* Cabecera principal, Indicador Visual de Próximo Evento, Botón de Eventos, Ruteo y Modos */}
      <header className="player-header">
        <div className="header-top-row">
          <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
            <img
              src="/perpetua-icon.png"
              alt="Perpetua Logo"
              style={{
                width: '30px',
                height: '30px',
                borderRadius: '6px',
                border: '1px solid #334155',
              }}
            />
            <h1>Perpetua — Broadcast Automation Console</h1>
          </div>
          <div className="status-badges">
            <span className="cabin-clock-badge" title="Reloj del motor de automatización en tiempo real">
              🕒 {status.clock || '00:00:00'}
            </span>
            <button
              type="button"
              className="btn-events"
              onClick={() => {
                setNewEventTime(getNextMinuteHHMM());
                setShowEventsModal(true);
              }}
              title="Abrir el Planificador de Eventos Horarios (Event Scheduler)"
            >
              📅 Eventos ({scheduledEvents.length})
            </button>
            <button
              type="button"
              className="btn-routing"
              onClick={handleOpenRoutingModal}
              title="Configurar tarjetas de sonido para Aire (Main) y Preescucha (Cue)"
            >
              ⚙ Ruteo Audio (Aire / Cue)
            </button>
            {status.is_ducking && (
              <span className="badge-status ducking">
                🔉 DUCKING -70% ACTIVO
              </span>
            )}
            {status.is_paused_by_event && (
              <span className="badge-status paused-event">
                ⏸ PAUSADO POR EVENTO
              </span>
            )}
            {status.is_crossfading && (
              <span className="badge-status crossfading">
                ⇄ CROSSFADE EN CURSO
              </span>
            )}
            <span
              className={`badge-status ${status.is_playing ? 'on-air' : 'stopped'}`}
            >
              {status.is_playing ? '● AL AIRE (ON AIR)' : '■ DETENIDO'}
            </span>
          </div>
        </div>

        {/* 4. Indicador Visual en Cabina: Próximo Evento y Estado de Automatización */}
        <div className="cabin-event-banner">
          <div className="cabin-event-next">
            {status.next_event ? (
              <>
                <span className="event-banner-icon">⏰</span>
                <span>
                  Próximo evento a las{' '}
                  <strong>{status.next_event.time}:00</strong> —{' '}
                  <strong className="event-banner-title">
                    {status.next_event.title}
                  </strong>{' '}
                  <span className="event-banner-countdown">
                    (en {formatSecondsUntil(status.next_event.seconds_until)} ·{' '}
                    {formatEventInterruptionLabel(
                      status.next_event.interruption_mode,
                      status.next_event.wait_for_current,
                    )}
                    )
                  </span>
                </span>
              </>
            ) : (
              <span className="event-banner-empty">
                ⏰ Próximo evento: Sin eventos programados activos —{' '}
                <button
                  type="button"
                  className="btn-inline-link"
                  onClick={() => {
                    setNewEventTime(getNextMinuteHHMM());
                    setShowNewEventForm(true);
                    setShowEventsModal(true);
                  }}
                >
                  + Programar evento
                </button>
              </span>
            )}
          </div>

          <div className="cabin-event-states">
            {status.overlay_is_playing && (
              <span className="event-pill-onair">
                {status.is_paused_by_event
                  ? `⏸ PAUSA Y REANUDA: ${status.overlay_file?.split('/').pop() ?? 'Evento'}`
                  : `🔉 TALKOVER (-70%): ${status.overlay_file?.split('/').pop() ?? 'Evento'}`}
              </span>
            )}
            {status.on_air_event && (
              <span className="event-pill-onair">
                📡 EVENTO AL AIRE: {status.on_air_event.title}
              </span>
            )}
            {pendingEvents.length > 0 && (
              <span className="event-pill-pending">
                ⏳ En espera ({pendingEvents.length}): {pendingEvents[0].title}
              </span>
            )}
          </div>
        </div>

        <div className="mode-selector-bar">
          <span className="mode-selector-label">Modo de Emisión:</span>
          <div className="mode-buttons">
            {PLAYBACK_MODES.map((m) => (
              <button
                key={m.id}
                type="button"
                className={`btn-mode ${status.playback_mode === m.id ? 'active' : ''}`}
                onClick={() => handleModeChange(m.id)}
                title={m.desc}
              >
                {m.label}
              </button>
            ))}
          </div>
        </div>
      </header>

      {/* Panel de Tiempo al Aire + Contador Regresivo Grande (-MM:SS) */}
      <div className="on-air-deck">
        <div className="deck-info-col">
          <div className="now-playing-label">
            MAIN DECK — SALIDA AIRE ({status.device_name})
            {status.is_ducking && (
              <span className="deck-event-tag">
                🔉 ATENUADO -70% (Evento superpuesto activo)
              </span>
            )}
            {status.is_paused_by_event && (
              <span className="deck-event-tag">
                ⏸ EN PAUSA POR EVENTO (Reanuda automáticamente)
              </span>
            )}
            {status.on_air_event && (
              <span className="deck-event-tag">
                📡 EVENTO EN EMISIÓN (Lista en pausa)
              </span>
            )}
            {status.sweeper_is_playing && (
              <span className="deck-sweeper-tag active">
                🎙 PISADOR AL AIRE: {status.sweeper_file?.split('/').pop() ?? 'Sweeper'} (Voz en{' '}
                {formatTimeMs(status.sweeper_intro_ms ?? 0)})
              </span>
            )}
            {!status.sweeper_is_playing && status.sweeper_armed && (
              <span className="deck-sweeper-tag armed">
                🎙 PISADOR ARMADO: {status.sweeper_file?.split('/').pop() ?? 'Sweeper'} · Disparo
                en {formatTimeMs(status.sweeper_trigger_ms ?? 0)} (Intro{' '}
                {formatTimeMs(status.sweeper_intro_ms ?? 0)})
              </span>
            )}
          </div>
          <div className="now-playing-title">
            {status.on_air_event
              ? `📡 ${status.on_air_event.title}`
              : status.current_file
                ? status.current_file.split('/').pop()
                : 'Ninguna pista activa'}
          </div>
          <div className="elapsed-detail">
            Transcurrido: <strong>{formatTimeMs(status.elapsed_ms)}</strong> /{' '}
            Duración: <strong>{formatTimeMs(status.duration_ms)}</strong>
          </div>

          <div className="progress-bar-track">
            <div
              className={`progress-bar-fill ${countdownStateClass}`}
              style={{ width: `${progressPercent.toFixed(2)}%` }}
            />
          </div>
        </div>

        <div className={`countdown-box ${countdownStateClass}`}>
          <div className="countdown-caption">TIEMPO RESTANTE</div>
          <div className="countdown-digits">{formatCountdownMMSS(remainingMs)}</div>
          <div className="countdown-sub">
            {status.is_playing && remainingMs <= 5000 && remainingMs > 0
              ? '⚠ Fin de pista próximo'
              : `${(remainingMs / 1000).toFixed(1)} s`}
          </div>
        </div>
      </div>

      {/* Controles de Transporte + Crossfade + Persistencia de Listas */}
      <div className="controls-bar">
        <div className="transport-group">
          <button
            type="button"
            className="btn-play"
            onClick={handlePlay}
            title="Reproducir pista seleccionada o primera en cola"
          >
            ▶ Play
          </button>
          <button
            type="button"
            className="btn-stop"
            onClick={handleStop}
            title="Detener reproducción del Main Deck"
          >
            ⏹ Stop
          </button>
          <button
            type="button"
            className="btn-next"
            onClick={handleNextTrack}
            disabled={status.playlist.length === 0}
            title="Saltar a la siguiente pista (con Crossfade si está al aire)"
          >
            ⏭ Siguiente
          </button>
        </div>

        <div className="crossfade-control-group">
          <label htmlFor="crossfade-slider">
            Solapamiento Crossfade:{' '}
            <strong>{(status.crossfade_ms / 1000).toFixed(1)} s</strong>
          </label>
          <div className="crossfade-inputs">
            <input
              id="crossfade-slider"
              type="range"
              min={0}
              max={5000}
              step={100}
              value={status.crossfade_ms}
              onChange={(e) => handleCrossfadeChange(Number(e.target.value))}
              className="crossfade-slider"
            />
            <div className="crossfade-presets">
              {[0, 1000, 2000, 3000, 5000].map((ms) => (
                <button
                  key={ms}
                  type="button"
                  className={`btn-preset ${status.crossfade_ms === ms ? 'active' : ''}`}
                  onClick={() => handleCrossfadeChange(ms)}
                >
                  {ms / 1000}s
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="file-actions-group">
          <button
            type="button"
            className="btn-primary"
            onClick={handleSelectFiles}
            title="Añadir archivos de audio a la cola"
          >
            🎵 Añadir Audio...
          </button>
          <button
            type="button"
            className="btn-folder"
            onClick={handleSelectRandomFolder}
            title="Añadir una Carpeta Aleatoria (Random Folder con regla Anti-Repeat de las últimas 50 canciones)"
          >
            📁 Añadir Carpeta...
          </button>
          <button
            type="button"
            className="btn-time-cmd"
            onClick={handleAddTimeAnnouncement}
            title="Insertar Comando Virtual de Hora (.time) que pronuncia la hora actual desde la carpeta horas/"
          >
            🕒 Añadir Hora (.time)
          </button>
          <button
            type="button"
            onClick={handleLoadPlaylist}
            title="Abrir una lista guardada (.m3u8, .m3u o .json)"
          >
            📥 Abrir Lista...
          </button>
          <button
            type="button"
            onClick={handleSavePlaylist}
            disabled={status.playlist.length === 0}
            title="Guardar la lista actual y modo de emisión en .m3u8 o .json"
          >
            💾 Guardar Lista...
          </button>
          <button
            type="button"
            onClick={handleAddTestTone}
            title="Añadir archivo local de prueba test_tone.wav"
          >
            + Tono
          </button>
          <button
            type="button"
            className="btn-danger-outline"
            onClick={handleClearPlaylist}
            disabled={status.playlist.length === 0}
          >
            🗑 Vaciar
          </button>
        </div>
      </div>

      {/* Layout principal: Lista de Reproducción + Botonera de Jingles (Cart Machine 1-9) */}
      <div className="workspace-split">
        {/* Columna Izquierda: Lista de Reproducción */}
        <div className="playlist-section">
          <div className="playlist-header-row">
            <h2>
              Cola de Reproducción ({status.playlist.length}{' '}
              {status.playlist.length === 1 ? 'ítem' : 'ítems'}) — Modo:{' '}
              <span className="mode-highlight">
                {status.playback_mode.toUpperCase()}
              </span>
            </h2>
            <span
              className="anti-repeat-pill"
              title={
                (status.recent_played_history ?? []).length > 0
                  ? `Últimas canciones protegidas contra repetición:\n${(
                      status.recent_played_history ?? []
                    )
                      .slice(0, 8)
                      .map((p, i) => `${i + 1}. ${p.split('/').pop()}`)
                      .join('\n')}`
                  : 'Historial Anti-Repeat (guarda los últimos 50 audios reproducidos)'
              }
            >
              🛡 Anti-Repeat: {(status.recent_played_history ?? []).length}/50
            </span>
          </div>

          {status.playlist.length === 0 ? (
            <div className="playlist-empty">
              La lista está vacía. Usa <strong>“🎵 Añadir Audio...”</strong>,{' '}
              <strong>“📁 Añadir Carpeta...”</strong> o{' '}
              <strong>“🕒 Añadir Hora (.time)”</strong> para armar tu rotación.
            </div>
          ) : (
            <div className="playlist-table-wrapper">
              <table className="playlist-table">
                <thead>
                  <tr>
                    <th style={{ width: '40px' }}>#</th>
                    <th>Elemento / Ruta / Marcadores de Mezcla</th>
                    <th style={{ width: '92px', textAlign: 'right' }}>Duración</th>
                    <th style={{ width: '215px', textAlign: 'center' }}>Acción / Pisador</th>
                  </tr>
                </thead>
                <tbody>
                  {status.playlist.map((track, idx) => {
                    const isCurrent = status.current_index === idx;
                    const isPlayingCurrent = isCurrent && status.is_playing;
                    const kind: TrackKind =
                      track.kind ??
                      (track.path === '.time'
                        ? 'time_announcement'
                        : 'file');
                    const isFolder = kind === 'random_folder';
                    const isTimeCmd = kind === 'time_announcement';
                    const typeIcon = isFolder
                      ? '📁'
                      : isTimeCmd
                        ? '🕒'
                        : '🎵';
                    const cueTarget = isFolder
                      ? track.last_resolved_file ?? null
                      : isTimeCmd
                        ? null
                        : track.path;
                    const isCueTrack =
                      Boolean(cueTarget) &&
                      status.cue_is_playing &&
                      status.cue_file === cueTarget;

                    const isExpanded = isFolder && Boolean(expandedFolders[track.id]);
                    const folderEntries = folderContentsByTrackId[track.id] ?? [];
                    const isLoadingFolder = loadingFolderTrackId === track.id;
                    const introMs = track.intro_ms ?? 0;
                    const hasSweeper = Boolean(track.sweeper_path);
                    const sweeperTriggerMs =
                      track.sweeper_trigger_ms ??
                      Math.max(0, introMs - (track.sweeper_duration_ms ?? 0));

                    return (
                      <React.Fragment key={track.id}>
                        <tr
                          className={`playlist-row ${isCurrent ? 'current-track' : ''} ${
                            isPlayingCurrent ? 'playing-track' : ''
                          } ${isFolder ? 'row-random-folder' : ''} ${
                            isTimeCmd ? 'row-time-cmd' : ''
                          }`}
                          onDoubleClick={() => handlePlayTrack(idx)}
                          onContextMenu={(e) =>
                            handleTrackContextMenu(e, track, idx)
                          }
                          title={
                            isFolder
                              ? 'Carpeta Aleatoria (Anti-Repeat 50): Haz clic en la flecha ▶ para desplegar sus archivos o doble clic para emitir un audio al azar al Aire'
                              : isTimeCmd
                                ? 'Comando Virtual .time: Doble clic para pronunciar la hora actual encadenando audios de horas/'
                                : 'Doble clic para emitir al Aire | Clic derecho para Editar Marcadores (Intro / Forma de Onda), Asignar Pisador o Preescuchar en Cue'
                          }
                        >
                          <td className="col-index">
                            {isPlayingCurrent ? '▶' : isCurrent ? '•' : idx + 1}
                          </td>
                          <td className="col-title">
                            <div className="track-title-line">
                              {isFolder && (
                                <button
                                  type="button"
                                  className={`btn-folder-chevron ${isExpanded ? 'open' : ''}`}
                                  onClick={(e) => handleToggleFolderExpand(track, e)}
                                  title={
                                    isExpanded
                                      ? 'Ocultar contenidos visuales de la carpeta'
                                      : 'Desplegar archivos de la carpeta (solo visualización; el slot sigue siendo aleatorio)'
                                  }
                                >
                                  {isExpanded ? '▼' : '▶'}
                                </button>
                              )}
                              <span className="track-kind-icon">{typeIcon}</span>
                              <span>{track.title}</span>
                              {isFolder && (
                                <span className="track-kind-badge kind-folder">
                                  SLOT ALEATORIO · ANTI-REPEAT
                                </span>
                              )}
                              {isTimeCmd && (
                                <span className="track-kind-badge kind-time">
                                  .TIME · HORA ACTUAL
                                </span>
                              )}
                              {!isTimeCmd && introMs > 0 && (
                                <button
                                  type="button"
                                  className="track-intro-badge"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    if (!isFolder) {
                                      handleOpenMarkerEditor(track.path, idx);
                                    }
                                  }}
                                  title={`Tiempo de Intro guardado en SQLite: ${formatTimeMs(
                                    introMs,
                                  )}. Haz clic para abrir el Editor Visual de Puntos de Mezcla.`}
                                >
                                  🚩 Intro: {formatTimeMs(introMs)}
                                </button>
                              )}
                              {!isTimeCmd && hasSweeper && (
                                <span
                                  className="track-sweeper-badge"
                                  title={`Pisador asignado: ${
                                    track.sweeper_title ?? track.sweeper_path
                                  } (${formatTimeMs(
                                    track.sweeper_duration_ms ?? 0,
                                  )}) → Se disparará en el segundo ${(
                                    sweeperTriggerMs / 1000
                                  ).toFixed(1)}s (Intro ${(introMs / 1000).toFixed(
                                    1,
                                  )}s - Pisador ${(
                                    (track.sweeper_duration_ms ?? 0) / 1000
                                  ).toFixed(1)}s)`}
                                >
                                  🎙 {track.sweeper_title ?? 'Pisador'} (@{' '}
                                  {(sweeperTriggerMs / 1000).toFixed(1)}s)
                                  <button
                                    type="button"
                                    className="btn-sweeper-clear-inline"
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      handleAssignTrackSweeperQuick(
                                        idx,
                                        track.path,
                                        null,
                                      );
                                    }}
                                    title="Quitar Pisador de esta pista"
                                  >
                                    ✕
                                  </button>
                                </span>
                              )}
                              {isCueTrack && (
                                <span className="cue-inline-tag">🎧 CUE</span>
                              )}
                            </div>
                            <div className="col-subpath">
                              {isTimeCmd
                                ? `Secuencia sin pausas desde ${status.horas_dir ?? 'horas'}/ (son_las + HH + y_MM)`
                                : isFolder && track.last_resolved_file
                                  ? `${track.path} · Última elegida: ${track.last_resolved_file.split('/').pop()}`
                                  : track.path}
                            </div>
                          </td>
                          <td className="col-duration">
                            {isTimeCmd
                              ? 'AUTO'
                              : isFolder && track.duration_ms === 0
                                ? 'DINÁM.'
                                : formatTimeMs(track.duration_ms)}
                          </td>
                          <td className="col-actions">
                            {!isFolder && !isTimeCmd && (
                              <button
                                type="button"
                                className="btn-row-markers"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleOpenMarkerEditor(track.path, idx);
                                }}
                                title="Editar Marcadores: Abrir forma de onda para arrastrar el Tiempo de Intro y configurar el Pisador"
                              >
                                🚩 Intro
                              </button>
                            )}
                            {!isTimeCmd && (
                              <button
                                type="button"
                                className={`btn-row-sweeper ${
                                  hasSweeper ? 'assigned' : ''
                                }`}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleAssignTrackSweeperFromFile(
                                    idx,
                                    track.path,
                                  );
                                }}
                                title={
                                  hasSweeper
                                    ? `Cambiar Pisador asignado (${track.sweeper_title})`
                                    : 'Asignar un archivo de Pisador (Sweeper) para que suene sobre el Intro de esta pista'
                                }
                              >
                                🎙 Pisador
                              </button>
                            )}
                            {cueTarget ? (
                              <button
                                type="button"
                                className="btn-row-cue"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handlePlayCuePath(cueTarget);
                                }}
                                title="Preescuchar por la salida Cue"
                              >
                                🎧 Cue
                              </button>
                            ) : (
                              <button
                                type="button"
                                className="btn-play"
                                style={{ padding: '3px 7px', fontSize: '11px' }}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handlePlayTrack(idx);
                                }}
                                title={
                                  isFolder
                                    ? 'Sortear canción de esta carpeta y reproducir al Aire'
                                    : 'Ejecutar locución de hora (.time) ahora'
                                }
                              >
                                ▶ Aire
                              </button>
                            )}
                            <button
                              type="button"
                              className="btn-row-remove"
                              onClick={(e) => handleRemoveTrack(idx, e)}
                              title="Eliminar ítem de la lista"
                            >
                              ✕
                            </button>
                          </td>
                        </tr>

                        {/* Sub-filas desplegables de la Carpeta Aleatoria (estrictamente visual) */}
                        {isExpanded && (
                          isLoadingFolder ? (
                            <tr className="folder-subrow">
                              <td className="col-index">↳</td>
                              <td colSpan={3} className="folder-subrow-loading">
                                Escaneando archivos de la carpeta sin cargarlos en memoria...
                              </td>
                            </tr>
                          ) : folderEntries.length === 0 ? (
                            <tr className="folder-subrow">
                              <td className="col-index">↳</td>
                              <td colSpan={3} className="folder-subrow-loading">
                                No se encontraron archivos de audio compatibles en esta carpeta.
                              </td>
                            </tr>
                          ) : (
                            folderEntries.map((entry, subIdx) => {
                              const isLastPicked =
                                track.last_resolved_file === entry.path;
                              const isSubCue =
                                status.cue_is_playing &&
                                status.cue_file === entry.path;
                              return (
                                <tr
                                  key={`${track.id}-sub-${entry.path}`}
                                  className={`folder-subrow ${
                                    isLastPicked ? 'subrow-last-picked' : ''
                                  }`}
                                >
                                  <td className="col-index subrow-branch">
                                    {subIdx === folderEntries.length - 1 ? '└─' : '├─'}
                                  </td>
                                  <td className="col-title">
                                    <div className="track-title-line">
                                      <span className="subrow-file-icon">🎵</span>
                                      <span className="subrow-file-title">
                                        {entry.title}
                                      </span>
                                      {isLastPicked && (
                                        <span className="track-kind-badge kind-folder">
                                          ÚLTIMA SORTEADA
                                        </span>
                                      )}
                                      {entry.in_recent_history && (
                                        <span
                                          className="subrow-history-badge"
                                          title="Esta canción ya sonó en las últimas 50 pistas; Anti-Repeat evitará repetirla mientras haya otras disponibles."
                                        >
                                          🛡 EN HISTORIAL 50
                                        </span>
                                      )}
                                      {isSubCue && (
                                        <span className="cue-inline-tag">
                                          🎧 CUE
                                        </span>
                                      )}
                                    </div>
                                    <div className="col-subpath">{entry.path}</div>
                                  </td>
                                  <td className="col-duration subrow-meta">
                                    {entry.extension}{' '}
                                    {entry.size_bytes > 0
                                      ? `· ${formatBytesKB(entry.size_bytes)}`
                                      : ''}
                                  </td>
                                  <td className="col-actions">
                                    <button
                                      type="button"
                                      className="btn-row-markers"
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        handleOpenMarkerEditor(entry.path, null);
                                      }}
                                      title="Editar Marcadores (Intro / Pisador) para este archivo de la carpeta"
                                    >
                                      🚩 Intro
                                    </button>
                                    <button
                                      type="button"
                                      className="btn-row-cue"
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        handlePlayCuePath(entry.path);
                                      }}
                                      title="Preescuchar este archivo de la carpeta en Cue (sin alterar el slot aleatorio)"
                                    >
                                      🎧 Cue
                                    </button>
                                  </td>
                                </tr>
                              );
                            })
                          )
                        )}
                      </React.Fragment>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>

        {/* Columna Derecha: Botonera de Jingles (Cart Machine 1-9) */}
        <div className="jingle-panel">
          <div className="jingle-panel-header">
            <div>
              <h2>🎛 Botonera de Jingles (Cart Machine)</h2>
              <span className="jingle-subtitle">
                Clic izq. / Teclas <strong>1–9</strong>: Aire | Clic der.: Menú{' '}
                <strong>Preescuchar (Cue)</strong> / Asignar
              </span>
            </div>
            <button
              type="button"
              className="btn-preset"
              onClick={handleAssignTestToneToSlot1}
              title="Asignar test_tone.wav a la Ranura 1"
            >
              ⚡ Cargar prueba en [1]
            </button>
          </div>

          <div className="jingle-grid">
            {jingleSlots.map((jingle) => {
              const isAssigned = Boolean(jingle.path);
              const isPlaying =
                jingle.is_playing || flashedSlot === jingle.slot;

              return (
                <div
                  key={jingle.slot}
                  role="button"
                  tabIndex={0}
                  className={`jingle-pad ${isAssigned ? 'assigned' : 'empty'} ${
                    isPlaying ? 'playing' : ''
                  }`}
                  onClick={() => handleTriggerJingle(jingle)}
                  onContextMenu={(e) => handleJingleContextMenu(e, jingle)}
                  title={
                    jingle.path
                      ? `Clic izquierdo o Tecla [${jingle.slot}] para sonar al Aire. Clic derecho para Preescuchar (Cue) o cambiar archivo.`
                      : `Ranura #${jingle.slot} vacía. Clic derecho para Preescuchar un audio en Cue o asignarlo.`
                  }
                >
                  <div className="jingle-pad-top">
                    <span className="jingle-key-badge">[{jingle.slot}]</span>
                    <span
                      className={`jingle-state-pill ${
                        isPlaying
                          ? 'pill-on'
                          : isAssigned
                            ? 'pill-ready'
                            : 'pill-empty'
                      }`}
                    >
                      {isPlaying
                        ? '🔊 AL AIRE'
                        : isAssigned
                          ? 'LISTO'
                          : 'VACÍO'}
                    </span>
                  </div>

                  <div className="jingle-pad-title">
                    {jingle.title ?? 'Sin asignar'}
                  </div>

                  <div className="jingle-pad-footer">
                    <span className="jingle-pad-duration">
                      {isAssigned ? formatTimeMs(jingle.duration_ms) : '--:--.-'}
                    </span>
                    <div className="jingle-pad-actions">
                      <button
                        type="button"
                        className="btn-pad-cue"
                        onClick={(e) => {
                          e.stopPropagation();
                          if (jingle.path) {
                            handlePlayCuePath(jingle.path);
                          } else {
                            handlePickAndPlayCueFile();
                          }
                        }}
                        title={
                          jingle.path
                            ? `Preescuchar Jingle #${jingle.slot} en Cue`
                            : 'Elegir archivo para Preescuchar en Cue'
                        }
                      >
                        🎧
                      </button>
                      <button
                        type="button"
                        className="btn-pad-assign"
                        onClick={(e) => handleAssignJingleFile(jingle.slot, e)}
                        title={`Asignar archivo a ranura ${jingle.slot}`}
                      >
                        📂
                      </button>
                      {isAssigned && (
                        <button
                          type="button"
                          className="btn-pad-clear"
                          onClick={(e) => handleClearJingleSlot(jingle.slot, e)}
                          title={`Vaciar ranura ${jingle.slot}`}
                        >
                          ✕
                        </button>
                      )}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* 2. Barra Inferior de Preescucha (Cue Player) */}
      <div className={`cue-player-bar ${status.cue_is_playing ? 'cue-active' : ''}`}>
        <div className="cue-left">
          <span
            className={`cue-badge ${status.cue_is_playing ? 'playing' : 'idle'}`}
          >
            {status.cue_is_playing ? '🎧 CUE ACTIVO' : '🎧 CUE / PREESCUCHA'}
          </span>
          <div className="cue-meta">
            <div className="cue-title">
              {status.cue_file
                ? status.cue_file.split('/').pop()
                : 'Haz clic derecho sobre cualquier pista o Jingle y elige “Preescuchar”'}
            </div>
            <div className="cue-device-sub">
              Salida Cue: <strong>{status.cue_device_name}</strong> |{' '}
              {formatTimeMs(status.cue_elapsed_ms)} /{' '}
              {formatTimeMs(status.cue_duration_ms)}
            </div>
          </div>
        </div>

        <div className="cue-progress-wrap">
          <div className="cue-progress-track">
            <div
              className="cue-progress-fill"
              style={{ width: `${cueProgressPercent.toFixed(2)}%` }}
            />
          </div>
        </div>

        <div className="cue-controls">
          {status.cue_file && (
            <button
              type="button"
              className="btn-cue-play"
              onClick={() =>
                status.cue_file && handlePlayCuePath(status.cue_file)
              }
              title="Reiniciar preescucha del archivo actual en Cue"
            >
              ▶ Reiniciar Cue
            </button>
          )}
          <button
            type="button"
            className="btn-cue-stop"
            onClick={handleStopCue}
            disabled={!status.cue_is_playing}
            title="Detener preescucha Cue"
          >
            ⏹ Detener Cue
          </button>
          <button
            type="button"
            onClick={handlePickAndPlayCueFile}
            title="Seleccionar cualquier archivo local para preescucharlo por la salida Cue"
          >
            📂 Preescuchar Archivo...
          </button>
        </div>
      </div>

      {uiNotice && <div className="status-notice">✔ {uiNotice}</div>}

      {(uiError || status.last_error) && (
        <div className="status-error">
          <strong>Error:</strong> {uiError || status.last_error}
        </div>
      )}

      {/* Menú Contextual Flotante (Clic Derecho en Pista o Jingle) */}
      {contextMenu && (
        <div
          className="context-menu"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          onClick={(e) => e.stopPropagation()}
        >
          {contextMenu.type === 'track' && contextMenu.track && (
            <>
              <div className="context-menu-header">
                Pista: {contextMenu.track.title}
              </div>
              {contextMenu.track.kind !== 'random_folder' &&
                contextMenu.track.kind !== 'time_announcement' &&
                contextMenu.track.path !== '.time' && (
                  <button
                    type="button"
                    className="context-menu-item highlight-marker"
                    onClick={() => {
                      const tr = contextMenu.track!;
                      const idx = contextMenu.trackIndex ?? null;
                      setContextMenu(null);
                      handleOpenMarkerEditor(tr.path, idx);
                    }}
                  >
                    🚩 Editar Marcadores (Intro / Forma de Onda)...
                  </button>
                )}
              {contextMenu.track.kind !== 'time_announcement' &&
                contextMenu.track.path !== '.time' && (
                  <>
                    <button
                      type="button"
                      className="context-menu-item"
                      onClick={() => {
                        const tr = contextMenu.track!;
                        const idx = contextMenu.trackIndex ?? null;
                        setContextMenu(null);
                        handleAssignTrackSweeperFromFile(idx, tr.path);
                      }}
                    >
                      🎙 Asignar Pisador (Sweeper) desde archivo...
                    </button>
                    {jingleSlots
                      .filter((j) => Boolean(j.path))
                      .slice(0, 4)
                      .map((j) => (
                        <button
                          key={`ctx-sw-${j.slot}`}
                          type="button"
                          className="context-menu-item"
                          onClick={() => {
                            const tr = contextMenu.track!;
                            const idx = contextMenu.trackIndex ?? null;
                            setContextMenu(null);
                            handleAssignTrackSweeperQuick(idx, tr.path, j.path);
                          }}
                        >
                          🎙 Usar Jingle [{j.slot}] como Pisador ({j.title})
                        </button>
                      ))}
                    {contextMenu.track.sweeper_path && (
                      <button
                        type="button"
                        className="context-menu-item danger"
                        onClick={() => {
                          const tr = contextMenu.track!;
                          const idx = contextMenu.trackIndex ?? null;
                          setContextMenu(null);
                          handleAssignTrackSweeperQuick(idx, tr.path, null);
                        }}
                      >
                        ✕ Quitar Pisador asignado ({contextMenu.track.sweeper_title})
                      </button>
                    )}
                  </>
                )}
              <button
                type="button"
                className="context-menu-item"
                onClick={() => {
                  const p = contextMenu.track!.path;
                  setContextMenu(null);
                  handlePlayCuePath(p);
                }}
              >
                🎧 Preescuchar en Cue
              </button>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => {
                  const idx = contextMenu.trackIndex!;
                  setContextMenu(null);
                  handlePlayTrack(idx);
                }}
              >
                ▶ Reproducir en Aire (Main Deck)
              </button>
              <button
                type="button"
                className="context-menu-item danger"
                onClick={() => {
                  const idx = contextMenu.trackIndex!;
                  setContextMenu(null);
                  handleRemoveTrack(idx);
                }}
              >
                ✕ Quitar de la lista
              </button>
            </>
          )}

          {contextMenu.type === 'jingle' && contextMenu.jingle && (
            <>
              <div className="context-menu-header">
                Ranura Jingle [{contextMenu.jingle.slot}] —{' '}
                {contextMenu.jingle.title ?? 'Vacío'}
              </div>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => {
                  const j = contextMenu.jingle!;
                  setContextMenu(null);
                  if (j.path) {
                    handlePlayCuePath(j.path);
                  } else {
                    handlePickAndPlayCueFile();
                  }
                }}
              >
                🎧 Preescuchar {contextMenu.jingle.path ? 'este Jingle' : 'archivo local...'}
              </button>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => {
                  const s = contextMenu.jingle!.slot;
                  setContextMenu(null);
                  handleAssignJingleFile(s);
                }}
              >
                📂 Asignar archivo de audio a [{contextMenu.jingle.slot}]...
              </button>
              {contextMenu.jingle.path && (
                <button
                  type="button"
                  className="context-menu-item danger"
                  onClick={() => {
                    const s = contextMenu.jingle!.slot;
                    setContextMenu(null);
                    handleClearJingleSlot(s);
                  }}
                >
                  ✕ Vaciar ranura [{contextMenu.jingle.slot}]
                </button>
              )}
            </>
          )}
        </div>
      )}

      {/* 1. Modal de Configuración de Ruteo de Audio Multitarjeta */}
      {showRoutingModal && (
        <div
          className="modal-backdrop"
          onClick={() => setShowRoutingModal(false)}
        >
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>⚙ Ruteo de Audio Multitarjeta (CPAL)</h2>
              <button
                type="button"
                className="btn-close"
                onClick={() => setShowRoutingModal(false)}
              >
                ✕
              </button>
            </div>

            <p className="modal-desc">
              Selecciona qué tarjeta de sonido física de tu sistema se asigna a la
              salida principal (<strong>Main / Aire</strong>) y cuál a los
              audífonos de monitoreo (<strong>Cue / Preescucha</strong>).
            </p>

            <div className="modal-field">
              <label htmlFor="select-air-device">
                📡 Salida Principal (Main / Aire + Jingles):
              </label>
              <select
                id="select-air-device"
                className="device-select"
                value={selectedAirDevice}
                onChange={(e) => setSelectedAirDevice(e.target.value)}
              >
                {deviceOptions.map((dev) => (
                  <option key={`air-${dev}`} value={dev}>
                    {dev}
                  </option>
                ))}
              </select>
            </div>

            <div className="modal-field">
              <label htmlFor="select-cue-device">
                🎧 Salida de Preescucha (Cue / Monitoreo):
              </label>
              <select
                id="select-cue-device"
                className="device-select"
                value={selectedCueDevice}
                onChange={(e) => setSelectedCueDevice(e.target.value)}
              >
                {deviceOptions.map((dev) => (
                  <option key={`cue-${dev}`} value={dev}>
                    {dev}
                  </option>
                ))}
              </select>
            </div>

            <div className="routing-mode-info">
              {selectedAirDevice === selectedCueDevice ? (
                <span>
                  ℹ <strong>Modo Tarjeta Única / Compartida:</strong> Aire y Cue
                  compartirán el mismo dispositivo de salida sin conflictos de
                  bloqueo.
                </span>
              ) : (
                <span>
                  ✔ <strong>Modo Multitarjeta Independiente:</strong> La
                  preescucha (Cue) saldrá por una tarjeta física separada del Aire.
                </span>
              )}
            </div>

            <div className="modal-actions">
              <button
                type="button"
                onClick={async () => {
                  const st = await invokeCommand('refresh_devices');
                  setStatus(st);
                }}
              >
                ↻ Refrescar Dispositivos
              </button>
              <div style={{ display: 'flex', gap: '8px' }}>
                <button
                  type="button"
                  onClick={() => setShowRoutingModal(false)}
                >
                  Cancelar
                </button>
                <button
                  type="button"
                  className="btn-primary"
                  onClick={handleApplyAudioRouting}
                >
                  💾 Aplicar Ruteo
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 3. Modal / Panel de Eventos Programados (Event Scheduler - Hito 6) */}
      {showEventsModal && (
        <div
          className="modal-backdrop"
          onClick={() => setShowEventsModal(false)}
        >
          <div
            className="modal-card modal-events-card"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="modal-header">
              <div>
                <h2>📅 Planificador de Eventos Horarios (Event Scheduler)</h2>
                <span className="events-modal-sub">
                  Reloj de Cabina: <strong>{status.clock}</strong> |{' '}
                  {status.next_event
                    ? `Próximo: ${status.next_event.title} a las ${status.next_event.time}:00 (en ${formatSecondsUntil(status.next_event.seconds_until)})`
                    : 'Sin eventos activos'}
                </span>
              </div>
              <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
                <button
                  type="button"
                  className="btn-primary"
                  onClick={() => {
                    if (!showNewEventForm) {
                      setNewEventTime(getNextMinuteHHMM());
                    }
                    setShowNewEventForm((v) => !v);
                  }}
                >
                  {showNewEventForm
                    ? '▲ Ocultar Formulario'
                    : '➕ Añadir Nuevo Evento'}
                </button>
                <button
                  type="button"
                  className="btn-close"
                  onClick={() => setShowEventsModal(false)}
                >
                  ✕
                </button>
              </div>
            </div>

            {status.last_event_message && (
              <div className="events-last-msg">
                📡 Última actividad del reloj: <strong>{status.last_event_message}</strong>
              </div>
            )}

            {/* Formulario: Añadir Nuevo Evento */}
            {showNewEventForm && (
              <form className="event-form" onSubmit={handleCreateEvent}>
                <div className="event-form-title">
                  Configurar Nuevo Evento Programado
                </div>

                <div className="event-form-grid">
                  {/* 1. Hora de Ejecución (HH:MM) */}
                  <div className="modal-field">
                    <label htmlFor="event-time-input">
                      ⏰ Hora de Ejecución (HH:MM, 24h):
                    </label>
                    <div style={{ display: 'flex', gap: '6px' }}>
                      <input
                        id="event-time-input"
                        type="time"
                        required
                        className="event-time-input"
                        value={newEventTime}
                        onChange={(e) => setNewEventTime(e.target.value)}
                      />
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() => setNewEventTime(getNextMinuteHHMM())}
                        title="Fijar al próximo minuto del reloj del sistema para prueba rápida"
                      >
                        ⏱ +1 min
                      </button>
                    </div>
                  </div>

                  {/* 2. Modo de Interrupción No Destructiva (Ducking -70%, Pausa y Reanuda, o Espera al Final) */}
                  <div className="modal-field">
                    <label>🎛 Modo de Interrupción No Destructiva:</label>
                    <div className="event-wait-selector">
                      <button
                        type="button"
                        className={`btn-wait-mode ${
                          newEventInterruptionMode === 'ducking' ? 'active-duck' : ''
                        }`}
                        onClick={() => setNewEventInterruptionMode('ducking')}
                        title="Baja suavemente el volumen de la canción actual un 70%, emite el evento encima y restaura el volumen al 100% al terminar"
                      >
                        🔉 Atenuación / Ducking (-70% sobre música)
                      </button>
                      <button
                        type="button"
                        className={`btn-wait-mode ${
                          newEventInterruptionMode === 'pause_resume' ? 'active-pause' : ''
                        }`}
                        onClick={() => setNewEventInterruptionMode('pause_resume')}
                        title="Pausa la canción actual en el instante exacto, emite el evento y reanuda la canción exactamente donde se quedó"
                      >
                        ⏸ Pausa y Reanudación (Reanuda donde quedó)
                      </button>
                      <button
                        type="button"
                        className={`btn-wait-mode ${
                          newEventInterruptionMode === 'wait_for_current' ? 'active-wait' : ''
                        }`}
                        onClick={() => setNewEventInterruptionMode('wait_for_current')}
                        title="Encola el evento como siguiente y espera a que termine la canción actual"
                      >
                        ⏳ Espera al Final (Al terminar pista actual)
                      </button>
                    </div>
                  </div>
                </div>

                {/* 3. Días de la Semana */}
                <div className="modal-field">
                  <div className="event-days-header">
                    <label style={{ marginBottom: 0 }}>
                      📆 Días de la Semana:
                    </label>
                    <div className="event-days-presets">
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() => setNewEventDays([0, 1, 2, 3, 4, 5, 6])}
                      >
                        Todos
                      </button>
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() => setNewEventDays([0, 1, 2, 3, 4])}
                      >
                        Lun–Vie
                      </button>
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() => setNewEventDays([5, 6])}
                      >
                        Sáb–Dom
                      </button>
                    </div>
                  </div>
                  <div className="event-days-row">
                    {WEEKDAYS.map((w) => {
                      const active = newEventDays.includes(w.day);
                      return (
                        <button
                          key={w.day}
                          type="button"
                          className={`btn-day-chip ${active ? 'active' : ''}`}
                          onClick={() => handleToggleEventDay(w.day)}
                          title={w.full}
                        >
                          {w.short}
                        </button>
                      );
                    })}
                  </div>
                </div>

                {/* 4. Archivo de Audio, Carpeta, Lista o Comando .time a Reproducir */}
                <div className="modal-field">
                  <label>📂 Archivo, Carpeta, Lista o Comando (.time):</label>
                  <div className="event-file-picker-row">
                    <input
                      type="text"
                      className="event-path-input"
                      placeholder="Archivo .wav/.mp3, lista .m3u8/.json o comando .time..."
                      value={newEventPath}
                      onChange={(e) => setNewEventPath(e.target.value)}
                    />
                    <button
                      type="button"
                      className="btn-primary"
                      onClick={handlePickEventFile}
                    >
                      📂 Examinar...
                    </button>
                    <button
                      type="button"
                      className="btn-time-cmd"
                      onClick={() => setNewEventPath('.time')}
                      title="Programar locución de hora (.time) a esta hora"
                    >
                      🕒 .time
                    </button>
                    <button
                      type="button"
                      className="btn-preset"
                      onClick={() => setNewEventPath(DEFAULT_TEST_WAV)}
                      title="Usar test_tone.wav local para prueba rápida"
                    >
                      ⚡ Tono Prueba
                    </button>
                  </div>
                </div>

                <div className="event-form-actions">
                  <button
                    type="button"
                    onClick={() => setShowNewEventForm(false)}
                  >
                    Cancelar
                  </button>
                  <button type="submit" className="btn-play">
                    ✔ Guardar Evento Programado
                  </button>
                </div>
              </form>
            )}

            {/* Tabla de Eventos Programados */}
            <div className="events-table-container">
              {scheduledEvents.length === 0 ? (
                <div className="playlist-empty">
                  No hay eventos programados todavía. Pulsa{' '}
                  <strong>“➕ Añadir Nuevo Evento”</strong> para automatizar
                  menciones horarias, cuñas o bloques de programación.
                </div>
              ) : (
                <table className="playlist-table events-table">
                  <thead>
                    <tr>
                      <th style={{ width: '75px' }}>Estado</th>
                      <th style={{ width: '80px' }}>Hora</th>
                      <th style={{ width: '130px' }}>Días</th>
                      <th>Archivo / Lista</th>
                      <th style={{ width: '165px' }}>Modo No Destructivo</th>
                      <th style={{ width: '185px', textAlign: 'center' }}>
                        Acciones
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {scheduledEvents.map((ev) => {
                      const isNext = status.next_event?.event_id === ev.id;
                      const effectiveMode: EventInterruptionMode =
                        ev.interruption_mode ??
                        (ev.wait_for_current ? 'wait_for_current' : 'ducking');
                      const modeClass =
                        effectiveMode === 'pause_resume'
                          ? 'mode-pause'
                          : effectiveMode === 'wait_for_current'
                            ? 'mode-wait'
                            : 'mode-duck';
                      return (
                        <tr
                          key={ev.id}
                          className={`playlist-row ${
                            !ev.enabled ? 'event-row-disabled' : ''
                          } ${isNext ? 'current-track' : ''}`}
                        >
                          <td>
                            <button
                              type="button"
                              className={`btn-event-toggle ${
                                ev.enabled ? 'enabled' : 'disabled'
                              }`}
                              onClick={() =>
                                handleToggleEventEnabled(ev.id, !ev.enabled)
                              }
                              title="Activar o desactivar este evento programado"
                            >
                              {ev.enabled ? '● ACTIVO' : '○ PAUSA'}
                            </button>
                          </td>
                          <td className="event-col-time">
                            <strong>{ev.time}:00</strong>
                            {isNext && (
                              <div className="event-next-tag">PRÓXIMO</div>
                            )}
                          </td>
                          <td className="event-col-days">
                            {formatDaysList(ev.days)}
                          </td>
                          <td className="col-title">
                            <div>{ev.title}</div>
                            <div className="col-subpath">{ev.path}</div>
                          </td>
                          <td>
                            <span className={`event-mode-badge ${modeClass}`}>
                              {formatEventInterruptionLabel(
                                ev.interruption_mode,
                                ev.wait_for_current,
                              )}
                            </span>
                          </td>
                          <td className="col-actions">
                            <button
                              type="button"
                              className="btn-play"
                              style={{ padding: '3px 8px', fontSize: '11px' }}
                              onClick={() => handleTriggerEventNow(ev.id)}
                              title="Disparar este evento inmediatamente en la emisión"
                            >
                              ▶ Disparar
                            </button>
                            <button
                              type="button"
                              className="btn-row-cue"
                              onClick={() => handlePlayCuePath(ev.path)}
                              title="Preescuchar archivo del evento por la salida Cue"
                            >
                              🎧 Cue
                            </button>
                            <button
                              type="button"
                              className="btn-row-remove"
                              onClick={() => handleRemoveEvent(ev.id)}
                              title="Eliminar evento programado"
                            >
                              ✕
                            </button>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        </div>
      )}

      {/* 4. Hito 8: Modal del Editor Visual de Puntos de Mezcla (Forma de Onda, Intro, Pisadores y Playhead en Vivo) */}
      {showMarkerModal && (
        <div
          className="modal-backdrop"
          onClick={() => {
            setIsDraggingMarker(false);
            setIsScrubbingPlayhead(false);
            setShowMarkerModal(false);
          }}
          onMouseUp={() => {
            setIsDraggingMarker(false);
            setIsScrubbingPlayhead(false);
          }}
        >
          <div
            className="modal-card modal-waveform-card"
            onClick={(e) => e.stopPropagation()}
            onMouseUp={() => {
              setIsDraggingMarker(false);
              setIsScrubbingPlayhead(false);
            }}
          >
            <div className="modal-header">
              <div>
                <h2>🚩 Editar Marcadores de Mezcla — Forma de Onda, Intro y Pisador</h2>
                <span className="events-modal-sub">
                  {markerData ? (
                    <>
                      Pista: <strong>{markerData.title}</strong> · Duración:{' '}
                      <strong>{formatTimeMs(markerData.duration_ms)}</strong> ·{' '}
                      {markerData.sample_rate} Hz (Persistido en SQLite)
                    </>
                  ) : (
                    'Analizando envolvente de audio...'
                  )}
                </span>
              </div>
              <button
                type="button"
                className="btn-close"
                onClick={() => setShowMarkerModal(false)}
              >
                ✕
              </button>
            </div>

            {isLoadingWaveform || !markerData ? (
              <div className="waveform-loading-box">
                Extrayendo picos PCM en modo streaming sin bloquear la memoria...
              </div>
            ) : (
              <>
                <p className="modal-desc" style={{ marginBottom: '10px' }}>
                  Haz <strong>clic en cualquier punto de la forma de onda</strong>{' '}
                  para saltar (Seek) inmediatamente con el audio de Preescucha (
                  <strong>Cue Deck</strong>), y pulsa{' '}
                  <strong>“🚩 Fijar marcador aquí”</strong> (o la tecla{' '}
                  <kbd>M</kbd> / <kbd>Intro</kbd>) en el instante exacto en que
                  entra la voz del cantante. También puedes arrastrar directamente
                  la bandera amarilla <strong>🚩 INTRO / VOZ</strong>.
                </p>

                {/* Barra de transporte Cue + Playhead en Vivo + Botón 'Fijar marcador aquí' */}
                <div className="waveform-playhead-toolbar">
                  <div className="wf-toolbar-left">
                    {((cueTelemetryRef.current.cue_file === markerData.path &&
                      cueTelemetryRef.current.is_playing) ||
                      (status.cue_file === markerData.path &&
                        status.cue_is_playing)) ? (
                      <button
                        type="button"
                        className="btn-cue-stop"
                        onClick={handleStopCue}
                        title="Detener Preescucha (Cue) [Barra Espaciadora]"
                      >
                        ⏹ Detener Cue
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="btn-row-cue"
                        onClick={() =>
                          handleSeekCueDeck(
                            cuePlayheadMs > 0 &&
                              cuePlayheadMs < markerData.duration_ms - 500
                              ? cuePlayheadMs
                              : 0,
                          )
                        }
                        title="Reproducir en Cue desde la posición actual del Playhead [Barra Espaciadora]"
                      >
                        🎧 Preescuchar aquí
                      </button>
                    )}
                    <button
                      type="button"
                      className="btn-preset"
                      onClick={() => handleSeekCueDeck(0)}
                      title="Saltar al inicio de la pista (00:00.0) en Cue"
                    >
                      ⏮ 00:00.0
                    </button>
                    {markerIntroMs > 0 && (
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() =>
                          handleSeekCueDeck(Math.max(0, markerIntroMs - 3000))
                        }
                        title="Saltar 3 segundos antes del marcador de Intro para verificar la entrada de voz"
                      >
                        🎯 Probar 3s antes de Intro
                      </button>
                    )}
                    <span className="wf-playhead-readout">
                      ▶ PLAYHEAD CUE:{' '}
                      <strong>
                        {formatTimeMs(
                          computeLiveModalPlayheadMs(
                            markerData.path,
                            markerData.duration_ms,
                          ),
                        )}
                      </strong>{' '}
                      / {formatTimeMs(markerData.duration_ms)}
                    </span>
                  </div>

                  <button
                    type="button"
                    className="btn-stamp-intro"
                    onClick={handleSetIntroAtPlayhead}
                    title="Fijar el marcador amarillo de Intro exactamente en el milisegundo actual del Playhead (Atajo: Tecla M o Intro)"
                  >
                    🚩 Fijar marcador aquí [Tecla M / Intro]
                  </button>
                </div>

                {/* Canvas interactivo con la forma de onda y Playhead sincronizado */}
                <div className="waveform-canvas-wrapper">
                  <canvas
                    ref={waveformCanvasRef}
                    className="waveform-canvas"
                    onMouseDown={(e) => {
                      if (
                        e.shiftKey ||
                        isPointerOnIntroFlagHandle(e.clientX, e.clientY)
                      ) {
                        setIsDraggingMarker(true);
                        updateIntroFromCanvasPointer(e.clientX);
                      } else {
                        setIsScrubbingPlayhead(true);
                        const targetMs = getCanvasTimeMsFromClientX(e.clientX);
                        handleSeekCueDeck(targetMs);
                      }
                    }}
                    onMouseMove={(e) => {
                      if (isDraggingMarker) {
                        updateIntroFromCanvasPointer(e.clientX);
                      } else if (isScrubbingPlayhead) {
                        const targetMs = getCanvasTimeMsFromClientX(e.clientX);
                        setCuePlayheadMs(targetMs);
                      }
                    }}
                    onMouseUp={(e) => {
                      if (isScrubbingPlayhead) {
                        const targetMs = getCanvasTimeMsFromClientX(e.clientX);
                        handleSeekCueDeck(targetMs);
                      }
                      setIsDraggingMarker(false);
                      setIsScrubbingPlayhead(false);
                    }}
                    onMouseLeave={() => {
                      setIsDraggingMarker(false);
                      setIsScrubbingPlayhead(false);
                    }}
                  />
                  <div className="waveform-legend-row">
                    <span className="wf-legend-item">
                      <span className="wf-dot playhead" /> Playhead Cue en Vivo (
                      {formatTimeMs(
                        computeLiveModalPlayheadMs(
                          markerData.path,
                          markerData.duration_ms,
                        ),
                      )}{' '}
                      · Clic en la onda para Seek)
                    </span>
                    <span className="wf-legend-item">
                      <span className="wf-dot intro" /> Zona de Intro (00:00.0 →{' '}
                      {formatTimeMs(markerIntroMs)})
                    </span>
                    <span className="wf-legend-item">
                      <span className="wf-dot sweeper" /> Zona de Pisador (
                      {markerSweeperPath
                        ? `${formatTimeMs(
                            Math.max(0, markerIntroMs - markerSweeperDurationMs),
                          )} → ${formatTimeMs(
                            Math.min(
                              markerData.duration_ms,
                              Math.max(0, markerIntroMs - markerSweeperDurationMs) +
                                markerSweeperDurationMs,
                            ),
                          )}`
                        : 'Sin pisador asignado'}
                      )
                    </span>
                    <span className="wf-legend-item">
                      <span className="wf-dot vocal" /> Cuerpo / Voz Cantada
                    </span>
                  </div>
                </div>

                {/* Controles de precisión de Tiempo de Intro + Fórmula de Disparo */}
                <div className="marker-controls-grid">
                  <div className="marker-control-box">
                    <label htmlFor="intro-seconds-input">
                      🚩 Tiempo de Intro (Entrada de Voz):
                    </label>
                    <div className="marker-fine-row">
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() =>
                          setMarkerIntroMs((v) => Math.max(0, v - 1000))
                        }
                      >
                        -1.0s
                      </button>
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() =>
                          setMarkerIntroMs((v) => Math.max(0, v - 500))
                        }
                      >
                        -0.5s
                      </button>
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() =>
                          setMarkerIntroMs((v) => Math.max(0, v - 100))
                        }
                      >
                        -0.1s
                      </button>
                      <input
                        id="intro-seconds-input"
                        type="number"
                        step="0.1"
                        min="0"
                        max={(markerData.duration_ms / 1000).toFixed(1)}
                        className="marker-sec-input"
                        value={(markerIntroMs / 1000).toFixed(1)}
                        onChange={(e) => {
                          const valSec = parseFloat(e.target.value);
                          if (Number.isFinite(valSec)) {
                            setMarkerIntroMs(
                              Math.max(
                                0,
                                Math.min(
                                  markerData.duration_ms,
                                  Math.round(valSec * 1000),
                                ),
                              ),
                            );
                          }
                        }}
                      />
                      <span className="marker-sec-unit">seg</span>
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() =>
                          setMarkerIntroMs((v) =>
                            Math.min(markerData.duration_ms, v + 100),
                          )
                        }
                      >
                        +0.1s
                      </button>
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() =>
                          setMarkerIntroMs((v) =>
                            Math.min(markerData.duration_ms, v + 500),
                          )
                        }
                      >
                        +0.5s
                      </button>
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() =>
                          setMarkerIntroMs((v) =>
                            Math.min(markerData.duration_ms, v + 1000),
                          )
                        }
                      >
                        +1.0s
                      </button>
                      <button
                        type="button"
                        className="btn-preset"
                        onClick={() => setMarkerIntroMs(0)}
                        title="Reiniciar Intro a 00:00.0"
                      >
                        0.0s
                      </button>
                    </div>
                  </div>

                  {/* Tarjeta de sincronización matemática del Pisador */}
                  <div className="marker-formula-box">
                    <div className="formula-header">
                      ⏱ Sincronización Exacta del Pisador (Sweeper Engine)
                    </div>
                    {markerSweeperPath ? (
                      <div className="formula-equation">
                        <span>
                          Intro{' '}
                          <strong>{(markerIntroMs / 1000).toFixed(1)}s</strong>
                        </span>
                        <span> − </span>
                        <span>
                          Pisador{' '}
                          <strong>
                            {(markerSweeperDurationMs / 1000).toFixed(1)}s
                          </strong>
                        </span>
                        <span> = </span>
                        <span className="formula-result">
                          Disparo exacto en{' '}
                          <strong>
                            {(
                              Math.max(0, markerIntroMs - markerSweeperDurationMs) /
                              1000
                            ).toFixed(1)}
                            s
                          </strong>{' '}
                          ({formatTimeMs(Math.max(0, markerIntroMs - markerSweeperDurationMs))})
                        </span>
                      </div>
                    ) : (
                      <div className="formula-empty">
                        Sin pisador asignado a esta pista. Selecciona un archivo o
                        un Jingle abajo para calcular el disparo automático.
                      </div>
                    )}
                  </div>
                </div>

                {/* Selector de Pisador (Sweeper) en el modal */}
                <div className="modal-field" style={{ marginTop: '12px' }}>
                  <label>🎙 Pisador (Sweeper) asignado a esta canción:</label>
                  <div className="sweeper-picker-row">
                    <div className="sweeper-current-pill">
                      {markerSweeperPath ? (
                        <>
                          <strong>
                            🎙 {markerSweeperTitle ?? markerSweeperPath.split('/').pop()}
                          </strong>{' '}
                          <span className="sweeper-dur-sub">
                            ({formatTimeMs(markerSweeperDurationMs)})
                          </span>
                        </>
                      ) : (
                        <span style={{ color: '#94a3b8' }}>
                          Ningún pisador asignado
                        </span>
                      )}
                    </div>
                    <button
                      type="button"
                      className="btn-primary"
                      onClick={handlePickSweeperInModal}
                    >
                      📂 Elegir Archivo de Pisador...
                    </button>
                    {markerSweeperPath && (
                      <>
                        <button
                          type="button"
                          className="btn-row-cue"
                          onClick={() => handlePlayCuePath(markerSweeperPath)}
                          title="Preescuchar el Pisador en Cue"
                        >
                          🎧 Cue Pisador
                        </button>
                        <button
                          type="button"
                          className="btn-danger-outline"
                          onClick={() => {
                            setMarkerSweeperPath(null);
                            setMarkerSweeperTitle(null);
                            setMarkerSweeperDurationMs(0);
                          }}
                        >
                          ✕ Quitar Pisador
                        </button>
                      </>
                    )}
                  </div>

                  {/* Selección rápida desde la Botonera de Jingles (1-9) */}
                  {jingleSlots.some((j) => Boolean(j.path)) && (
                    <div className="sweeper-jingle-quick-row">
                      <span className="sweeper-quick-label">
                        O usar un Jingle de la Cart Machine:
                      </span>
                      {jingleSlots
                        .filter((j) => Boolean(j.path))
                        .map((j) => (
                          <button
                            key={`modal-jingle-sw-${j.slot}`}
                            type="button"
                            className={`btn-preset ${
                              markerSweeperPath === j.path ? 'active' : ''
                            }`}
                            onClick={() => handleSelectJingleAsSweeperInModal(j)}
                          >
                            [{j.slot}] {j.title} ({(j.duration_ms / 1000).toFixed(1)}s)
                          </button>
                        ))}
                    </div>
                  )}
                </div>

                <div className="modal-actions" style={{ marginTop: '16px' }}>
                  <div style={{ display: 'flex', gap: '8px' }}>
                    <button
                      type="button"
                      className="btn-row-cue"
                      onClick={() => handlePlayCuePath(markerData.path)}
                    >
                      🎧 Escuchar Canción en Cue
                    </button>
                    {status.cue_is_playing && (
                      <button
                        type="button"
                        className="btn-cue-stop"
                        onClick={handleStopCue}
                      >
                        ⏹ Detener Cue
                      </button>
                    )}
                  </div>
                  <div style={{ display: 'flex', gap: '8px' }}>
                    <button
                      type="button"
                      onClick={() => setShowMarkerModal(false)}
                    >
                      Cancelar
                    </button>
                    <button
                      type="button"
                      className="btn-play"
                      onClick={handleSaveTrackMarkers}
                    >
                      💾 Guardar Marcadores (SQLite)
                    </button>
                  </div>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
