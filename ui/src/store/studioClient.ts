import type {
  PlaylistRowView,
  ScheduledEvent,
  StudioSnapshot,
  SweeperRecord,
  TrackItem,
  TrackKind,
} from '../types.ts';

/**
 * Clasifica la cadena de recurso tal como lo hace `perpetua-formats::TrackItem::from_raw_spec`
 * (incluyendo el sufijo de solapamiento `~%d` descubierto en `0x004843d0`).
 */
export function parseRawTrackSpec(durationMs: number, rawSpec: string): TrackItem {
  const trimmed = rawSpec.trim();
  let cleanPath = trimmed;
  let customOverlapSec: number | null = null;

  const tildeIdx = trimmed.lastIndexOf('~');
  if (tildeIdx > 0) {
    const suffix = Number(trimmed.slice(tildeIdx + 1));
    if (Number.isInteger(suffix) && suffix >= 0) {
      cleanPath = trimmed.slice(0, tildeIdx);
      customOverlapSec = suffix;
    }
  }

  const lower = cleanPath.toLowerCase();
  let kind: TrackKind;
  let displayTitle = cleanPath;

  if (lower === '.time') {
    kind = { type: 'time_announcement' };
    displayTitle = 'Locución de Hora (.time)';
  } else if (lower === '.temperature') {
    kind = { type: 'temperature_announcement' };
    displayTitle = 'Locución de Temperatura (.temperature)';
  } else if (lower === '.humidity') {
    kind = { type: 'humidity_announcement' };
    displayTitle = 'Locución de Humedad (.humidity)';
  } else if (lower === '.satcon') {
    kind = { type: 'satellite_connect_cmd' };
    displayTitle = 'Conectar Satélite (.satcon)';
  } else if (lower === '.satdis') {
    kind = { type: 'satellite_disconnect_cmd' };
    displayTitle = 'Desconectar Satélite (.satdis)';
  } else if (lower === '.dtmfon') {
    kind = { type: 'dtmf_enable_cmd' };
    displayTitle = 'Activar Detector DTMF (.dtmfon)';
  } else if (lower === '.dtmfoff') {
    kind = { type: 'dtmf_disable_cmd' };
    displayTitle = 'Desactivar Detector DTMF (.dtmfoff)';
  } else if (lower === '.stop') {
    kind = { type: 'stop_command' };
    displayTitle = 'Comando Detener (.stop)';
  } else if (lower === '.play') {
    kind = { type: 'play_command' };
    displayTitle = 'Comando Reproducir (.play)';
  } else if (lower.endsWith('.pause')) {
    const secs = parseInt(cleanPath.slice(0, -'.pause'.length), 10) || 5;
    kind = { type: 'pause', seconds: secs };
    displayTitle = `Pausa (${secs} s)`;
  } else if (lower.endsWith('.satellite')) {
    const secs = parseInt(cleanPath.slice(0, -'.satellite'.length), 10) || 60;
    kind = { type: 'satellite_connection', seconds: secs };
    displayTitle = `Conexión Satélite (${secs} s)`;
  } else if (lower.endsWith('.stream')) {
    const body = cleanPath.slice(0, -'.stream'.length);
    const star = body.lastIndexOf('*');
    const url = star >= 0 ? body.slice(0, star) : body;
    const secs = star >= 0 ? parseInt(body.slice(star + 1), 10) || 60 : 60;
    kind = { type: 'internet_stream', url, seconds: secs };
    displayTitle = `Stream: ${url} (${secs} s)`;
  } else if (lower.endsWith('.dir')) {
    const dirPath = cleanPath.slice(0, -'.dir'.length);
    kind = { type: 'random_directory', dir_path: dirPath };
    const folder = dirPath.split(/[\\/]/).filter(Boolean).pop() || dirPath;
    displayTitle = `Aleatoria sin repetición: ${folder}`;
  } else if (lower.endsWith('.rot')) {
    kind = { type: 'rotation', path: cleanPath };
    displayTitle = cleanPath.split(/[\\/]/).pop()?.replace(/\.rot$/i, '') || cleanPath;
  } else if (lower.endsWith('.seq')) {
    kind = { type: 'sequence', path: cleanPath };
    displayTitle = cleanPath.split(/[\\/]/).pop()?.replace(/\.seq$/i, '') || cleanPath;
  } else {
    kind = { type: 'audio_file', path: cleanPath };
    const file = cleanPath.split(/[\\/]/).pop() || cleanPath;
    displayTitle = file.replace(/\.[^.]+$/, '');
  }

  return {
    id: `trk-${Math.random().toString(36).slice(2, 10)}`,
    duration_ms: durationMs,
    kind,
    custom_overlap_sec: customOverlapSec,
    display_title: displayTitle,
    loudness_lufs: -16.0,
  };
}

export function trackToRawSpec(item: TrackItem): string {
  const k = item.kind;
  let base = '';
  switch (k.type) {
    case 'audio_file':
      base = k.path;
      break;
    case 'random_directory':
      base = `${k.dir_path}.dir`;
      break;
    case 'time_announcement':
      base = '.time';
      break;
    case 'temperature_announcement':
      base = '.temperature';
      break;
    case 'humidity_announcement':
      base = '.humidity';
      break;
    case 'pause':
      base = `${k.seconds}.pause`;
      break;
    case 'satellite_connection':
      base = `${k.seconds}.satellite`;
      break;
    case 'satellite_connect_cmd':
      base = '.satcon';
      break;
    case 'satellite_disconnect_cmd':
      base = '.satdis';
      break;
    case 'dtmf_enable_cmd':
      base = '.dtmfon';
      break;
    case 'dtmf_disable_cmd':
      base = '.dtmfoff';
      break;
    case 'stop_command':
      base = '.stop';
      break;
    case 'play_command':
      base = '.play';
      break;
    case 'internet_stream':
      base = `${k.url}*${k.seconds}.stream`;
      break;
    case 'rotation':
      base = k.path;
      break;
    case 'sequence':
      base = k.path;
      break;
  }
  return item.custom_overlap_sec !== null ? `${base}~${item.custom_overlap_sec}` : base;
}

export function serializeLstContent(rows: PlaylistRowView[]): string {
  const lines = [`${rows.length}`];
  for (const r of rows) {
    lines.push(`${r.item.duration_ms}\t${trackToRawSpec(r.item)}`);
  }
  return lines.join('\n') + '\n';
}

export function createInitialSnapshot(): StudioSnapshot {
  const rawTracks: Array<[number, string]> = [
    [214000, 'C:\\Musica\\Pop\\The Weeknd - Blinding Lights.mp3~3'],
    [198000, 'C:\\Musica\\Rock\\Soda Stereo - De Musica Ligera.flac~2'],
    [2500, '.time'],
    [2200, '.temperature'],
    [185000, 'C:\\Musica\\Exitos_80s.dir'],
    [30000, 'C:\\Spots\\Bloque_Comercial_Tarde.rot'],
    [242000, 'C:\\Musica\\Indie\\Tame Impala - Let It Happen.opus~4'],
    [60000, 'http://icecast.emisora.local:8000/boletin.ogg*60.stream'],
  ];

  const items = rawTracks.map(([dur, spec]) => parseRawTrackSpec(dur, spec));
  const now = new Date();

  const programmedEvents: ScheduledEvent[] = [
    {
      id: 'evt-hora',
      immediate: false,
      day_hour_sec: 0,
      day_mask: [true, true, true, true, true, true, true],
      track: parseRawTrackSpec(2500, '.time'),
      start_date: '01/01/26 00:00:00',
      expiration_date: '12/31/30 23:59:59',
      expires: false,
      priority: 'high',
      hour_bits: 0x00ffffff,
      enabled: true,
      max_wait_minutes: 5,
    },
    {
      id: 'evt-temp-30',
      immediate: false,
      day_hour_sec: 1800,
      day_mask: [true, true, true, true, true, true, true],
      track: parseRawTrackSpec(2200, '.temperature'),
      start_date: '01/01/26 00:00:00',
      expiration_date: '12/31/30 23:59:59',
      expires: false,
      priority: 'low',
      hour_bits: 0x00ffffff,
      enabled: true,
      max_wait_minutes: 10,
    },
    {
      id: 'evt-sat-informativo',
      immediate: true,
      day_hour_sec: 12 * 3600,
      day_mask: [false, true, true, true, true, true, false],
      track: parseRawTrackSpec(300000, '300.satellite'),
      start_date: '01/01/26 00:00:00',
      expiration_date: '12/31/30 23:59:59',
      expires: false,
      priority: 'high',
      hour_bits: 0,
      enabled: true,
      max_wait_minutes: -1,
    },
  ];

  const slots = Array.from({ length: 90 }, (_, idx) => ({
    index: idx,
    title: `Cuña ${(idx % 9) + 1}`,
    file_path: '',
    loop_enabled: false,
    sweeper_ducking: true,
  }));

  const page0Presets: Array<[number, string, string, boolean, boolean]> = [
    [0, 'ID Hora Oficial', 'C:\\Jingles\\ID_Estacion_Hora.wav', false, true],
    [1, 'Separador 104.5 FM', 'C:\\Jingles\\Separador_FM.flac', false, true],
    [2, 'Ráfaga Informativo', 'C:\\Jingles\\Rafaga_Noticias.mp3', false, false],
    [3, 'Cortina Fondo (Loop)', 'C:\\Jingles\\Cortina_Fondo.ogg', true, false],
    [4, 'Flash Deportivo', 'C:\\Jingles\\Deportes_Flash.mp3', false, true],
    [5, 'Aplausos Cabina', 'C:\\Jingles\\FX_Aplausos.wav', false, true],
    [6, 'Alerta Última Hora', 'C:\\Jingles\\FX_Urgente.wav', false, true],
    [7, 'Sello El Tiempo', 'C:\\Jingles\\Sello_Meteo.mp3', false, true],
    [8, 'Cierre de Bloque', 'C:\\Jingles\\Cierre_Bloque.flac', false, true],
  ];

  for (const [idx, title, path, loop, sw] of page0Presets) {
    slots[idx] = {
      index: idx,
      title,
      file_path: path,
      loop_enabled: loop,
      sweeper_ducking: sw,
    };
  }

  const sweepers: SweeperRecord[] = [
    {
      target_song_file: '*',
      sweeper_file: 'C:\\Pisadores\\Locucion_General_Estacion.mp3',
      intro_seconds: 8,
      enabled: true,
    },
    {
      target_song_file: 'C:\\Musica\\Pop\\The Weeknd - Blinding Lights.mp3',
      sweeper_file: 'C:\\Pisadores\\Exclusivo_Blinding_Lights.wav',
      intro_seconds: 11,
      enabled: true,
    },
  ];

  const baseSnapshot: StudioSnapshot = {
    station_name: 'Perpetua — 104.5 FM Cabina Aire',
    clock_display: now.toTimeString().slice(0, 8),
    date_display: now.toISOString().slice(0, 10),
    transport: 'playing',
    mode: 'repeat',
    stop_after_current: false,
    events_enabled: true,
    sweepers_enabled: true,
    dtmf_enabled: true,
    satellite_connected: false,
    current_track: items[0],
    current_index: 0,
    elapsed_ms: 42300,
    remaining_ms: items[0].duration_ms - 42300,
    active_sweeper: null,
    next_track: items[1],
    playlist: [],
    programmed_events: programmedEvents,
    immediate_queue: [],
    delayed_queue: [
      {
        event: programmedEvents[0],
        triggered_at_epoch_sec: Math.floor(now.getTime() / 1000) - 25,
        triggered_at_display: now.toTimeString().slice(0, 8),
      },
    ],
    jingles: {
      page_names: [
        '1. Identificativos',
        '2. Efectos Cabina',
        '3. Informativos',
        '4. Deportes',
        '5. Concursos',
        '6. Especiales',
        '7. Madrugada',
        '8. Fin de Semana',
        '9. Institucional',
        '10. Reserva',
      ],
      slots,
      active_page: 0,
    },
    sweepers,
    fader_settings: {
      fade_length_ms: 4000,
      fade_overlap_ms: 2500,
      sweeper_duck_volume_pct: 25,
      silence_detector_enabled: true,
      silence_threshold_db: -42.0,
      agc_compressor_enabled: true,
      target_lufs: -16.0,
      curve: 'equal_power',
    },
    routing_config: {
      air_device: 'PipeWire / WASAPI — Salida Principal Aire (Estéreo 1/2)',
      cue_device: 'PipeWire / WASAPI — Preescucha PFL Auriculares (Estéreo 3/4)',
      sweeper_device: 'PipeWire / WASAPI — Salida Principal Aire (Estéreo 1/2)',
      jingles_device: 'PipeWire / WASAPI — Salida Dedicada Consola (Estéreo 5/6)',
      satellite_input_device: 'Line-In / Captura Satélite + Decodificador DTMF FFT',
      aux_devices: [
        'Salida Principal Aire',
        'Salida Principal Aire',
        'Salida Principal Aire',
        'Salida Principal Aire',
      ],
    },
    weather_time: {
      base_resources_dir: '.',
      time_mode: 'twenty_four_hours',
      temperature_celsius: 23,
      humidity_percent: 56,
      weather_html_path: 'weather.html',
    },
    vu_telemetry: {
      left_db: -6.2,
      right_db: -5.8,
      peak_left_db: -2.0,
      peak_right_db: -1.8,
      agc_gain_reduction_db: -1.6,
      integrated_lufs: -15.9,
    },
    recent_logs: [
      {
        timestamp: now.toTimeString().slice(0, 8),
        level: 'info',
        message: 'Application started (Perpetua Broadcast Core v0.1.0-alpha)',
      },
      {
        timestamp: now.toTimeString().slice(0, 8),
        level: 'info',
        message: 'Loaded 8 playlist entries (.lst) and 3 scheduled events (EVT03)',
      },
    ],
  };

  return recomputeDerivedPlaylist(baseSnapshot, items);
}

export function recomputeDerivedPlaylist(
  snap: StudioSnapshot,
  rawItems?: TrackItem[],
): StudioSnapshot {
  const items = rawItems ?? snap.playlist.map((r) => r.item);
  const curIdx = snap.current_index ?? 0;
  const currentTrack = items[curIdx] ?? null;
  const currentDur = currentTrack ? Math.max(currentTrack.duration_ms, 2500) : 0;
  const remainingMs = Math.max(0, currentDur - snap.elapsed_ms);

  const nextIdx =
    items.length === 0
      ? null
      : snap.mode === 'repeat'
        ? (curIdx + 1) % items.length
        : curIdx + 1 < items.length
          ? curIdx + 1
          : null;

  let cumulativeMs = remainingMs;
  const nowMs = Date.now();
  const defaultOverlapMs = snap.fader_settings.fade_overlap_ms;

  const playlist: PlaylistRowView[] = items.map((item, idx) => {
    let etaStr = '—';
    if (idx === curIdx) {
      etaStr = new Date(nowMs).toTimeString().slice(0, 8);
    } else if (idx > curIdx) {
      const etaDate = new Date(nowMs + cumulativeMs);
      etaStr = etaDate.toTimeString().slice(0, 8);
      const overlapMs =
        item.custom_overlap_sec !== null
          ? item.custom_overlap_sec * 1000
          : defaultOverlapMs;
      const effectiveDur = Math.max(1000, Math.max(item.duration_ms, 2500) - overlapMs);
      cumulativeMs += effectiveDur;
    }

    return {
      index: idx,
      item,
      estimated_air_time: etaStr,
      is_playing: idx === curIdx,
      is_next: idx === nextIdx,
    };
  });

  return {
    ...snap,
    current_track: currentTrack,
    remaining_ms: remainingMs,
    next_track: nextIdx !== null ? items[nextIdx] : null,
    playlist,
  };
}
