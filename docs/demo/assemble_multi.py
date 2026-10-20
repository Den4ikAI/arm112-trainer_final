"""Собрать MP4 из записи нескольких рабочих мест (frontend/e2e/demo/студия.ts).

    python3 assemble_multi.py <каталог записи> <итоговый.mp4> [crf]

В каталоге:
    screens.json              порядок и подписи экранов
    <экран>/frames/*.jpg, <экран>/frames.json   кадры CDP screencast (1280×720)
    sounds.json, audio/*.wav  диктор и реплики собеседников по отметкам времени
    focus.json                какой экран в этот момент главный
    cards.json                заставки (полнокадровые карточки)
    notes.json                пометки над экраном («ИИ готовит задание…»)
    speedup.json              сжатые отрезки ожидания (плашка «ускорено ×N»)

Кадр 1920×1080: главный экран 1280×720 без масштабирования (текст чёткий),
остальные — живые миниатюры 592×333 справа, подписи над каждым, титр
диктора внизу. Нужны ffmpeg, numpy, Pillow (системный python3).
"""

import bisect
import json
import subprocess
import sys
import wave
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

W, H = 1920, 1080
FPS = 25
RATE = 44_100
TAIL = 1.0
FONT_DIR = Path.home() / ".local/share/fonts/montserrat"
BOLD = str(FONT_DIR / "Montserrat-Bold.ttf")
REG = str(FONT_DIR / "Montserrat-Regular.ttf")
if not Path(BOLD).exists():
    BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
    REG = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"

BG = (18, 18, 30)
PANEL_BG = (34, 34, 52)
ACCENT = (255, 0, 83)
LABEL = (222, 220, 245)
MUTED = (160, 158, 190)

# Раскладка: главный экран слева 1:1, две миниатюры справа
MAIN = (16, 68, 1280, 720)
THUMBS = [(1312, 68, 592, 333), (1312, 449, 592, 333)]
CAPTION = (16, 812, 1888, 240)


def font(path: str, size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(path, size)


F_LABEL = font(BOLD, 22)
F_LABEL_S = font(BOLD, 17)
F_CAP = font(BOLD, 30)
F_NOTE = font(BOLD, 20)
F_TITLE = font(BOLD, 72)
F_SUB = font(REG, 34)
F_BADGE = font(BOLD, 22)


def read_wav(path: str) -> tuple[np.ndarray, int]:
    with wave.open(path, "rb") as w:
        rate = w.getframerate()
        ch = w.getnchannels()
        width = w.getsampwidth()
        raw = w.readframes(w.getnframes())
    if width == 2:
        data = np.frombuffer(raw, dtype="<i2").astype("float32") / 32768.0
    elif width == 4:
        data = np.frombuffer(raw, dtype="<i4").astype("float32") / 2147483648.0
    else:
        data = (np.frombuffer(raw, dtype="u1").astype("float32") - 128) / 128.0
    if ch > 1:
        data = data.reshape(-1, ch).mean(axis=1)
    return data, rate


def write_wav(path: Path, data: np.ndarray) -> None:
    pcm = np.clip(data, -1, 1) * 32767
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.astype("<i2").tobytes())


def wrap(draw: ImageDraw.ImageDraw, text: str, f: ImageFont.FreeTypeFont, width: int) -> list[str]:
    words = text.split()
    lines: list[str] = []
    cur = ""
    for w in words:
        t = (cur + " " + w).strip()
        if draw.textlength(t, font=f) <= width:
            cur = t
        else:
            if cur:
                lines.append(cur)
            cur = w
    if cur:
        lines.append(cur)
    return lines


def ellipsize(draw: ImageDraw.ImageDraw, text: str, f: ImageFont.FreeTypeFont, width: int) -> str:
    if draw.textlength(text, font=f) <= width:
        return text
    while text and draw.textlength(text + "…", font=f) > width:
        text = text[:-1]
    return text + "…"


class ScreenFrames:
    def __init__(self, src: Path, name: str, remap):
        self.dir = src / name / "frames"
        frames = json.loads((src / name / "frames.json").read_text())
        self.at = [remap(f["at"]) for f in frames]
        self.files = [f["file"] for f in frames]
        self.cache: dict[tuple[int, tuple[int, int]], Image.Image] = {}

    def index(self, t: float) -> int:
        i = bisect.bisect_right(self.at, t) - 1
        return max(0, i)

    def image(self, t: float, size: tuple[int, int]) -> Image.Image | None:
        if not self.files:
            return None
        i = self.index(t)
        key = (i, size)
        img = self.cache.get(key)
        if img is None:
            self.cache = {k: v for k, v in self.cache.items() if k[0] >= i - 1}
            img = Image.open(self.dir / self.files[i]).convert("RGB")
            if img.size != size:
                img = img.resize(size, Image.LANCZOS)
            self.cache[key] = img
        return img


def main(src: Path, out: Path, crf: int = 28) -> None:
    screens = json.loads((src / "screens.json").read_text())
    sounds = json.loads((src / "sounds.json").read_text())
    focus = json.loads((src / "focus.json").read_text())
    cards = json.loads((src / "cards.json").read_text())
    notes = json.loads((src / "notes.json").read_text()) if (src / "notes.json").exists() else []
    windows = json.loads((src / "speedup.json").read_text()) if (src / "speedup.json").exists() else []
    windows.sort(key=lambda w: w["from"])

    def remap(t: float) -> float:
        shift = 0.0
        for w in windows:
            a, b, k = w["from"], w["to"], w["factor"]
            if t <= a:
                break
            if t < b:
                return t - shift - (t - a) * (1 - 1 / k)
            shift += (b - a) * (1 - 1 / k)
        return t - shift

    global MAIN
    if len(screens) == 1:
        # Один экран — по центру, без колонки миниатюр
        MAIN = ((W - 1280) // 2, 68, 1280, 720)
    sf = {s["name"]: ScreenFrames(src, s["name"], remap) for s in screens}
    labels = {s["name"]: s["label"] for s in screens}
    order = [s["name"] for s in screens]
    t0 = min(f.at[0] for f in sf.values() if f.at)
    focus = sorted(({**f, "at": remap(f["at"])} for f in focus), key=lambda f: f["at"])
    cards = [{**c, "from": remap(c["from"]), "to": remap(c["to"] or c["from"])} for c in cards]
    notes = [{**n, "from": remap(n["from"]), "to": remap(n["to"] or n["from"])} for n in notes]
    sounds = [{**s, "at": remap(s["at"])} for s in sounds]
    speed = [{**w, "from": remap(w["from"]), "to": remap(w["to"])} for w in windows]

    # Звуковая дорожка
    end = max(f.at[-1] for f in sf.values() if f.at) - t0
    clips = []
    caps: list[tuple[float, float, str]] = []
    for s in sounds:
        data, rate = read_wav(s["file"])
        if rate != RATE:
            n = int(len(data) * RATE / rate)
            data = np.interp(np.linspace(0, len(data) - 1, n), np.arange(len(data)), data).astype("float32")
        start = max(0.0, s["at"] - t0)
        clips.append((start, data))
        dur = len(data) / RATE
        end = max(end, start + dur)
        if s.get("voice") == "narrator" and s.get("text"):
            caps.append((start, start + dur + 0.6, s["text"]))
    end += TAIL
    track = np.zeros(int(end * RATE) + RATE, dtype="float32")
    for start, data in clips:
        i = int(start * RATE)
        track[i:i + len(data)] += data
    peak = float(np.max(np.abs(track))) or 1.0
    track *= min(1.0, 0.89 / peak)
    wav = out.with_suffix(".voice.wav")
    write_wav(wav, track[: int(end * RATE)])

    focus_at = [f["at"] for f in focus]

    def active(t: float) -> str:
        i = bisect.bisect_right(focus_at, t) - 1
        return focus[i]["screen"] if i >= 0 else order[0]

    ff = subprocess.Popen([
        "ffmpeg", "-y", "-loglevel", "error",
        "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-",
        "-i", str(wav),
        "-c:v", "libx264", "-preset", "medium", "-crf", str(crf), "-pix_fmt", "yuv420p",
        "-c:a", "aac", "-b:a", "128k", "-t", f"{end:.3f}", "-movflags", "+faststart", str(out),
    ], stdin=subprocess.PIPE)
    assert ff.stdin

    total = int(end * FPS)
    base = Image.new("RGB", (W, H), BG)
    last_sig = None
    last_bytes = b""
    for k in range(total):
        tv = k / FPS
        t = t0 + tv
        main = active(t)
        thumbs = [n for n in order if n != main]
        card = next((c for c in cards if c["from"] - t0 <= tv < c["to"] - t0), None)
        cap = next((c for c in caps if c[0] <= tv < c[1]), None)
        note = [n for n in notes if n["from"] - t0 <= tv < n["to"] - t0]
        fast = next((w for w in speed if w["from"] - t0 <= tv < w["to"] - t0), None)
        sig = (
            main, tuple(sf[n].index(t) for n in order), card and card["title"], cap and cap[2],
            tuple(n["text"] for n in note), fast and fast["factor"],
        )
        if sig == last_sig:
            ff.stdin.write(last_bytes)
            continue
        last_sig = sig
        img = base.copy()
        d = ImageDraw.Draw(img)
        if card:
            d.rectangle((0, 0, W, H), fill=(27, 27, 46))
            d.rectangle((0, H - 8, W, H), fill=ACCENT)
            d.text((W / 2, H / 2 - 70), card["title"], font=F_TITLE, fill=(255, 255, 255), anchor="mm")
            y = H / 2 + 20
            for line in wrap(d, card.get("text", ""), F_SUB, 1500):
                d.text((W / 2, y), line, font=F_SUB, fill=LABEL, anchor="mm")
                y += 46
        else:
            # Главный экран
            x, y, w, h = MAIN
            d.rectangle((x - 4, y - 4, x + w + 4, y + h + 4), fill=ACCENT)
            im = sf[main].image(t, (w, h))
            if im is not None:
                img.paste(im, (x, y))
            d.text((x, y - 36), ellipsize(d, labels[main], F_LABEL, w - 200), font=F_LABEL, fill=(255, 255, 255))
            d.rounded_rectangle((x + w - 150, y - 38, x + w, y - 8), radius=8, fill=ACCENT)
            d.text((x + w - 75, y - 23), "на экране", font=F_LABEL_S, fill=(255, 255, 255), anchor="mm")
            for n in note:
                if n["screen"] == main:
                    tw = d.textlength(n["text"], font=F_NOTE) + 40
                    d.rounded_rectangle((x + w / 2 - tw / 2, y + 14, x + w / 2 + tw / 2, y + 54), radius=10, fill=(255, 170, 0))
                    d.text((x + w / 2, y + 34), n["text"], font=F_NOTE, fill=(20, 20, 30), anchor="mm")
            # Миниатюры
            for (tx, ty, tw, th), name in zip(THUMBS, thumbs):
                d.rectangle((tx - 2, ty - 2, tx + tw + 2, ty + th + 2), fill=(70, 70, 100))
                im = sf[name].image(t, (tw, th))
                if im is not None:
                    img.paste(im, (tx, ty))
                d.text((tx, ty - 30), ellipsize(d, labels[name], F_LABEL_S, tw), font=F_LABEL_S, fill=LABEL)
                for n in note:
                    if n["screen"] == name:
                        nw = d.textlength(n["text"], font=F_LABEL_S) + 30
                        d.rounded_rectangle((tx + tw / 2 - nw / 2, ty + 10, tx + tw / 2 + nw / 2, ty + 42), radius=8, fill=(255, 170, 0))
                        d.text((tx + tw / 2, ty + 26), n["text"], font=F_LABEL_S, fill=(20, 20, 30), anchor="mm")
            if fast:
                txt = f"⏩ ожидание ускорено ×{fast['factor']}"
                bw = d.textlength(txt, font=F_BADGE) + 36
                d.rounded_rectangle((W - 16 - bw, 14, W - 16, 52), radius=10, fill=ACCENT)
                d.text((W - 16 - bw / 2, 33), txt, font=F_BADGE, fill=(255, 255, 255), anchor="mm")
            # Титр диктора
            cx, cy, cw, ch = CAPTION
            if cap:
                lines = wrap(d, cap[2], F_CAP, cw - 80)
                lh = 42
                bh = len(lines) * lh + 36
                by = cy + (ch - bh) / 2
                d.rounded_rectangle((cx, by, cx + cw, by + bh), radius=16, fill=PANEL_BG)
                yy = by + 18 + lh / 2
                for line in lines:
                    d.text((cx + cw / 2, yy), line, font=F_CAP, fill=(255, 255, 255), anchor="mm")
                    yy += lh
        d.text((W - 16, H - 14), "Тренажёр диспетчера ДДС · ELABS", font=F_LABEL_S, fill=MUTED, anchor="rd")
        last_bytes = img.tobytes()
        ff.stdin.write(last_bytes)
    ff.stdin.close()
    ff.wait()
    wav.unlink(missing_ok=True)
    if ff.returncode:
        sys.exit(f"ffmpeg: код {ff.returncode}")
    print(f"{out}: {end:.1f} с, кадров {total}, звуков {len(sounds)}")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]), int(sys.argv[3]) if len(sys.argv) > 3 else 28)
