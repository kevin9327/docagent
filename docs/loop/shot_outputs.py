"""Rasterize convert outputs into README screenshots. Not part of the engine."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pypdfium2 as pdfium
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1] / "assets"
CHROME = Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe")
TEAL = (7, 16, 24)
ACCENT = (45, 212, 191)
MUTED = (148, 163, 184)
WHITE = (248, 250, 252)


def font(size: int, bold: bool = False):
    names = ["segoeuib.ttf" if bold else "segoeui.ttf", "arialbd.ttf" if bold else "arial.ttf"]
    windir = Path(r"C:\Windows\Fonts")
    for name in names:
        path = windir / name
        if path.exists():
            try:
                return ImageFont.truetype(str(path), size)
            except OSError:
                continue
    return ImageFont.load_default()


def crop_whitespace(img: Image.Image, bg: tuple[int, int, int], pad: int) -> Image.Image:
    px = img.load()
    w, h = img.size

    def is_bg(x: int, y: int) -> bool:
        p = px[x, y]
        return all(abs(p[i] - bg[i]) < 18 for i in range(3))

    top, bot, left, right = 0, h - 1, 0, w - 1
    while top < h and all(is_bg(x, top) for x in range(0, w, 4)):
        top += 1
    while bot > top and all(is_bg(x, bot) for x in range(0, w, 4)):
        bot -= 1
    while left < w and all(is_bg(left, y) for y in range(top, bot + 1, 4)):
        left += 1
    while right > left and all(is_bg(right, y) for y in range(top, bot + 1, 4)):
        right -= 1
    return img.crop(
        (max(0, left - pad), max(0, top - pad), min(w, right + 1 + pad), min(h, bot + 1 + pad))
    )


def chrome_shot(html_path: Path, png_path: Path, width: int, height: int) -> None:
    subprocess.run(
        [
            str(CHROME),
            "--headless=new",
            "--disable-gpu",
            "--hide-scrollbars",
            f"--window-size={width},{height}",
            "--force-device-scale-factor=2",
            f"--screenshot={png_path}",
            html_path.resolve().as_uri(),
        ],
        check=True,
        capture_output=True,
    )


def frame_page(inner: Image.Image, title: str, subtitle: str, max_w: int = 720) -> Image.Image:
    if inner.width > max_w:
        ratio = max_w / inner.width
        inner = inner.resize((max_w, int(inner.height * ratio)), Image.Resampling.LANCZOS)
    pad_x, top, bottom = 36, 88, 36
    w = inner.width + pad_x * 2
    h = inner.height + top + bottom
    canvas = Image.new("RGB", (w, h), TEAL)
    draw = ImageDraw.Draw(canvas)
    draw.rectangle((0, 0, w, 8), fill=ACCENT)
    draw.text((pad_x, 22), title, font=font(22, True), fill=WHITE)
    draw.text((pad_x, 52), subtitle, font=font(14), fill=MUTED)
    shadow = Image.new("RGB", (inner.width + 8, inner.height + 8), (4, 10, 14))
    canvas.paste(shadow, (pad_x + 4, top + 4))
    canvas.paste(inner, (pad_x, top))
    return canvas


def trio(pdf: Image.Image, html: Image.Image, cap: Image.Image) -> Image.Image:
    target_h = 720
    panels = []
    for img in (pdf, html, cap):
        ratio = target_h / img.height
        panels.append(img.resize((int(img.width * ratio), target_h), Image.Resampling.LANCZOS))
    gap = 18
    w = sum(p.width for p in panels) + gap * 4
    h = target_h + 110
    canvas = Image.new("RGB", (w, h), TEAL)
    draw = ImageDraw.Draw(canvas)
    draw.text((gap * 2, 22), "examples/letter.md  →  PDF/A + HTML + capsule", font=font(24, True), fill=WHITE)
    draw.text(
        (gap * 2, 56),
        "One Command::Convert. Integer layout. Three SHA-256 hashes. GPU-free.",
        font=font(14),
        fill=MUTED,
    )
    x = gap * 2
    for p in panels:
        canvas.paste(p, (x, 88))
        x += p.width + gap
    return canvas


def write_prove_html() -> Path:
    prove = json.loads((ROOT / "letter-prove.json").read_text(encoding="utf-8"))
    r1, r2 = prove["run1"], prove["run2"]
    html = f"""<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"/><title>prove</title>
<style>
body{{margin:0;background:#071018;color:#e2e8f0;font-family:"Segoe UI",Roboto,Helvetica,sans-serif}}
main{{padding:28px 32px 32px}}
.kicker{{color:#5eead4;letter-spacing:.18em;font-size:12px;font-weight:700}}
h1{{font-size:22px;margin:10px 0 6px;color:#f8fafc}}
.sub{{color:#94a3b8;font-size:14px;margin:0 0 18px}}
.grid{{display:grid;grid-template-columns:1fr 1fr;gap:18px}}
.card{{background:#0b1c24;border:1px solid #134e4a;border-radius:12px;padding:16px 18px}}
.card h2{{margin:0 0 12px;font-size:14px;color:#5eead4;letter-spacing:.12em}}
.row{{display:grid;grid-template-columns:92px 1fr;gap:8px;padding:8px 0;border-bottom:1px solid #1e293b;font-family:Consolas,"Cascadia Mono",monospace;font-size:12px}}
.key{{color:#94a3b8}}
.val{{color:#99f6e4;word-break:break-all}}
.badge{{display:inline-block;margin-top:16px;background:#134e4a;color:#99f6e4;padding:6px 12px;border-radius:999px;font-size:13px;font-weight:700}}
</style></head>
<body><main>
<div class="kicker">PROVE · TWO CONVERTS</div>
<h1>examples/letter.md → PDF/A + HTML</h1>
<p class="sub">Same input, same fonts, same plan. Engine {r1.get("engine_version", "0.1.0")}. Not a mock.</p>
<div class="grid">
<article class="card">
<h2>RUN 1</h2>
<div class="row"><div class="key">input</div><div class="val">{r1["input_hash"]}</div></div>
<div class="row"><div class="key">plan</div><div class="val">{r1["plan_hash"]}</div></div>
<div class="row"><div class="key">output</div><div class="val">{r1["output_hash"]}</div></div>
</article>
<article class="card">
<h2>RUN 2</h2>
<div class="row"><div class="key">input</div><div class="val">{r2["input_hash"]}</div></div>
<div class="row"><div class="key">plan</div><div class="val">{r2["plan_hash"]}</div></div>
<div class="row"><div class="key">output</div><div class="val">{r2["output_hash"]}</div></div>
</article>
</div>
<div class="badge">identical: true · pdf_bytes_equal · html_bytes_equal</div>
</main></body></html>
"""
    path = ROOT / "_prove_preview.html"
    path.write_text(html, encoding="utf-8")
    return path


def write_capsule_html() -> Path:
    cap = json.loads((ROOT / "letter-capsule.json").read_text(encoding="utf-8"))
    html = f"""<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"/><title>capsule</title>
<style>
body {{ margin:0; background:#071018; color:#e2e8f0; font-family: Consolas, "Cascadia Mono", ui-monospace, monospace; }}
main {{ padding: 28px 32px 36px; }}
.kicker {{ color:#5eead4; letter-spacing:.18em; font-size:12px; font-weight:700; font-family: Segoe UI, sans-serif; }}
h1 {{ font-family: Segoe UI, sans-serif; font-size:22px; margin:10px 0 18px; color:#f8fafc; }}
.row {{ display:grid; grid-template-columns: 140px 1fr; gap:10px 16px; padding:12px 0; border-bottom:1px solid #1e293b; font-size:13px; }}
.key {{ color:#94a3b8; }}
.val {{ color:#99f6e4; word-break:break-all; }}
.note {{ margin-top:18px; color:#64748b; font-family: Segoe UI, sans-serif; font-size:13px; }}
</style></head>
<body><main>
<div class="kicker">CAPSULE · SHA-256 ×3</div>
<h1>letter.md → PDF/A + HTML</h1>
<div class="row"><div class="key">input_hash</div><div class="val">{cap["input_hash"]}</div></div>
<div class="row"><div class="key">plan_hash</div><div class="val">{cap["plan_hash"]}</div></div>
<div class="row"><div class="key">output_hash</div><div class="val">{cap["output_hash"]}</div></div>
<div class="row"><div class="key">engine</div><div class="val">{cap["engine_version"]}</div></div>
<div class="row"><div class="key">signature</div><div class="val">null (unsigned replay is still evidence)</div></div>
<p class="note">Same fonts, same plan, same output bytes on the next run.</p>
</main></body></html>
"""
    path = ROOT / "_capsule_preview.html"
    path.write_text(html, encoding="utf-8")
    return path


def main() -> None:
    doc = pdfium.PdfDocument(str(ROOT / "letter.pdf"))
    page = doc[0]
    pdf_raw = crop_whitespace(page.render(scale=2.4).to_pil().convert("RGB"), (255, 255, 255), 48)
    page.close()
    doc.close()

    html_raw = ROOT / "_html_shot.png"
    chrome_shot(ROOT / "letter.html", html_raw, 900, 920)
    html_img = crop_whitespace(Image.open(html_raw).convert("RGB"), (248, 250, 252), 24)

    cap_html = write_capsule_html()
    cap_raw = ROOT / "_cap_shot.png"
    chrome_shot(cap_html, cap_raw, 900, 520)
    cap_img = crop_whitespace(Image.open(cap_raw).convert("RGB"), (7, 16, 24), 12)

    pdf_framed = frame_page(pdf_raw, "PDF/A  ·  page 1", "Integer layout at 1/7200 in  ·  GPU-free")
    html_framed = frame_page(html_img, "HTML  ·  agent-readable", "Headings, table, same IR as the PDF")
    cap_framed = frame_page(cap_img, "Capsule  ·  three hashes", "input  ·  plan  ·  output")

    pdf_framed.save(ROOT / "output-pdf.png")
    html_framed.save(ROOT / "output-html.png")
    cap_framed.save(ROOT / "output-capsule.png")
    trio(pdf_framed, html_framed, cap_framed).save(ROOT / "output-trio.png")

    prove_html = write_prove_html()
    chrome_shot(prove_html, ROOT / "output-rerun.png", 1400, 560)
    crop_whitespace(Image.open(ROOT / "output-rerun.png").convert("RGB"), TEAL, 36).save(
        ROOT / "output-rerun.png"
    )

    for p in (html_raw, cap_raw, cap_html, prove_html):
        p.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
