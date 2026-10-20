#!/usr/bin/env python3
"""Сборка презентации финала `финал.html` из шаблона `финал-шаблон.html`.

Картинки берутся из `screens/final/` (JPEG, уже обрезанные) и встраиваются
base64; шрифт Onest — из `screens/final/onest.css` (три начертания woff2 в
base64). Запуск: `python3 docs/presentation/build_final.py`.

Подготовка кадров (из PNG-снимков стенда) — функция `prepare()`:
обрезка и сжатие в JPEG; источники указаны в SOURCES, в репозиторий кладётся
только результат.
"""
from __future__ import annotations

import base64
import re
import sys
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
OUT_DIR = HERE / "screens" / "final"
TEMPLATE = HERE / "финал-шаблон.html"
RESULT = HERE / "финал.html"

# имя → (источник, обрезка (l, t, r, b) или None, ширина результата, качество)
SCRATCH = Path("/tmp/claude-1000/-media-denis-work4-lct-hack-final/063342fe-52f9-4708-b91f-5acbc42e71bd/scratchpad")
SOURCES = {
    "card": (SCRATCH / "shots-a9285a/04-карточка-подсказки-шаг1.png", None, 1280, 82),
    "journal-hints": (SCRATCH / "shots-a9285a/01-журнал-подсказки.png", None, 1280, 82),
    "phone": (SCRATCH / "shots-a9285a/03-карточка-телефон-открыт.png", (836, 346, 1272, 648), 872, 85),
    "phonebook": (SCRATCH / "fin/shots/teacher-phonebook.png", (0, 150, 1300, 540), 1300, 82),
    "ai-button": (SCRATCH / "fin/shots/dds-journal.png", (1040, 100, 1920, 330), 880, 82),
    "teacher-home": (SCRATCH / "fin/shots/teacher-home.png", (0, 100, 1920, 620), 1400, 80),
    "debrief": (SCRATCH / "fin/shots/dds-debrief.png", (0, 536, 915, 1012), 915, 85),
    "monitor": (HERE / "screens/teacher/01б-ход-занятия-ддс.png", (0, 0, 1600, 640), 1400, 80),
    "weights": (SCRATCH / "fin/shots/teacher-weights.png", (0, 100, 1920, 700), 1200, 80),
    "analytics": (SCRATCH / "fin/shots/teacher-analytics.png", (0, 100, 1920, 780), 1200, 80),
    "admin": (SCRATCH / "fin/shots/admin.png", (0, 0, 1920, 760), 1400, 80),
    "login": (SCRATCH / "fin/shots/login.png", None, 1400, 80),
    "mand-1": (SCRATCH / "mand2/s-01.png", None, 1600, 78),
    "mand-2": (SCRATCH / "mand2/s-02.png", None, 1600, 78),
    "mand-3": (SCRATCH / "mand2/s-03.png", None, 1600, 78),
    "mand-4": (SCRATCH / "mand2/s-04.png", None, 1600, 78),
    "mand-5": (SCRATCH / "mand2/s-05.png", None, 1600, 78),
}


def prepare() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for name, (src, crop, width, q) in SOURCES.items():
        im = Image.open(src).convert("RGB")
        if crop:
            im = im.crop(crop)
        if im.width > width:
            im = im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)
        im.save(OUT_DIR / f"{name}.jpg", "JPEG", quality=q, optimize=True)
        print(name, im.size, (OUT_DIR / f"{name}.jpg").stat().st_size // 1024, "КБ")


def build() -> None:
    html = TEMPLATE.read_text(encoding="utf-8")
    html = html.replace("{{FONTS}}", (OUT_DIR / "onest.css").read_text(encoding="utf-8"))

    def img(m: re.Match) -> str:
        p = OUT_DIR / f"{m.group(1)}.jpg"
        return "data:image/jpeg;base64," + base64.b64encode(p.read_bytes()).decode()

    html = re.sub(r"\{\{IMG:([\w-]+)\}\}", img, html)

    def png(m: re.Match) -> str:
        p = OUT_DIR / f"{m.group(1)}.png"
        return "data:image/png;base64," + base64.b64encode(p.read_bytes()).decode()

    html = re.sub(r"\{\{PNG:([\w-]+)\}\}", png, html)
    RESULT.write_text(html, encoding="utf-8")
    print(RESULT, RESULT.stat().st_size // 1024, "КБ")


if __name__ == "__main__":
    if "prepare" in sys.argv:
        prepare()
    build()
