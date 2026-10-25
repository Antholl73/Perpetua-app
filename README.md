# ZaraStudio 2.0 (Reconstrucción Moderna Multiplataforma — Linux & Windows)

Reconstrucción moderna, desacoplada y multiplataforma (**Linux** sobre PipeWire/ALSA y **Windows** sobre WASAPI/ASIO) de **ZaraRadio 1.6.2**, diseñada a partir de la evidencia de ingeniería inversa obtenida con **REA (`reverse-engineer-anything`)**.

## Estructura del Monorepo

- [`Cargo.toml`](file:///home/anthony/zara-radio-app/Cargo.toml) — Workspace de Rust.
- [`crates/zara-formats/`](file:///home/anthony/zara-radio-app/crates/zara-formats/src/lib.rs) — Parsers y serializadores 100% compatibles con ZaraRadio 1.6.2:
  - [`.lst`](file:///home/anthony/zara-radio-app/crates/zara-formats/src/lst.rs) (con sufijos de solapamiento `~N` y las 13+ variantes polimórficas de `File`: `.time`, `.temperature`, `.humidity`, `.dir`, `%d.pause`, `%d.satellite`, `.satcon`, `.satdis`, `.dtmfon`, `.dtmfoff`, `.stop`, `.play`, `%s*%d.stream`, `.rot`, `.seq`).
  - [`.evt`](file:///home/anthony/zara-radio-app/crates/zara-formats/src/evt.rs) (formato binario `EVT01`, `EVT02` y `EVT03`).
  - [`.rot`, `.seq`, `.pls`, `.zsd` y `.m3u`](file:///home/anthony/zara-radio-app/crates/zara-formats/src/ini_formats.rs).
  - [`sweepers.db` y `.zsl`](file:///home/anthony/zara-radio-app/crates/zara-formats/src/sweeper_db.rs) (base de datos SQLite3 de pisadores).
- [`crates/zara-audio/`](file:///home/anthony/zara-radio-app/crates/zara-audio/src/lib.rs) — Motor de audio multitarjeta (`cpal`), [motor de fundido `Fader`](file:///home/anthony/zara-radio-app/crates/zara-audio/src/fader.rs), [procesador DSP de Compresor/AGC y detector de silencio final](file:///home/anthony/zara-radio-app/crates/zara-audio/src/dsp.rs) y [decodificador de tonos DTMF por FFT de 2048 muestras](file:///home/anthony/zara-radio-app/crates/zara-audio/src/dtmf.rs).
- [`crates/zara-core/`](file:///home/anthony/zara-radio-app/crates/zara-core/src/lib.rs) — Coordinador central [`StudioEngine`](file:///home/anthony/zara-radio-app/crates/zara-core/src/lib.rs) (`Secuenciador`), [`EventScheduler`](file:///home/anthony/zara-radio-app/crates/zara-core/src/scheduler.rs) (`GestionEventos` con colas prioritarias `immediate_queue` y `delayed_queue` y expiración `maxWait`), [`AntiRepeatPool`](file:///home/anthony/zara-radio-app/crates/zara-core/src/random_pool.rs) (`\random\tabla.txt`, `data%d.txt`, `antirepeat%d.txt`) y [locuciones horarias/meteorológicas](file:///home/anthony/zara-radio-app/crates/zara-core/src/announcements.rs).
- [`crates/zara-server/`](file:///home/anthony/zara-radio-app/crates/zara-server/src/lib.rs) y [`crates/zara-desktop/`](file:///home/anthony/zara-radio-app/crates/zara-desktop/src/main.rs) — Servidor API REST + WebSocket (`axum` en puerto `4850`) para control remoto web/modo *headless* y puente IPC de escritorio.
- [`ui/`](file:///home/anthony/zara-radio-app/ui/src/App.tsx) — Interfaz gráfica moderna en React 19 + TypeScript + Vite con sistema de temas **Broadcast Studio Dark** y **Daylight Studio Light**, vúmetros estéreo dBFS + EBU R128 LUFS, lista con Drag & Drop y cálculo de Hora Estimada (ETA), botonera instantánea de Jingles (`1`–`9`) y reproductor Cue/PFL.

## Requisitos en Arch Linux

Las librerías nativas (`alsa-lib`, `sqlite`, `webkit2gtk-4.1`, `gtk3`) y `nodejs`/`npm` ya se encuentran instaladas en el sistema. Para compilar los crates de Rust, instala el toolchain oficial desde los repositorios de Arch Linux:

```bash
sudo pacman -S rust
```

## Ejecución y Pruebas

### 1. Ejecutar las pruebas unitarias de formatos y motor en Rust
```bash
cargo test
```

### 2. Iniciar el servidor Core / API WebSocket (`http://localhost:4850`)
```bash
cargo run -p zara-server
```

### 3. Iniciar la Interfaz Gráfica (`http://localhost:5173`)
```bash
cd ui
npm install
npm run dev
```
*(La interfaz se conecta automáticamente por WebSocket a `ws://localhost:4850/ws` si `zara-server` está activo, o funciona de forma autónoma con el simulador de cabina integrado en [`studioClient.ts`](file:///home/anthony/zara-radio-app/ui/src/store/studioClient.ts)).*
