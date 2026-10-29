# Performance

Measurements of the **Padwake v0.1.0 Windows x64 executable**, taken on 2026-10-29. These are observations on one laptop, not minimum requirements or promises for every PC.

Executable SHA-256: `eb6a29a3e32c687b383f92f8263ea694703284f431e4c4174a46e7856bdeba90`.

## Test machine and scope

- AMD Ryzen 5 3500U, 8 logical processors; Windows 11.
- 1920 × 1080, 60 Hz, 150% display scaling.
- The release executable, with its crash watchdog enabled and no recording running.
- CPU and GPU figures include Padwake, its WebView2 processes and the watchdog. CPU percentages use the entire machine, not one core. The game process is excluded.

## Results

| Test | Result | Sample |
| :--- | :--- | :--- |
| Keypress → first moving frame | Median **29.75 ms**, range 20.6–68.0 ms; no missed inputs | 20 presses, random phase relative to refresh |
| Game exit → Home visible | Median **85.1 ms** | 5 cycles with the windowed stand-in |
| Game exit → row drawn | Median **282.3 ms** | Same 5 cycles |
| Game exit → Home answering a key | Median **315.9 ms**, maximum 316.6 ms | Same 5 cycles |
| Home idle CPU | **0.0830%** of the machine; 0.664% of one core | 9 processes, 61.1 seconds |
| Home idle GPU | **0.006%** average, 0.341% maximum | 60 one-second GPU engine counter samples |
| While a game runs — CPU | **0.0576%** of the machine; 0.460% of one core | 9 processes, 61.1 seconds |
| While a game runs — GPU | **0.000%** average and maximum | 60 one-second GPU engine counter samples |

The idle result is for the Home screen. The animated startup, search background, trailers and transitions have different costs.

The in-game test uses the full-screen windowed stand-in and starts ten seconds after launch, after the notification closes. Padwake is hidden and its WebView2 view is suspended. The stand-in's own resources are excluded. A measured zero GPU counter is not a promise of zero usage on every system.

[Raw measurements](benchmarks/2026-10-29.txt) include every timing sample and the per-process CPU counters.

The input test uses injected **keyboard** events, not physical controller latency. It watches the first changed row pixels with BitBlt. Each capture waits for a composed frame, so the measurement is an upper bound in roughly 16.7 ms steps at 60 Hz. A median result is not a worst-case guarantee; the slowest press in this run was 68.0 ms.

## Reproduce

Build in release mode, open the app on Home and wait for startup and artwork loading to settle. Leave the laptop otherwise idle. The scripts below are Windows tools and use the developer laptop's `W:` capture paths by default.

```powershell
# App + WebView2 + watchdog CPU/GPU over 60 seconds.
powershell -File tools/app/cost.ps1 -Sec 60 -Exe padwake

# Repeat cost.ps1 while the stand-in is running and Padwake is hidden.

# First moving frame, 20 key presses with randomized timing.
powershell -File tools/app/latency.ps1 -N 20 -Jitter -Exe padwake

# Exit-to-Home timing with the windowed stand-in test game.
powershell -File tools/app/latency.ps1 -Return -N 5 -Exe padwake
```

The latency script uses the input guard and stops if the app cannot be brought to the foreground. Its return test needs **Stand-in Long**, created by `tools/make-test-games.ps1`. It closes only that test game's window. Run hardware-changing tests separately; none is needed for these measurements.

When reporting results, include the release commit, CPU, refresh rate, display scaling, process count and whether a controller was connected. Keep startup animations and video playback out of an idle measurement.
