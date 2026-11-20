import sys, numpy as np, soundfile as sf
from scipy.signal import resample_poly
from faster_whisper import WhisperModel
m=WhisperModel("small.en",device="cpu",compute_type="int8")
for f in sys.argv[1:]:
    x,sr=sf.read(f,dtype="float32"); x=x if x.ndim==1 else x.mean(1)
    x=resample_poly(x,16000,sr).astype("float32"); x=x/ (abs(x).max()+1e-9)*0.9
    seg,_=m.transcribe(x,language="en",beam_size=5,condition_on_previous_text=False)
    print(f.split('/')[-1],"::"," ".join(s.text.strip() for s in seg),"\n")
