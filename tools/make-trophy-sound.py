#!/usr/bin/env python3
"""Makes app/ui/assets/sounds/trophy.wav: Padwake's own trophy chime, synthesised here from sine waves, so it comes from
no recording and no other product. Three rising bell notes that ring over each other. Standard library only.
    python tools/make-trophy-sound.py
The app plays it at a lower pitch for bronze and a higher one for gold (app/ui/input/input.js)."""

import math
import struct
import wave
from pathlib import Path

RATE = 44100
LENGTH = 1.5                         # seconds
PEAK = 0.7                           # of full scale
# (start in seconds, frequency in Hz, loudness)
NOTES = [(0.0, 880.0, 0.75), (0.11, 1174.66, 0.85), (0.22, 1567.98, 1.0)]  # A5, D6, G6
# a bell's partials: (multiple of the note, loudness, how fast it dies away per second)
PARTIALS = [(1.0, 1.0, 4.0), (2.0, 0.35, 7.0), (3.01, 0.12, 11.0), (4.2, 0.05, 16.0)]
ATTACK = 0.004                       # seconds, so a note starts without a click
FADE = 0.05                          # seconds, so the file ends at silence


def sample(t):
    v = 0.0
    for start, freq, loud in NOTES:
        dt = t - start
        if dt < 0:
            continue
        env = min(1.0, dt / ATTACK)
        for mult, part, decay in PARTIALS:
            v += loud * part * env * math.exp(-decay * dt) * math.sin(2 * math.pi * freq * mult * dt)
    return v * min(1.0, (LENGTH - t) / FADE)


def main():
    samples = [sample(n / RATE) for n in range(int(LENGTH * RATE))]
    scale = PEAK / max(abs(s) for s in samples)
    path = Path(__file__).resolve().parent.parent / 'app' / 'ui' / 'assets' / 'sounds' / 'trophy.wav'
    with wave.open(str(path), 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b''.join(struct.pack('<h', round(s * scale * 32767)) for s in samples))
    print(f'{path}: {path.stat().st_size // 1024} KB, {LENGTH} s')


if __name__ == '__main__':
    main()
