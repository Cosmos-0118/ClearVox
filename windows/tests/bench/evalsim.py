import numpy as np, soundfile as sf, glob, os, re, jiwer
from scipy.signal import resample_poly, stft
from faster_whisper import WhisperModel
import onnxruntime as ort
exec(open("evalreal.py").read().split("ref_path = sys.argv[1]")[0].split("sess = ")[0])  # imports/load16
sess = ort.InferenceSession("dnsmos/sig_bak_ovr.onnx", providers=["CPUExecutionProvider"])
p_ovr = np.poly1d([-0.06766283, 1.11546468, 0.04602535]); p_sig = np.poly1d([-0.08397278, 1.22083953, 0.0052439]); p_bak = np.poly1d([-0.13166888, 1.60915514, -0.39604546])
def dnsmos(a):
    L=int(9.01*16000)
    while len(a)<L: a=np.concatenate([a,a])
    n=max(1,int(np.floor(len(a)/16000)-9.01)+1); S=[]
    for i in range(n):
        seg=a[i*16000:i*16000+L]
        if len(seg)<L: seg=np.pad(seg,(0,L-len(seg)))
        o=sess.run(None,{"input_1":seg[None].astype("float32")})[0][0]; S.append([p_sig(o[0]),p_bak(o[1]),p_ovr(o[2])])
    return np.mean(S,0)
REF="Hello, this is a test of the noise suppression. Can you hear me clearly? The quick brown fox jumps over the lazy dog. Please keep your voice steady while the fan is running in the background."
norm=lambda t: re.sub(r"[^a-z ]","",t.lower())
wm=WhisperModel("small.en",device="cpu",compute_type="int8")
def wer(a):
    a=a/(abs(a).max()+1e-9)*0.9
    seg,_=wm.transcribe(a,language="en",beam_size=5,condition_on_previous_text=False)
    return jiwer.wer(norm(REF)," ".join(norm(s.text) for s in seg) or "x")
def mag_snr(clean,y):
    L=min(len(clean),len(y)); c=clean[:L]; y=y[:L]
    _,_,C=stft(c,16000,nperseg=512); _,_,Y=stft(y,16000,nperseg=512)
    C=np.abs(C); Y=np.abs(Y)
    # scale-invariant: fit gain
    g=(C*Y).sum()/((Y*Y).sum()+1e-9); return 10*np.log10((C**2).sum()/(((C-g*Y)**2).sum()+1e-9))
clean=load16("sim/clean.wav")
res={}
for n in ["fan","pink","hum","clicks","white"]:
    for f in [f"sim/{n}_in.wav"]+sorted(glob.glob(f"sim/{n}/*.wav")):
        name="input" if f.endswith("_in.wav") else os.path.basename(f)[:-4]
        a=load16(f); s=dnsmos(a)
        res.setdefault(name,[]).append((s[0],s[1],s[2],mag_snr(clean,a),wer(a)))
print(f"{'model':26s} SIG  BAK  OVRL  magSNR  WER   (mean over fan,pink,hum,clicks,white)")
for k,v in sorted(res.items(), key=lambda kv: -np.mean([r[2] for r in kv[1]])):
    m=np.mean(v,0); print(f"{k:26s} {m[0]:.2f} {m[1]:.2f} {m[2]:.2f}  {m[3]:5.1f}  {m[4]*100:4.0f}%")
print("\nper-noise OVRL / WER%:")
for k,v in res.items(): print(f"{k:26s}"," ".join(f"{r[2]:.2f}/{r[4]*100:3.0f}" for r in v))
