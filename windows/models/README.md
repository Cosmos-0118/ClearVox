# Bundled models

`dpdfnet2_48khz_hr.onnx` and `dpdfnet8_48khz_hr.onnx` are the 48 kHz streaming speech-enhancement
models from [DPDFNet](https://github.com/ceva-ip/DPDFNet) by Ceva (Apache-2.0, see
`LICENSE-DPDFNet.txt`; the weights are also published under Apache-2.0 at
<https://huggingface.co/Ceva-IP/DPDFNet>). Paper: <https://arxiv.org/abs/2512.16420>.

`sordino.exe` loads them from `SORDINO_MODELS_DIR`, a `models` folder next to the exe, or
`%LOCALAPPDATA%\Sordino\models` (where `install.ps1` puts the ones you pick), and falls back to the
built-in RNNoise if none is installed. Build with `--features embed-models` to compile them in instead.

The models are published once as the `models-v1` GitHub release (`.github/workflows/models.yml`), not with each
app release; `install.ps1` downloads them from that pinned tag. To change them, update this folder, push a new
`models-vN` tag and bump `$ModelsTag` in `install.ps1`.
