<p align="center">
  <img src="./docs/img/splash.gif" width="400" alt="Dark Hub boot screen on the cooler display" />
</p>

<h1 align="center">Kalkan-RS</h1>

<p align="center">
  Lightweight panel and driver, written in Rust, for the display of the <b>Kalkan Aura LCD 240</b> water cooler.<br/>
  Replaces the official software (WPF, 400 MB+ of RAM) with a ~22 MB native service and a canvas-style theme editor.
</p>

<p align="center">
  <a href="https://github.com/Pokeralho/kalkan-rs/releases/latest"><img src="https://img.shields.io/github/v/release/Pokeralho/kalkan-rs?style=flat-square&label=release" alt="Latest release" /></a>
  <a href="https://github.com/Pokeralho/kalkan-rs/releases"><img src="https://img.shields.io/github/downloads/Pokeralho/kalkan-rs/total?style=flat-square" alt="Downloads" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT license" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011%20x64-informational?style=flat-square" alt="Windows 10/11 x64" />
  <img src="https://img.shields.io/badge/rust-stable-orange?style=flat-square" alt="Rust stable" />
</p>

<p align="center">
  <b>English</b> · <a href="./README.pt-BR.md">Português</a>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#features">Features</a> ·
  <a href="#themes">Themes</a> ·
  <a href="#kalkan-studio">Studio</a> ·
  <a href="#configuration">Configuration</a> ·
  <a href="#performance">Performance</a> ·
  <a href="#building-from-source">Build</a>
</p>

<p align="center">
  <img src="./docs/img/studio.png" width="94%" alt="Kalkan Studio editing the Dark Hub theme" />
</p>

---

## Hardware

Built for the **Kalkan Aura LCD 240** water cooler, whose pump block carries a 320×240 display
(USB `VID 0x1B80` / `PID 0xB550`, exposed by Windows as a serial COM port). The same display and protocol are used
by GAMDIAS ZEUS CAST software, so other coolers that ship with the same `LcdComm.dll` should work too, but only the
Kalkan Aura LCD 240 has been tested.

## Install

1. Download `Kalkan-RS-x.y.z-win-x64.zip` from the [latest release](https://github.com/Pokeralho/kalkan-rs/releases/latest)
   and extract it anywhere (for example `C:\Tools\Kalkan-RS`).
2. Keep the **official Kalkan software installed** but **closed**. Kalkan-RS talks to the display through the vendor's
   `LcdComm.dll` (looked up in `C:\Program Files\KALKAN\`), which is not redistributed here, and two programs cannot
   share the same COM port.
3. Run `kalkan-rs.exe`. An icon appears in the system tray; the display shows the boot screen and then the active theme.

To start with Windows, tick **Iniciar com o Windows** in the tray menu (or in Studio, Display tab), or run
`kalkan-rs --autostart on`. It adds an entry for your user only (`HKCU\...\Run`, no administrator rights) that
always points to the executable you are running: if you move the folder, the path is fixed the next time you open it.
If the official Kalkan software also starts with Windows, disable it there, since both cannot use the display at once.

Requirements: Windows 10 or 11, 64-bit. Kalkan Studio uses WebView2, which ships with Windows 11 (on Windows 10,
install the [Evergreen runtime](https://developer.microsoft.com/microsoft-edge/webview2/) if Studio does not open).

---

## Features

### Display service
- **Light**: ~22 MB of RAM and ~0.04% CPU with static themes. No .NET, no bundled ffmpeg.
- **Driver**: triple-buffered frames that never block rendering, keep-alive, automatic reconnection when the USB
  cable comes back, and identical frames are not resent.
- **Tray menu**: themes, brightness, max FPS, rotation, flip/mirror, silent mode, automatic music mode, start with
  Windows, reload and a shortcut to Kalkan Studio. Changes made by Studio to `config.toml` are applied within a second.
- **Boot screen**: an animated Dark Hub snowflake. It is a regular theme and can be edited or replaced.

### Music mode
- **Now Playing**: album art, title, artist, elapsed time, duration, progress bar and **synced lyrics**
  (karaoke style, smooth scrolling) from [LRCLIB](https://lrclib.net).
- **Adaptive look**: the background can be the blurred album art, and colors extracted from the cover
  (`$album_vibrant`, `$album_light`, ...) recolor the whole theme on every track.
- **Automatic**: when something starts playing (Spotify, browsers, any app on the Windows media session) the display
  switches to the music theme, and goes back a few seconds after you pause. Can be turned off.

### Telemetry
- **CPU**: total and per-core usage, effective clock, temperature, power.
- **GPU**: temperature, usage, clock, VRAM and fan through D3DKMT (NVIDIA, AMD and Intel, no vendor library loaded).
  NVML is optional for power draw and fan percentage.
- **Memory, disk, network**, media session and weather ([Open-Meteo](https://open-meteo.com), off by default).
- Values that Windows does not expose without a kernel driver (Tdie, CPU watts, GPU hotspot) are read from
  LibreHardwareMonitor when it is running, and shown as unavailable otherwise, never estimated.

### Media backgrounds
- MP4 / H.264 / HEVC decoded by Windows Media Foundation, animated GIF / WebP / APNG cached in memory,
  static images, solid colors and album art.

---

## Themes

A theme is a folder in `assets/themes/` with a `theme.toml`. Saving the file updates the display immediately.

| Now Playing | Dark Hub | Cyberpunk |
|:-:|:-:|:-:|
| ![](./docs/img/now_playing.gif) | ![](./docs/img/theme_dark_hub.png) | ![](./docs/img/cyberpunk.gif) |
| **Minimal Clean** | **Neon Shapes** | **Boot screen** |
| ![](./docs/img/theme_minimal_stats.png) | ![](./docs/img/theme_neon_shapes.png) | ![](./docs/img/theme_splash.png) |

<sub>Now Playing images were generated with `KALKAN_DEMO_MEDIA=1` (fictional track, cover and lyrics).</sub>

- **Layers**: text, radial gauges, bars, sparklines, per-core grid, free-form shapes (polygons, stars, ellipses, lines,
  radial repetition), images, album art and synced lyrics.
- **Expressions** in placeholders and metrics: `{cpu_temp*1.8+32:.0}°F`, `ram_used_gb / ram_total_gb * 100`,
  with functions such as `max`, `clamp` and `if`.
- **Behaviors on any layer**: `visible_if` conditions, `color_rules`, animations (pulse, blink, spin, marquee,
  fade, slide), glow and layer opacity.
- **Palette**: named colors used as `$accent` anywhere, plus the colors extracted from the playing album.

```toml
[[widgets.text]]
text = "{cpu_temp:.0}°C"
x = 160
y = 120
align = "center"
color_hex = "$accent"
color_rules = [{ when = "cpu_temp >= 85", color = "#FF3B5C" }]
glow = "$accent/40"
```

Full reference (in Portuguese): [docs/THEMES.md](./docs/THEMES.md).

## Kalkan Studio

A visual editor (Tauri 2) opened from the tray. The canvas is drawn by the same engine that feeds the display,
with live telemetry, so what you see is exactly what the cooler shows.

- Move and resize on the canvas with magnetic guides, edit shape points, reorder, hide and lock layers, undo/redo.
- Inspector with color picker (alpha), gradient editor, placeholder chips and system font search.
- Ready-made blocks: full music player, clock, CPU/GPU cards, network graph, liquid gauge.
- Display settings: brightness, FPS, orientation, boot screen, music mode, lyrics, GPU backend.

<p align="center">
  <img src="./docs/img/studio_now_playing.png" width="94%" alt="Kalkan Studio editing the Now Playing theme" />
</p>

Guide (in Portuguese): [docs/STUDIO.md](./docs/STUDIO.md).

---

## Configuration

`config.toml` is created next to the executable. See [config.example.toml](./config.example.toml) for every option.

| Key | Default | Description |
|---|---|---|
| `active_theme` | `"dark_hub"` | Theme folder in `assets/themes/` |
| `autostart` | `false` | Start with Windows (current user, no admin) |
| `brightness` | `80` | Panel brightness (0–100) |
| `max_fps` | `24` | 1–30. Animated backgrounds cost CPU in proportion to FPS |
| `flip_vertical` / `flip_horizontal` | `true` / `true` | Orientation (180° matches the Kalkan Aura LCD 240 mounting) |
| `music_mode` / `music_theme` | `true` / `"now_playing"` | Switch to the music theme while something plays |
| `lyrics` | `true` | Fetch synced lyrics from LRCLIB (sends title, artist, album and duration) |
| `splash` / `splash_ms` | `true` / `2600` | Boot screen and its duration |
| `gpu_backend` | `"auto"` | `auto` (D3DKMT), `nvml` (adds power and fan %, ~19 MB more RAM) or `pdh` |

Command line: `--theme <name>`, `--no-tray`, `--autostart on|off`, `--preview <theme> [out.png]`, `--record <theme> <out.gif> [s]`,
`--list-themes`, `--verbose`.

## Performance

Measured on a 16-thread Ryzen with an RTX 5060.

| Scenario | Total CPU | Private RAM |
|---|---|---|
| Static theme (clock, telemetry) | ~0.04% | ~19 MB |
| Music mode (album art, player, lyrics) | ~0.1% | ~23 MB |
| Animated background, 24 FPS | ~0.3–0.45% | ~22 MB |

About 85% of the CPU spent on animated backgrounds is the Windows USB serial driver (`usbser.sys`) moving
~3 MB/s, which is why FPS is configurable. The reverse-engineered protocol is documented in
[docs/PROTOCOL.md](./docs/PROTOCOL.md).

---

## Building from source

Requires stable [Rust](https://rustup.rs) with the MSVC toolchain.

```bash
cargo build --release --workspace
cargo test --release -p kalkan-rs
```

This produces `target\release\kalkan-rs.exe` (service) and `target\release\kalkan-studio.exe` (editor). Keep both
next to the `assets\` folder. Architecture notes: [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md).
Changes per version: [CHANGELOG.md](./CHANGELOG.md).

## Disclaimer

Independent project, not affiliated with Kalkan or GAMDIAS. It does not include or redistribute any vendor file;
it uses the `LcdComm.dll` already installed by the official software. Use at your own risk.

Released under the [MIT license](./LICENSE).
