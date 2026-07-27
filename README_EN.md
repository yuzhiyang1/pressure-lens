# Pressure Lens

[中文](./README.md) | [English](./README_EN.md)

Pressure Lens is a local-first cognitive workload visualizer for Windows. It
combines typing intensity, modifier-key ratio, application switching,
continuous activity, and Agent context usage into an explainable view of your
workload throughout the day.

> Status: experimental Windows MVP. Its pressure score reflects work patterns;
> it is not a medical or mental-health assessment.

## What it does

- Shows current pressure, contributing factors, Agent context usage, and daily
  trends in a dashboard.
- Automatically collects global keyboard activity, foreground-app switches,
  and continuous activity on Windows without recording typed characters.
- Reads structured token metrics from local Codex sessions without reading
  conversation content.
- Displays a transparent, always-on-top, click-through WebGL black hole.
- Uses live desktop sampling for local gravitational lensing and rejects stale
  frames while dragging to prevent trails.
- Controls black-hole visibility and position through the tray, a shortcut, or
  a hover interaction.

## Black-hole visuals

The desktop black hole is rendered by a transparent WebGL2 shader. Its
Schwarzschild photon paths, thin accretion-disk crossings, temperature
gradient, and relativistic beaming model are adapted from the MIT-licensed
[`s0xDk/ghostty-blackhole`](https://github.com/s0xDk/ghostty-blackhole).
See [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md) for attribution.

The black hole continuously drifts and rotates while morphing between seven
looks: Inferno, Gargantua, M87* donut, Face-on ember, Quasar, Blazar, and Pure
lens. It normally enters a new look every 7–12 seconds and completes a tour in
roughly 48–82 seconds. Higher pressure increases its travel, rotation speed,
morphing speed, and event-horizon size. Accumulated animation phases prevent
pressure changes from causing visible jumps.

## Automatic collection and pressure model

After startup, Pressure Lens watches `%USERPROFILE%\.codex\sessions`:

- It checks for recently active Codex sessions every two seconds.
- Context usage is calculated from
  `last_token_usage.total_tokens / model_context_window`.
- When several sessions are active, the session with the highest context usage
  drives Agent pressure while the dashboard reports the active-session count.
- If the newest JSONL line is still being written, the last complete metric is
  retained.
- Unchanged files reuse an in-memory cache instead of being scanned again.

The pressure score also considers typing intensity, modifier-key ratio,
application switching, continuous work time, Agent context usage, active
Agents, and recent failures. Other Agents can be added through the same local
adapter boundary. Without structured token metrics, only process activity can
be detected reliably.

## Privacy boundaries

- Typed characters, window titles, and clipboard content are never stored.
- Virtual key codes are counted only in memory; SQLite receives aggregated
  numbers.
- The Codex collector parses structured events such as `token_count`; it does
  not store, display, or analyze conversation content.
- Desktop lensing captures only the overlay-sized region beneath the floating
  window, at about 12 FPS. Frames remain in memory, are never uploaded, and do
  not affect the pressure score.
- Lensing is paused and cleared while dragging. Only frames from the new
  position are accepted after release, with a short fade-in to hide stale
  samples.
- The overlay is excluded from Windows screen capture to avoid recursive
  mirroring.
- The lens texture is feathered at all edges; there is no full-desktop
  translucent layer.
- Aggregated data is stored only in a local SQLite database by default.
- The locked black hole is click-through and receives pointer input only in
  move mode.

## Interaction

- Closing the dashboard hides it in the system tray without stopping
  collection.
- The tray menu opens the dashboard, toggles the black hole, starts move mode,
  or exits the app.
- Press `Ctrl + Alt + M` to unlock move mode. Releasing a drag automatically
  locks the overlay and remembers its position.
- You can also hover over the black-hole center for two seconds. Once the
  gravity charge ring completes, hold and drag the black hole. The overlay
  remains click-through during the countdown.

## Architecture

| Layer | Technology | Responsibility |
| --- | --- | --- |
| Desktop shell | Rust + Tauri 2 | Windows, tray, shortcuts, local storage, and OS collection |
| Dashboard | HTML + CSS + JavaScript | Pressure explanation, trends, and visibility controls |
| Desktop black hole | WebGL2 + GLSL | Rendering, morphing, and desktop lensing |
| Data | SQLite | Local storage for aggregated metrics only |

## Requirements

- Windows 10 or Windows 11
- Stable Rust with the MSVC toolchain
- Microsoft Edge WebView2 Runtime

The frontend has no Node.js build step.

## Run locally

```powershell
git clone git@github.com:yuzhiyang1/pressure-lens.git
cd pressure-lens\src-tauri
cargo run
```

Build a release executable:

```powershell
cd src-tauri
cargo build --release
```

The executable is written to
`src-tauri/target/release/pressure-lens.exe`.

## Verification

```powershell
cd src-tauri
cargo test

cd ..
node --test tests/backdrop-capture-gate.test.cjs
```

The second command requires Node.js only for the frontend regression test; it
is not required to build the application.

## License

No project-wide open-source license has been declared yet. Third-party code
remains subject to its original license. See
[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md).
