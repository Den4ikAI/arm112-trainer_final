#!/usr/bin/env python3
"""Сборка сопроводительной документации в .docx и .pdf.

Требования к сдаче (§18 ТЗ) допускают только эти два формата,
а Markdown держим исходником: его видно в истории изменений.

Зачем свой конвейер, а не pandoc: pandoc не установлен на стенде
и тянет за собой LaTeX. Здесь — только то, что уже есть в системе:
python-markdown для разметки и LibreOffice для форматов.
"""
import os
import subprocess
import sys

import markdown

ЗДЕСЬ = os.path.dirname(os.path.abspath(__file__))
ИСХОДНИК = os.path.join(ЗДЕСЬ, "СОПРОВОДИТЕЛЬНАЯ-ДОКУМЕНТАЦИЯ.md")

# Шрифт с засечками для текста, моноширинный для кода и таблиц кода.
# Размеры подобраны так, чтобы широкие таблицы лицензий помещались
# в книжную страницу А4 без переносов в середине слова.
СТИЛЬ = """
@page { size: A4; margin: 2cm 1.8cm; }
body { font-family: "Liberation Serif", Georgia, serif; font-size: 11pt;
       line-height: 1.45; }
h1 { font-size: 20pt; margin-top: 0; }
h2 { font-size: 15pt; margin-top: 20pt; border-bottom: 1px solid #999;
     padding-bottom: 3pt; }
h3 { font-size: 12.5pt; margin-top: 14pt; }
code, pre { font-family: "Liberation Mono", monospace; font-size: 9pt; }
pre { background: #f4f4f4; border: 1px solid #ddd; padding: 7pt;
      white-space: pre-wrap; }
table { border-collapse: collapse; width: 100%; font-size: 9.5pt;
        margin: 8pt 0; }
th, td { border: 1px solid #bbb; padding: 3pt 5pt; text-align: left;
         vertical-align: top; }
th { background: #eee; }
img { max-width: 100%; height: auto; border: 1px solid #ccc;
      display: block; margin: 8pt auto 2pt; }
p > img + img { display: inline-block; margin: 8pt 1%; }
p:has(> img + img) { text-align: center; }
hr { border: none; border-top: 1px solid #ccc; margin: 16pt 0; }
"""


def в_html(текст: str) -> str:
    тело = markdown.markdown(текст, extensions=["tables", "fenced_code"])
    return (
        '<!doctype html><html lang="ru"><head><meta charset="utf-8">'
        f"<style>{СТИЛЬ}</style></head><body>{тело}</body></html>"
    )


def вшить_картинки(html: str) -> str:
    """Заменяет относительные пути картинок на data: URI."""
    import base64
    import re

    def замена(m):
        путь = os.path.join(ЗДЕСЬ, m.group(1))
        if m.group(1).startswith("data:") or not os.path.exists(путь):
            return m.group(0)
        with open(путь, "rb") as f:
            данные = base64.b64encode(f.read()).decode()
        return f'src="data:image/png;base64,{данные}"'

    return re.sub(r'src="([^"]+)"', замена, html)


def собрать() -> int:
    if not os.path.exists(ИСХОДНИК):
        print(f"нет исходника: {ИСХОДНИК}", file=sys.stderr)
        return 1

    with open(ИСХОДНИК, encoding="utf-8") as f:
        html = в_html(f.read())

    путь_html = os.path.join(ЗДЕСЬ, "документация.html")
    with open(путь_html, "w", encoding="utf-8") as f:
        f.write(html)

    ошибки = 0

    # PDF через weasyprint: он понимает @page и держит поля страницы
    try:
        subprocess.run(
            ["weasyprint", путь_html, os.path.join(ЗДЕСЬ, "документация.pdf")],
            check=True, capture_output=True,
        )
        print("собран документация.pdf")
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        print(f"pdf не собран: {e}", file=sys.stderr)
        ошибки += 1

    # DOCX через LibreOffice: единственный способ получить формат,
    # который можно открыть и поправить. Картинки вшиваются в HTML
    # как data: URI, иначе LibreOffice оставляет на них ссылки.
    путь_html_docx = os.path.join(ЗДЕСЬ, "документация.html")
    try:
        with open(путь_html_docx, "w", encoding="utf-8") as f:
            f.write(вшить_картинки(html))
        subprocess.run(
            ["soffice", "--headless", "--convert-to", "docx:MS Word 2007 XML",
             "--outdir", ЗДЕСЬ, путь_html_docx],
            check=True, capture_output=True, timeout=180,
        )
        print("собран документация.docx")
        with open(путь_html_docx, "w", encoding="utf-8") as f:
            f.write(html)
    except (subprocess.CalledProcessError, FileNotFoundError,
            subprocess.TimeoutExpired) as e:
        print(f"docx не собран: {e}", file=sys.stderr)
        ошибки += 1

    return 1 if ошибки else 0


if __name__ == "__main__":
    sys.exit(собрать())
