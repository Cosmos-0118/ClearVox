"""Rasterizes assets/logo.svg's geometry into PNG / ICO / ICNS (pure Python + numpy, no SVG lib).
Run: python assets/build_icons.py   (keep in sync with logo.svg if the design changes)"""
import struct, zlib, os
import numpy as np

OUT = os.path.dirname(os.path.abspath(__file__))
CREAM = np.array([0xF4, 0xE9, 0xD2], float)
C0, C1 = np.array([0x16, 0x18, 0x1F], float), np.array([0x2A, 0x2E, 0x3B], float)
BARS = [(392, 395, 629), (488, 290, 734), (584, 205, 819), (680, 290, 734), (776, 395, 629)]
DOTS = [(200, y) for y in (400, 463, 526, 589)] + [(296, y) for y in (330, 393, 456, 519, 582, 645)]

def cov(d):  # signed distance (px) -> antialiased coverage
    return np.clip(0.5 - d, 0, 1)

def render(size, inset=0.0):
    """size px square; inset = fraction of the canvas left transparent on each side."""
    s = size * (1 - 2 * inset) / 1024.0
    off = size * inset
    ys, xs = np.mgrid[0:size, 0:size].astype(float) + 0.5
    x, y = (xs - off) / s, (ys - off) / s            # design coords (0..1024)
    aa = 1.0 / s                                      # one output pixel in design units

    # rounded rect (rx=230)
    qx, qy = np.abs(x - 512) - (512 - 230), np.abs(y - 512) - (512 - 230)
    d = np.hypot(np.maximum(qx, 0), np.maximum(qy, 0)) + np.minimum(np.maximum(qx, qy), 0) - 230
    a_bg = np.clip(0.5 - d / aa, 0, 1)
    t = np.clip((x + y) / 2048, 0, 1)[..., None]
    rgb = C0 * (1 - t) + C1 * t
    g = np.clip(1 - np.hypot(x / 1024 - 0.7, y / 1024 - 0.25) / 0.8, 0, 1)[..., None] * 0.10
    rgb = rgb * (1 - g) + 255 * g

    def over(rgb, a, mask, alpha):
        m = (mask * alpha)[..., None]
        return rgb * (1 - m) + CREAM * m

    for cx, cy in DOTS:                               # noisy dots, 35% opacity
        dd = np.hypot(x - cx, y - cy) - 29
        rgb = over(rgb, a_bg, np.clip(0.5 - dd / aa, 0, 1), 0.35)
    for cx, y0, y1 in BARS:                           # clean bars
        dd = np.hypot(x - cx, y - np.clip(y, y0, y1)) - 29
        rgb = over(rgb, a_bg, np.clip(0.5 - dd / aa, 0, 1), 1.0)

    out = np.zeros((size, size, 4), np.uint8)
    out[..., :3] = np.clip(rgb, 0, 255).astype(np.uint8)
    out[..., 3] = (a_bg * 255 + 0.5).astype(np.uint8)
    return out

def png_bytes(img):
    h, w = img.shape[:2]
    raw = b"".join(b"\x00" + img[r].tobytes() for r in range(h))
    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))

def write(name, data):
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(data)

def main():
    write("logo-1024.png", png_bytes(render(1024)))
    write("logo-256.png", png_bytes(render(256)))
    # Windows .ico (PNG-compressed entries)
    sizes = [16, 24, 32, 48, 64, 128, 256]
    pngs = [png_bytes(render(n)) for n in sizes]
    head = struct.pack("<HHH", 0, 1, len(sizes))
    off, ents, blob = 6 + 16 * len(sizes), b"", b""
    for n, p in zip(sizes, pngs):
        ents += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(p), off + len(blob))
        blob += p
    write("ClearVox.ico", head + ents + blob)
    # macOS .icns: standard ~10% transparent margin around the squircle
    types = {"ic11": 32, "ic12": 64, "ic07": 128, "ic13": 256, "ic08": 256, "ic14": 512, "ic09": 512, "ic10": 1024}
    body = b""
    for t, n in types.items():
        p = png_bytes(render(n, inset=0.0977))
        body += t.encode() + struct.pack(">I", len(p) + 8) + p
    write("ClearVox.icns", b"icns" + struct.pack(">I", len(body) + 8) + body)

if __name__ == "__main__":
    main()
