import sys, glob, os, numpy as np, soundfile as sf, onnxruntime as ort
from scipy.signal import resample_poly
from math import gcd

def load16(p):
    x, sr = sf.read(p, dtype="float32")
    if x.ndim > 1: x = x.mean(1)
    g = gcd(16000, sr)
    return resample_poly(x, 16000 // g, sr // g).astype("float32")

sess = ort.InferenceSession("dnsmos/sig_bak_ovr.onnx", providers=["CPUExecutionProvider"])
p_ovr = np.poly1d([-0.06766283, 1.11546468, 0.04602535])
p_sig = np.poly1d([-0.08397278, 1.22083953, 0.0052439])
p_bak = np.poly1d([-0.13166888, 1.60915514, -0.39604546])

def dnsmos(a):
    L = int(9.01 * 16000)
    while len(a) < L: a = np.concatenate([a, a])
    n = max(1, int(np.floor(len(a) / 16000) - 9.01) + 1)
    S = []
    for i in range(n):
        seg = a[i * 16000:(i + int(9.01)) * 16000 + 1000][:L]
        if len(seg) < L: seg = np.pad(seg, (0, L - len(seg)))
        o = sess.run(None, {"input_1": seg[None].astype("float32")})[0][0]
        S.append([p_sig(o[0]), p_bak(o[1]), p_ovr(o[2])])
    return np.mean(S, 0)

def frames_db(a, n=320):
    k = len(a) // n
    r = np.sqrt((a[:k * n].reshape(k, n) ** 2).mean(1) + 1e-12)
    return 20 * np.log10(r + 1e-9)

ref_path = sys.argv[1]
files = sys.argv[2:]
ref = load16(ref_path)
# speech mask from the cleanest candidate available
from scipy.signal import butter, sosfilt
HP = butter(4, 120, "high", fs=16000, output="sos")
vad_src = next(f for f in files if f.endswith("dpd_dpdfnet8.wav"))
v = frames_db(load16(vad_src)[:len(ref)])
mask = v > -45
rdb = frames_db(sosfilt(HP, ref))
m = min(len(mask), len(rdb)); mask = mask[:m]; rdb = rdb[:m]
print(f"speech frames {mask.mean()*100:.0f}%  input speech {rdb[mask].mean():.1f} dB  input noise {rdb[~mask].mean():.1f} dB")
print(f"{'file':34s} SIG  BAK  OVRL | noise_supp  speech_chg")
rows = [(ref_path, ref)] + [(f, load16(f)) for f in files]
for name, a in rows:
    a = a[:len(ref)] if len(a) >= len(ref) else np.pad(a, (0, len(ref) - len(a)))
    d = frames_db(sosfilt(HP, a))[:m]
    s = dnsmos(a)
    ns = rdb[~mask].mean() - d[~mask].mean()
    sp = d[mask].mean() - rdb[mask].mean()
    print(f"{os.path.basename(name):34s} {s[0]:.2f} {s[1]:.2f} {s[2]:.2f} | {ns:7.1f} dB  {sp:7.1f} dB")
