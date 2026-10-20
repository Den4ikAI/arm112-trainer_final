#!/usr/bin/env python3
"""Сборка документации в .pdf и .docx.

Требования к сдаче допускают только эти два формата, а Markdown держим
исходником: его видно в истории изменений. Исходников несколько —
по рекомендации финалистам (п. 3) документы раздельные: общее
техническое описание и три руководства. Старый единый документ
собирается по-прежнему.

Зачем свой конвейер, а не pandoc: pandoc не установлен на стенде
и тянет за собой LaTeX. Здесь — только то, что уже есть в системе:
python-markdown для разметки, weasyprint для PDF и LibreOffice для DOCX.

    docs/build.py            # все документы
    docs/build.py 3          # только те, чьё имя начинается с «3»
"""
import base64
import os
import re
import subprocess
import sys

import markdown

ЗДЕСЬ = os.path.dirname(os.path.abspath(__file__))

# (исходник, имя собранного файла без расширения)
ДОКУМЕНТЫ = [
    ("1-Общее-техническое-описание.md", "1-Общее-техническое-описание"),
    ("2-Руководство-администратора.md", "2-Руководство-администратора"),
    ("3-Руководство-преподавателя.md", "3-Руководство-преподавателя"),
    ("4-Руководство-обучаемого.md", "4-Руководство-обучаемого"),
    ("СОПРОВОДИТЕЛЬНАЯ-ДОКУМЕНТАЦИЯ.md", "документация"),
]

# Шрифт с засечками для текста, моноширинный для кода и таблиц кода.
# Размеры подобраны так, чтобы широкие таблицы помещались в книжную
# страницу А4 без переносов в середине слова.
СТИЛЬ = """
@page { size: A4; margin: 2cm 1.8cm;
        @bottom-center { content: counter(page); font-size: 9pt; color: #666; } }
body { font-family: "Liberation Serif", Georgia, serif; font-size: 11pt;
       line-height: 1.45; }
h1 { font-size: 20pt; margin-top: 0; }
h2 { font-size: 15pt; margin-top: 20pt; border-bottom: 1px solid #999;
     padding-bottom: 3pt; page-break-after: avoid; }
h3 { font-size: 12.5pt; margin-top: 14pt; page-break-after: avoid; }
h4 { font-size: 11pt; margin-top: 10pt; page-break-after: avoid; }
code, pre { font-family: "Liberation Mono", monospace; font-size: 9pt; }
pre { background: #f4f4f4; border: 1px solid #ddd; padding: 7pt;
      white-space: pre-wrap; }
table { border-collapse: collapse; width: 100%; font-size: 9.5pt;
        margin: 8pt 0; }
th, td { border: 1px solid #bbb; padding: 3pt 5pt; text-align: left;
         vertical-align: top; }
th { background: #eee; }
img { max-width: 100%; max-height: 16cm; height: auto; border: 1px solid #ccc;
      display: block; margin: 8pt auto 2pt; }
p > img + img { display: inline-block; margin: 8pt 1%; }
p:has(> img + img) { text-align: center; }
hr { border: none; border-top: 1px solid #ccc; margin: 16pt 0; }
/* Титульный лист: всё до первой горизонтальной черты */
.титул { page-break-after: always; padding-top: 5cm; text-align: center; }
.титул h1 { font-size: 26pt; border: none; }
.титул p { font-size: 13pt; }
.титул .надзаголовок { font-size: 12pt; color: #444; }
/* Оглавление от расширения toc */
.toc { page-break-after: always; }
.toc ul { list-style: none; padding-left: 0; }
.toc ul ul { padding-left: 1.5em; }
.toc li { margin: 2pt 0; }
.toc a { text-decoration: none; color: #000; }
"""


def в_html(текст: str) -> str:
    тело = markdown.markdown(
        текст,
        extensions=["tables", "fenced_code", "toc", "md_in_html"],
        extension_configs={"toc": {"toc_depth": "2-3", "title": "Содержание"}},
    )
    return (
        '<!doctype html><html lang="ru"><head><meta charset="utf-8">'
        f"<style>{СТИЛЬ}</style></head><body>{тело}</body></html>"
    )


def вшить_картинки(html: str) -> str:
    """Заменяет относительные пути картинок на data: URI."""

    def замена(m):
        путь = os.path.join(ЗДЕСЬ, m.group(1))
        if m.group(1).startswith("data:") or not os.path.exists(путь):
            return m.group(0)
        with open(путь, "rb") as f:
            данные = base64.b64encode(f.read()).decode()
        return f'src="data:image/png;base64,{данные}"'

    return re.sub(r'src="([^"]+)"', замена, html)


def собрать_один(исходник: str, имя: str) -> int:
    путь_md = os.path.join(ЗДЕСЬ, исходник)
    if not os.path.exists(путь_md):
        print(f"нет исходника: {путь_md}", file=sys.stderr)
        return 1

    with open(путь_md, encoding="utf-8") as f:
        html = в_html(f.read())

    путь_html = os.path.join(ЗДЕСЬ, f"{имя}.html")
    with open(путь_html, "w", encoding="utf-8") as f:
        f.write(html)

    ошибки = 0

    # PDF через weasyprint: он понимает @page и держит поля страницы
    try:
        subprocess.run(
            ["weasyprint", путь_html, os.path.join(ЗДЕСЬ, f"{имя}.pdf")],
            check=True, capture_output=True,
        )
        print(f"собран {имя}.pdf")
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        print(f"{имя}.pdf не собран: {e}", file=sys.stderr)
        ошибки += 1

    # DOCX через LibreOffice: единственный способ получить формат,
    # который можно открыть и поправить. Картинки вшиваются в HTML
    # как data: URI, иначе LibreOffice оставляет на них ссылки.
    try:
        with open(путь_html, "w", encoding="utf-8") as f:
            f.write(вшить_картинки(html))
        subprocess.run(
            ["soffice", "--headless", "--convert-to", "docx:MS Word 2007 XML",
             "--outdir", ЗДЕСЬ, путь_html],
            check=True, capture_output=True, timeout=300,
        )
        print(f"собран {имя}.docx")
    except (subprocess.CalledProcessError, FileNotFoundError,
            subprocess.TimeoutExpired) as e:
        print(f"{имя}.docx не собран: {e}", file=sys.stderr)
        ошибки += 1
    finally:
        # Промежуточный HTML не нужен: исходник — Markdown
        if os.path.exists(путь_html):
            os.remove(путь_html)

    return ошибки


def собрать(фильтр: str = "") -> int:
    ошибки = 0
    for исходник, имя in ДОКУМЕНТЫ:
        if фильтр and not имя.startswith(фильтр):
            continue
        ошибки += собрать_один(исходник, имя)
    return 1 if ошибки else 0


if __name__ == "__main__":
    sys.exit(собрать(sys.argv[1] if len(sys.argv) > 1 else ""))
