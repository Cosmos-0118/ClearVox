# Bundled models

`dpdfnet2_48khz_hr.onnx` and `dpdfnet8_48khz_hr.onnx` are the 48 kHz streaming speech-enhancement
models from [DPDFNet](https://github.com/ceva-ip/DPDFNet) by Ceva (Apache-2.0, see
`LICENSE-DPDFNet.txt`; the weights are also published under Apache-2.0 at
<https://huggingface.co/Ceva-IP/DPDFNet>). Paper: <https://arxiv.org/abs/2512.16420>.

They are compiled into `clearvox.exe` with `include_bytes!`, so the app needs no downloads at run time.
