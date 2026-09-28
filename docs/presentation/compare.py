# -*- coding: utf-8 -*-
"""Сравнение «кадр боевой системы — экран тренажёра» для аудита и слайдов.

У кадров боевой системы срезается рамка браузера и панель задач: в
адресной строке там внутренний адрес стенда заказчика, выносить его
в открытые материалы нельзя.

Запуск: python3 compare.py <каталог эталонов> <каталог наших кадров> <куда>
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ЭТАЛОНЫ, НАШИ, КУДА = map(Path, sys.argv[1:4])
КУДА.mkdir(parents=True, exist_ok=True)

ПАРЫ = [
    ("вход", "dds_01__login.png", "desktop-01-вход.png"),
    ("журнал", "dds_04__journal-rows.png", "desktop-05-журнал-ддс.png"),
    ("карточка-112", "kartochka_01__card112-empty-quick-types.png", "desktop-03-карточка112.png"),
    ("карточка-ддс", "dds_18__card-dds-history-full.png", "desktop-06-карточка-ддс.png"),
]

ШИРИНА = 960


def шрифт(размер):
    for путь in ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                 "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf"):
        if Path(путь).exists():
            return ImageFont.truetype(путь, размер)
    return ImageFont.load_default()


def эталон(путь):
    im = Image.open(путь).convert("RGB")
    w, h = im.size
    # Рамка браузера сверху (вкладки и адрес), боковая панель слева,
    # панель задач снизу — по кадрам 1920×1080
    im = im.crop((48, 76, w, h - 40))
    return im.resize((ШИРИНА, round(im.height * ШИРИНА / im.width)))


def наш(путь):
    im = Image.open(путь).convert("RGB")
    return im.resize((ШИРИНА, round(im.height * ШИРИНА / im.width)))


for имя, ref, our in ПАРЫ:
    if not (ЭТАЛОНЫ / ref).exists() or not (НАШИ / our).exists():
        print("нет кадра для", имя)
        continue
    a, b = эталон(ЭТАЛОНЫ / ref), наш(НАШИ / our)
    высота = max(a.height, b.height) + 40
    лист = Image.new("RGB", (ШИРИНА * 2 + 16, высота), "white")
    d = ImageDraw.Draw(лист)
    f = шрифт(20)
    d.text((8, 8), f"Боевая система (кадр {ref.split('__')[0]})", fill="black", font=f)
    d.text((ШИРИНА + 24, 8), "Тренажёр", fill="black", font=f)
    лист.paste(a, (0, 40))
    лист.paste(b, (ШИРИНА + 16, 40))
    лист.save(КУДА / f"сравнение-{имя}.png", optimize=True)
    print("готово", имя)
