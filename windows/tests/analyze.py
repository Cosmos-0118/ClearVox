"""Per-window loudness of a WAV before/after Sordino: python analyze.py in.wav out.wav [win_s]

Prints dBFS RMS per window for both files plus a summary of the quietest windows (noise floor)
and loudest (speech), so suppression and speech loss can be read off directly.
"""
import sys, wave
import numpy as np


def load(p):
    with wave.open(p) as w:
        assert w.getsampwidth() == 2
        x = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768
        return x.reshape(-1, w.getnchannels()).mean(1), w.getframerate()


def db(x):
    return 20 * np.log10(np.sqrt(np.mean(x * x)) + 1e-9)


a, sr = load(sys.argv[1])
b, _ = load(sys.argv[2])
win = float(sys.argv[3]) if len(sys.argv) > 3 else 0.5
n = int(sr * win)
rows = []
for i in range(0, min(len(a), len(b)) - n, n):
    rows.append((i / sr, db(a[i:i + n]), db(b[i:i + n])))
for t, x, y in rows:
    print(f"{t:6.1f}s  in {x:7.1f}  out {y:7.1f}  delta {y - x:6.1f}")
rows.sort(key=lambda r: r[1])
k = max(1, len(rows) // 5)
q, l = rows[:k], rows[-k:]
print(f"quietest 20% (noise floor): in {np.mean([r[1] for r in q]):.1f} -> out {np.mean([r[2] for r in q]):.1f} dBFS "
      f"(suppression {np.mean([r[1] - r[2] for r in q]):.1f} dB)")
print(f"loudest 20% (speech):       in {np.mean([r[1] for r in l]):.1f} -> out {np.mean([r[2] for r in l]):.1f} dBFS "
      f"(change {np.mean([r[2] - r[1] for r in l]):.1f} dB)")
