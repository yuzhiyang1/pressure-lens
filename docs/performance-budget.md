# Performance budget

Pressure Lens is useful only when its own monitoring cost stays far below the work it observes.
The Windows process-tree budget therefore includes the Rust process and every WebView2 child.

| Signal | Default gate | Why |
| --- | ---: | --- |
| Normalized CPU | ≤ 3% | Measured across all logical processors after a 30-second warm-up |
| Peak private memory | ≤ 450 MB | Includes the dashboard, overlay, GPU and renderer processes |
| Peak working set | ≤ 700 MB | Caps physical-memory pressure |
| 30-second private-memory growth | ≤ 30 MB | Detects capture or GPU texture leaks |

The gate is intentionally process-tree based. Measuring only `pressure-lens.exe` would hide most
WebView and GPU cost. Run it on Windows after a release build:

```powershell
.\scripts\measure-performance.ps1 `
  -Executable .\src-tauri\target\release\pressure-lens.exe
```

The normal command enforces every limit, including CPU. GitHub-hosted Windows runners do not expose
a stable GPU-backed desktop, so WebView2 falls back to software rendering and their CPU value is not
comparable with a user's machine. Pull-request CI still records normalized CPU and hard-gates private
memory, working set, and memory growth by passing `-SkipCpuGate`. The manually dispatched
`Representative Windows performance` workflow targets a labeled physical Windows runner and applies
the complete CPU gate.

Four validated balanced-mode runs on Windows measured 1.11–2.64% CPU, 340.38–351.06 MB peak
private memory, 585.92–602.24 MB peak working set, and -4.05–1.13 MB private-memory change over
30 seconds. The previous raw-frame implementation reached about 3.33 GB private memory; compressed
frame IPC and explicit `ImageBitmap` disposal removed that growth.

The six-shape tour uses a 15 FPS balanced-mode cap while retaining 2.5 DPR spatial supersampling.
On the representative Windows machine it measured 11.71% average GPU and 15.20% peak GPU across
a 30-second multi-shape interval. The manual representative-machine ceiling is 20%; hosted CI
cannot enforce this signal because it has no stable hardware-backed desktop.

The committed values are upper safety limits, not targets. Eco and balanced modes should normally
sit below them. A release may tighten the thresholds after a representative hardware sample. Hosted
CI's software-rendering CPU number must never be used to relax the physical-machine CPU budget.
