# Pressure Lens

[中文](./README.md) | [English](./README_EN.md)

Pressure Lens is a local-first cognitive-workload visualizer for Windows. It turns aggregated
typing rhythm, app switches, continuous activity, and Agent context signals into an explainable,
calibrated live score and a real daily history.

> The score describes work patterns. It is not a medical or mental-health assessment. Typed
> characters, window titles, and conversation content are not recorded.

## What is implemented

- A true rolling 60-second window with no whole-minute reset jump.
- Background assessment every two seconds and an independent SQLite Journal write every minute.
- Real daily history, average, peak, high-pressure minutes, self-report count, and daily summary.
- Personal calibration from self-reports: the first report can move a score by at most 3 points;
  repeated evidence builds a bounded adjustment of at most 15 points.
- Confidence, window coverage, and per-source health.
- One concrete recovery action at high pressure.
- Settings for performance, animation/lensing strength, quiet hours, pause, privacy switches,
  retention, autostart, and history deletion.
- Single instance, tray residency, bounded file logs, unclean-exit recovery, SQLite WAL, and
  off-screen overlay protection.

## Agent Providers

Providers parse structured metadata only; Pressure Lens does not persist conversation content.

| Provider | Quality | Collection |
| --- | --- | --- |
| Codex | Exact | Latest `token_count` and context window under `.codex/sessions` |
| Claude Code | Estimated | Latest assistant usage under `.claude/projects`, conservatively using a 200k window |
| Cursor | Activity only | `workspaceStorage/state.vscdb` modification time; the message database is never opened |

Providers poll every five seconds. Directory inventories refresh at most every 30 seconds and only
the final 512 KB of active logs is read. When several sessions are active, the highest context
pressure wins and its metric quality remains visible.

## Stable black-hole semantics

The transparent WebGL2 shader adapts Schwarzschild photon paths, a thin accretion disk, temperature
gradients, and relativistic beaming from the MIT-licensed
[`s0xDk/ghostty-blackhole`](https://github.com/s0xDk/ghostty-blackhole). See
[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md).

The black hole always flows, drifts, and rotates, but shapes have stable meaning:

- `calm`: the original inclined Inferno disk;
- `focused`: a tighter edge-on Gargantua disk;
- `overloaded`: hotter and wider Quasar/Blazar forms;
- `uncertain`: the stable Inferno anchor when confidence is low or collection is paused.

Transitions are smooth. The six-shape demo is off by default; when explicitly enabled it cycles
through all six forms, so shape no longer represents pressure alone.

## Privacy boundaries

- The keyboard hook increments counters and discards the virtual key before the callback ends.
- Window titles, clipboard data, and typed characters are never read.
- SQLite stores only minute aggregates, self-reports, settings, and runtime state.
- Lensing captures only the 420×420 region under the overlay. It is compressed in memory, never
  written or uploaded, and never enters the pressure model.
- Capture pauses and clears during drag; late frames from the old position are rejected.
- The overlay remains visible in ordinary system screenshots for recording and feedback. Lensing
  stays inside a locally feathered window; there is no full-desktop translucent layer.
- Pausing freezes the last reading and lowers confidence instead of pretending pressure is zero.

## Performance

Performance is a release gate. Balanced mode is capped at 15 FPS, 1 FPS lens capture, 2 DPR, and
52 ray-marching steps. Hidden overlays and quiet hours stop rendering and capture. Only vivid mode
raises the limits to 30 FPS, 2.5 DPR, and 56 steps.

Validated full-process-tree results (Rust, two WebViews, and GPU):

| Metric | v0.2.0 observed | CI gate |
| --- | ---: | ---: |
| Normalized CPU | 1.11–2.64% | ≤ 3% |
| GPU (default pressure-driven mode) | 9.98% average, 12.97% peak | representative machine manual gate ≤ 20% |
| GPU (six-shape tour) | 11.71% average, 15.20% peak | representative machine manual gate ≤ 20% |
| Peak private memory | 340.38–351.06 MB | ≤ 450 MB |
| Peak working set | 585.92–602.24 MB | ≤ 700 MB |
| 30-second private-memory change | -4.05–1.13 MB | growth ≤ 30 MB |

An additional Release run with the current visual policy on 2026-07-28 measured 1.04% CPU,
384.91 MB peak private memory, 625.59 MB peak working set, and -6.12 MB private-memory change over
30 seconds, passing every gate.

The earlier raw-frame implementation reached about 3.33 GB of private memory. v0.2.0 uses
compressed frame IPC, a fixed buffer, explicit `ImageBitmap.close()`, in-place texture updates, and
consumer backpressure. See [docs/performance-budget.md](./docs/performance-budget.md).

## Interaction

- Closing the dashboard hides it to the tray without stopping collection.
- The tray opens the dashboard, toggles the overlay, enters move mode, or quits.
- Press `Ctrl + Alt + M`, or hover over the black-hole center for two seconds, then hold and drag.
- Releasing restores click-through behavior and transactionally stores the position.

## Install and run

Requirements: Windows 10/11 and Microsoft Edge WebView2 Runtime.

```powershell
git clone git@github.com:yuzhiyang1/pressure-lens.git
cd pressure-lens\src-tauri
cargo run
```

Build an NSIS installer:

```powershell
cd pressure-lens
npx --yes @tauri-apps/cli@latest build --bundles nsis
```

The installer is written under `src-tauri/target/release/bundle/nsis/`.

## Verification

```powershell
npm ci
npm test

cd src-tauri
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release

cd ..
.\scripts\windows-native-smoke.ps1 `
  -Executable .\src-tauri\target\release\pressure-lens.exe
.\scripts\measure-performance.ps1 `
  -Executable .\src-tauri\target\release\pressure-lens.exe
```

CI runs Rust checks, frontend unit tests, real-Chrome E2E, native single-instance/logging smoke, and
the full-process-tree performance gate.

## Signing, updates, and releases

The release workflow requires two signatures:

1. a Tauri Updater private key signs update artifacts and its public key is embedded in the app;
2. a trusted Windows code-signing certificate signs and timestamps the EXE and NSIS installer.

Required repository configuration:

- variable: `PRESSURE_LENS_UPDATER_PUBKEY`
- secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- secrets: `WINDOWS_CERTIFICATE` (Base64 PFX), `WINDOWS_CERTIFICATE_PASSWORD`

The workflow fails closed if any value is missing, so it cannot publish an unsigned installer. A
local acceptance bundle does not claim publisher trust; a certificate owner must configure the
production credentials.

## License

No project-wide open-source license has been declared. Third-party code remains under its original
license; see [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md).
