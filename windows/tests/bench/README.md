# Denoiser benchmark scripts

Python (3.13) scripts used to choose DPDFNet over RNNoise / DeepFilterNet3 / others. They are not
part of the build. Needs `numpy scipy soundfile onnxruntime faster-whisper jiwer dpdfnet noisereduce`
(plus `clearvoice`, `denoiser` for the other candidates) and the DNSMOS model
`sig_bak_ovr.onnx` from <https://github.com/microsoft/DNS-Challenge/tree/master/DNSMOS/DNSMOS>
placed in `dnsmos/`.

- `run_cands.py IN.wav "" nr cv:MossFormerGAN_SE_16K dm:dns64` writes candidate outputs to `out/`.
- `evalreal.py REF.wav out/*.wav` scores a real recording: DNSMOS (SIG/BAK/OVRL), noise suppression
  and speech level change between speech and non-speech frames.
- `evalsim.py` scores simulated 5 dB-SNR mixes against the clean reference (DNSMOS, WER).
- `tr.py FILES...` transcribes with Whisper small.en.

Results on a 20 s recording made with a fan blowing on the mic (higher DNSMOS OVRL is better):
original 1.08, RNNoise 1.40, noisereduce 1.4-1.5, DeepFilterNet3 2.13, DPDFNet-2 2.67,
DPDFNet-8 2.73, MossFormerGAN (offline, 98 s for 21 s) 3.06.
