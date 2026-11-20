import sys, traceback, numpy as np, soundfile as sf, time
inp = sys.argv[1]
tag = sys.argv[2] if len(sys.argv) > 2 else ""
x, sr = sf.read(inp, dtype="float32")
if x.ndim > 1: x = x.mean(1)
def save(name, y, sr_=sr):
    sf.write(f"out/{tag}{name}.wav", y.astype("float32"), sr_)

def run(name, fn):
    t = time.time()
    try:
        fn(); print(f"{name}: ok {time.time()-t:.1f}s")
    except Exception as e:
        print(f"{name}: FAIL {e!r}"); traceback.print_exc(limit=2)

def nr_stat():
    import noisereduce as nr
    save("noisereduce_stat", nr.reduce_noise(y=x, sr=sr, stationary=True, prop_decrease=1.0))
def nr_nonstat():
    import noisereduce as nr
    save("noisereduce_nonstat", nr.reduce_noise(y=x, sr=sr, stationary=False, prop_decrease=1.0))

def clearvoice(model):
    def f():
        from clearvoice import ClearVoice
        cv = ClearVoice(task="speech_enhancement", model_names=[model])
        out = cv(input_path=inp, online_write=False)
        out = np.asarray(out).squeeze()
        save("cv_" + model, out, 48000 if "48K" in model else 16000)
    return f

def demucs(model):
    def f():
        import torch, torchaudio
        from denoiser import pretrained
        from denoiser.dsp import convert_audio
        m = getattr(pretrained, model)().eval()
        import soundfile as _sf; _a,s=_sf.read(inp,dtype="float32"); wav=torch.from_numpy(_a if _a.ndim>1 else _a[:,None]).T.contiguous()
        wav = convert_audio(wav, s, m.sample_rate, m.chin)
        with torch.no_grad():
            out = m(wav[None])[0]
        save("demucs_" + model, out.squeeze(0).numpy(), m.sample_rate)
    return f

for a in sys.argv[3:]:
    if a == "nr": run("nr_stat", nr_stat); run("nr_nonstat", nr_nonstat)
    elif a.startswith("cv:"): run(a, clearvoice(a[3:]))
    elif a.startswith("dm:"): run(a, demucs(a[3:]))
