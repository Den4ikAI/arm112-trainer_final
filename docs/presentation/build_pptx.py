"""Сборка презентации финала из шаблона организаторов ЛЦТ-2026.

Запуск (нужен python-pptx):

    python docs/presentation/build_pptx.py "путь/к/ЛЦТ2026 Шаблон презентации.pptx"

Результат — docs/презентация.pptx. Шаблон в репозиторий не кладётся
(refs/ в .gitignore, 33 МБ медиа), поэтому путь к нему передаётся явно.

Регламент финала: дизайн всех слайдов можно менять, кроме обязательных
слайдов 7–11 шаблона — они сохраняют формат шаблона, меняется только
текст в существующих фигурах. Описательная часть собирается на макете
шаблона «Пустой с заголовком» по сюжету: проблема → замкнутый цикл →
рабочее место → ИИ-карточки → бригада и телефон → оценка → рекомендации →
преподаватель → администратор → контур → что сделано после отбора →
как посмотреть.

Каждое утверждение сверено с README и docs/1…4; числа — из README,
документации или статистики демонстрационного стенда (подписана на
слайде). Чего о команде не знаем — «[ЗАПОЛНИТЬ: …]».
"""

import copy
import sys
from pathlib import Path

from pptx import Presentation
from pptx.chart.data import CategoryChartData
from pptx.dml.color import RGBColor
from pptx.enum.text import MSO_ANCHOR, MSO_AUTO_SIZE, PP_ALIGN
from pptx.util import Emu, Pt

ROOT = Path(__file__).resolve().parents[2]
SCREENS = ROOT / "docs" / "presentation" / "screens"
LOGO = ROOT / "docs" / "presentation" / "logo-dgochs.png"
OUT = ROOT / "docs" / "презентация.pptx"

# Цвета шаблона, слайд 5 «Оформление»
PINK = RGBColor(0xFF, 0x00, 0x53)
LILAC = RGBColor(0x8A, 0x83, 0xD1)
PLUM = RGBColor(0x31, 0x0F, 0x53)
INK = RGBColor(0x1C, 0x1D, 0x22)
WHITE = RGBColor(0xFF, 0xFF, 0xFF)
PALE = RGBColor(0xF3, 0xF0, 0xFA)
GREEN = RGBColor(0x1B, 0x7F, 0x3B)
AMBER = RGBColor(0xB8, 0x5C, 0x00)
FONT = "Montserrat"

TODO = "[ЗАПОЛНИТЬ: {}]"


# ─── Текст ──────────────────────────────────────────────────────────────

def shape(slide, shape_id):
    for sh in slide.shapes:
        if sh.shape_id == shape_id:
            return sh
    raise KeyError(f"на слайде нет фигуры {shape_id}")


def set_run(para, index, text):
    para.runs[index].text = text


def plain(sh):
    """Снять маркеры списка, которые заполнитель наследует от макета."""
    for p in sh.text_frame.paragraphs:
        pPr = p._p.get_or_add_pPr()
        for child in list(pPr):
            if child.tag.split("}")[1].startswith("bu"):
                pPr.remove(child)
        pPr.set("marL", "0")
        pPr.set("indent", "0")
        pPr.append(pPr.makeelement(
            "{http://schemas.openxmlformats.org/drawingml/2006/main}buNone", {}))


def fill(sh, lines, size=None, bold=None, color=INK, bullets=False):
    """Текст описательного слайда: цвет задаём явно.

    У макетов шаблона цвет текста в заполнителях бледный — рассчитан
    на подложку, которой в нашем слайде может не быть, — и на белой
    карточке текст не читается.
    """
    fill_lines(sh, lines, size=size, bold=bold, color=color)
    if not bullets:
        plain(sh)


def fill_lines(sh, lines, size=None, bold=None, color=None):
    """Заменить текст фигуры строками, сохранив оформление первого абзаца.

    Новые абзацы — копии первого (отступы, маркеры, шрифт прогона), так
    что вид остаётся тем, который задал шаблон.
    """
    tf = sh.text_frame
    first = tf.paragraphs[0]._p
    proto_r = first.find("{http://schemas.openxmlformats.org/drawingml/2006/main}r")
    for p in list(tf.paragraphs)[1:]:
        p._p.getparent().remove(p._p)
    # Первый абзац очищаем, но оставляем pPr
    for child in list(first):
        if not child.tag.endswith("}pPr"):
            first.remove(child)
    paras = [first]
    for _ in lines[1:]:
        new = copy.deepcopy(first)
        first.getparent().append(new)
        paras.append(new)
    for p_el, text in zip(paras, lines):
        if proto_r is not None:
            r = copy.deepcopy(proto_r)
            p_el.append(r)
        else:
            r = p_el.makeelement("{http://schemas.openxmlformats.org/drawingml/2006/main}r", {})
            p_el.append(r)
        t = r.find("{http://schemas.openxmlformats.org/drawingml/2006/main}t")
        if t is None:
            t = r.makeelement("{http://schemas.openxmlformats.org/drawingml/2006/main}t", {})
            r.append(t)
        t.text = text
    for p in tf.paragraphs:
        for r in p.runs:
            if size:
                r.font.size = Pt(size)
            if bold is not None:
                r.font.bold = bold
            if color is not None:
                r.font.color.rgb = color


def textbox(slide, x, y, w, h, lines, size=14, color=INK, bold=False,
            bullets=False, anchor=MSO_ANCHOR.TOP, align=PP_ALIGN.LEFT, space=4):
    tb = slide.shapes.add_textbox(Emu(x), Emu(y), Emu(w), Emu(h))
    tf = tb.text_frame
    tf.word_wrap = True
    tf.vertical_anchor = anchor
    tf.margin_left = tf.margin_right = Emu(0)
    for i, line in enumerate(lines):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.alignment = align
        p.space_after = Pt(space)
        parts = line if isinstance(line, list) else [(line, bold)]
        if bullets:
            parts = [("•  ", False)] + parts
        for text, b in parts:
            r = p.add_run()
            r.text = text
            r.font.name = FONT
            r.font.size = Pt(size)
            r.font.bold = b
            r.font.color.rgb = PINK if (bullets and text == "•  ") else color
    return tb


def remove(sh):
    sh._element.getparent().remove(sh._element)


def title_pill(slide, pill_id, title_sh, text):
    """Заголовок в плашке: плашка растёт под длину текста."""
    fill_lines(title_sh, [text])
    pill = shape(slide, pill_id)
    need = int(380_000 + len(text) * 205_000)
    if pill.width < need:
        pill.width = Emu(need)
    title_sh.width = Emu(max(title_sh.width, need))


def picture(slide, path, x, y, w, h, crop=None, border=None):
    """Картинка, вписанная в прямоугольник с сохранением пропорций.

    crop — (лево, верх, право, низ) в долях исходника.
    """
    pic = slide.shapes.add_picture(str(path), Emu(x), Emu(y))
    iw, ih = pic.image.size
    cl, ct, cr, cb = crop or (0, 0, 0, 0)
    pic.crop_left, pic.crop_top, pic.crop_right, pic.crop_bottom = cl, ct, cr, cb
    vw, vh = iw * (1 - cl - cr), ih * (1 - ct - cb)
    scale = min(w / vw, h / vh)
    pw, ph = int(vw * scale), int(vh * scale)
    pic.width, pic.height = Emu(pw), Emu(ph)
    pic.left, pic.top = Emu(x + (w - pw) // 2), Emu(y + (h - ph) // 2)
    if border:
        pic.line.color.rgb = border
        pic.line.width = Pt(1)
    return pic


def photo_in(slide, ph_id, path, cover=True):
    """Фото на место заполнителя-рисунка: cover — заполнить рамку с обрезкой
    по краям, иначе вписать целиком. Заполнитель убираем."""
    ph = shape(slide, ph_id)
    x, y, w, h = ph.left, ph.top, ph.width, ph.height
    remove(ph)
    if not cover:
        return picture(slide, path, x, y, w, h)
    pic = slide.shapes.add_picture(str(path), Emu(x), Emu(y))
    iw, ih = pic.image.size
    if iw / ih > w / h:  # шире рамки — режем бока
        c = (1 - (w / h) / (iw / ih)) / 2
        pic.crop_left = pic.crop_right = c
    else:                # выше рамки — режем низ, лицо сверху
        pic.crop_bottom = 1 - (iw / ih) / (w / h)
    pic.left, pic.top, pic.width, pic.height = Emu(x), Emu(y), Emu(w), Emu(h)
    return pic


# ─── Слайды ─────────────────────────────────────────────────────────────

# ─── Обязательный блок шаблона (слайды 7–11) ───────────────────────────

def s07_title(s):
    """Титул: название задачи и решения, логотип постановщика."""
    fill_lines(shape(s, 3), ["Тренажёр диспетчера ДДС с ИИ"])
    fill_lines(shape(s, 5), [
        "Задача №9 · Департамент ГОЧСиПБ города Москвы",
        "Команда ELABS"])
    # Логотип постановщика — из того же шаблона (слайд 6, белая версия
    # для тёмного фона). Вписываем в рамку заполнителя без обрезки
    ph = shape(s, 4)
    box_w, box_h, x0, y0 = ph.width, ph.height, ph.left, ph.top
    pic = ph.insert_picture(str(LOGO))
    iw, ih = pic.image.size
    pic.crop_left = pic.crop_right = pic.crop_top = pic.crop_bottom = 0
    scale = min(box_w / iw, box_h / ih)
    pic.width, pic.height = Emu(int(iw * scale)), Emu(int(ih * scale))
    pic.left, pic.top = x0, Emu(y0 + (box_h - pic.height) // 2)


def s08_about(s):
    photo_in(s, 2, SCREENS / "team-both.jpg", cover=False)
    fill_lines(shape(s, 18), ["КОМАНДА «ELABS»"], color=PLUM, bold=True)
    about = shape(s, 14).text_frame.paragraphs
    set_run(about[0], 1, "Денис Петров, ML-инженер: синтез и распознавание "
                         "речи, LLM")
    set_run(about[1], 1, "2 человека")
    set_run(about[3], 0, "дружим со школы, с пятого класса; объединили опыт "
                         "в речевых технологиях")
    set_run(about[4], 0, "студенты 1 курса МТИ и МАИ")
    city = about[5]
    r = copy.deepcopy(about[1].runs[1]._r)
    city._p.append(r)
    city.runs[-1].text = " Москва"

    fill_lines(shape(s, 5), [
        "Веб-тренажёр диспетчера ДДС в привычном интерфейсе рабочего места: "
        "ИИ собирает учебное происшествие, диспетчер ведёт карточку, бригаду и "
        "звонки по IP-телефону, тренажёр оценивает работу, даёт рекомендации и "
        "сам готовит следующее задание. Преподаватель управляет и видит всё."],
        size=12)
    fill_lines(shape(s, 8), [
        "Замкнутый цикл без преподавателя: ИИ → задание → разбор → рекомендация → "
        "следующее задание. Эталон считает классификатор ЕКП, модель пишет только "
        "текст. Голос и модели работают локально, в закрытом контуре."], size=12)


# Карточка шаблона: подложка, место под фото, имя, строки
# «роль / ник / телефон / учёба»
TEAM = [
    ("Денис Петров", ["Капитан, ML-инженер: синтез и распознавание речи, LLM",
                      "Telegram: @bceloss", "+7 901 664-73-53",
                      "МТИ, 1 курс, «Информационные системы и технологии»"]),
    ("Александр Головченко", ["Фуллстек-разработчик", "Telegram: @Angel3044",
                              "+7 914 883-66-57", "МАИ, 1 курс, ТОП-ИТ"]),
]
PHOTOS = ["team-denis.jpg", "team-sasha-sq.jpg"]
CARDS = [(17, 2, 15, 9), (56, 3, 58, 57), (59, 4, 61, 60), (62, 5, 64, 63),
         (65, 6, 67, 66)]


def s09_team(s):
    fill_lines(shape(s, 7), ["КОМАНДА"])
    # Карточек в шаблоне пять, участников двое: лишние убираем, а две
    # оставшиеся ставим по центру ряда, не меняя их вида
    for card in CARDS[len(TEAM):]:
        for sid in card:
            el = shape(s, sid)._element
            el.getparent().remove(el)
    left = shape(s, CARDS[0][0]).left
    row_l, row_r = 346076, 9567349 + 2192713  # края ряда из пяти в шаблоне
    first, last = shape(s, CARDS[0][0]), shape(s, CARDS[len(TEAM) - 1][0])
    width = last.left + last.width - first.left
    dx = (row_l + row_r - width) // 2 - left
    for (name, info), card in zip(TEAM, CARDS):
        for sid in card:
            sh = shape(s, sid)
            sh.left = sh.left + dx
        fill_lines(shape(s, card[2]), [name])
        fill_lines(shape(s, card[3]), info)
    for photo, card in zip(PHOTOS, CARDS):
        photo_in(s, card[1], SCREENS / photo)


def s10_history(s):
    fill_lines(shape(s, 7), ["ИСТОРИЯ КОМАНДЫ"])
    fill_lines(shape(s, 37), [
        "Дружим со школы, с пятого класса. Денис несколько лет занимается "
        "синтезом речи и аудиокодеками: собрал открытый русскоязычный речевой "
        "корпус ESpeech и выводил TTS в прод для голосового агента."])
    fill_lines(shape(s, 43), [
        "Объединить опыт в речевых технологиях и сделать продукт, который "
        "реально можно поставить на обычное железо."])
    fill_lines(shape(s, 40), [
        "Реалистичный голос в реальном времени без GPU — обучили облегчённую "
        "модель синтеза под CPU; в llama.cpp добавили MTP для GigaChat 3.1 "
        "(+44 % скорости). На отборе построили оператора 112 и ДДС, после "
        "обратной связи экспертов за три дня перестроили тренажёр вокруг "
        "диспетчера ДДС: IP-телефон, подсказки, задания от ИИ.",
        "Совмещали с учёбой и работой: всё делали вечерами и в дороге.",
    ])


def s11_short(s):
    fill_lines(shape(s, 3), [
        "Сервер на Rust (axum) и PostgreSQL, клиент на React; закрытый контур, "
        "развёртывание одной командой.",
        "Происшествие от ИИ: позиция классификатора ЕКП (1 283 позиции) задаёт "
        "эталон, модель пишет только текст заявителя и описание.",
        "IP-телефон диспетчера: справочник от преподавателя, входящие от "
        "бригады с АОН, журнал вызовов, запись разговора, голос.",
        "Разбор по нормативам 30 с и 3 мин, рекомендации с проверкой следующей "
        "карточкой, «Следующее задание от ИИ».",
        "Два профиля моделей: GigaChat 3.1 на CPU или Gemma 4 на GPU; "
        "голос GigaAM v3 и TeraTTS на CPU. 437 серверных и 120 браузерных проверок.",
    ], size=12)
    fill_lines(shape(s, 7), [
        "Для учебного центра: подготовка диспетчеров ДДС в привычном им "
        "интерфейсе, на отдельном стенде без выхода в интернет.",
        "Преподаватель задаёт службы и телефоны, проводит занятия и экзамены, "
        "слушает записи разговоров, ставит экспертную оценку.",
        "Курсант тренируется сам: тренажёр выдаёт задание, оценивает, "
        "советует и готовит следующее.",
        "Приём вызовов 112 в составе тренажёра: включает администратор.",
    ], size=12)


# ─── Описательная часть ────────────────────────────────────────────────
#
# Каждый слайд — про одну возможность тренажёра: что сделано и чем это
# подтверждено. Числа — только измеренные (backend/docs/замеры/,
# сопроводительная документация); чего ещё нет — «[ЦИФРА: …]».

from pptx.enum.shapes import MSO_SHAPE
from pptx.enum.dml import MSO_LINE_DASH_STYLE

FRONT = ROOT / "frontend" / "screenshots" / "react"
AUDIT = SCREENS / "audit"
TEACH = SCREENS / "teacher"
# Фон макета — тёмный фиолетовый градиент: текст прямо на нём светлый
SOURCE_INK = RGBColor(0xCF, 0xC8, 0xF4)
ON_BG = RGBColor(0xF3, 0xF0, 0xFF)
GREY = RGBColor(0xDD, 0xD8, 0xF5)
PH_INK = RGBColor(0x5B, 0x53, 0xA8)

X0 = 346_075                 # левое поле, как у заголовков шаблона
W = 11_500_000               # ширина рабочей области
TOP = 1_150_000              # верх содержимого под заголовком
BOTTOM = 6_200_000           # низ содержимого над строкой источника

STATUS = {"да": GREEN, "частично": AMBER, "сверх MVP": LILAC, "нет": PINK}

# Число сценариев в браузере — из последнего полного прогона (29.09, вечер)
E2E = "102"


def blank(prs):
    """Слайд на макете шаблона «Пустой с заголовком»: фон и логотипы
    партнёров — от макета, остальное рисуем сами."""
    return prs.slides.add_slide(prs.slide_layouts[3])


def pill(s, text):
    """Розовая плашка заголовка, как на описательных слайдах шаблона."""
    # Справа от плашки — логотипы партнёров из макета: длинный заголовок
    # не должен на них налезать, поэтому шрифт уменьшается под ширину
    limit = 6_150_000
    # Один кегль на всех слайдах; длинные заголовки укорочены под него.
    # 172 тыс. EMU на знак — средняя ширина прописных Montserrat Bold в 17 пт
    size = 14.0
    w = min(limit, int(520_000 + len(text) * 172_000 * size / 17))
    sh = s.shapes.add_shape(MSO_SHAPE.ROUNDED_RECTANGLE, Emu(X0), Emu(324_562),
                            Emu(w), Emu(560_000))
    sh.adjustments[0] = 0.28
    sh.fill.solid()
    sh.fill.fore_color.rgb = PINK
    sh.line.fill.background()
    sh.shadow.inherit = False
    tf = sh.text_frame
    tf.margin_left = tf.margin_right = Emu(200_000)
    tf.vertical_anchor = MSO_ANCHOR.MIDDLE
    tf.word_wrap = False
    p = tf.paragraphs[0]
    p.alignment = PP_ALIGN.LEFT
    r = p.add_run()
    r.text = text.upper()
    r.font.name = FONT
    r.font.size = Pt(size)
    r.font.bold = True
    r.font.color.rgb = WHITE


def source(s, text):
    """Строка источника требования — ответ на вопрос «откуда это»."""
    textbox(s, X0, 6_300_000, 10_900_000, 420_000,
            [[("Требование: ", True), (text, False)]], size=9, color=SOURCE_INK)


def box(s, x, y, w, h, fill=WHITE, border=LILAC, radius=0.06):
    sh = s.shapes.add_shape(MSO_SHAPE.ROUNDED_RECTANGLE, Emu(x), Emu(y), Emu(w), Emu(h))
    sh.adjustments[0] = radius
    sh.fill.solid()
    sh.fill.fore_color.rgb = fill
    if border is None:
        sh.line.fill.background()
    else:
        sh.line.color.rgb = border
        sh.line.width = Pt(1)
    sh.shadow.inherit = False
    return sh


def card(s, x, y, w, h, head, lines, size=11.5, head_color=PLUM, fill=WHITE,
         border=LILAC, bullets=True, color=INK, space=5):
    """Карточка: рамка, заголовок и строки. Строка — текст или список
    (текст, жирный)."""
    box(s, x, y, w, h, fill=fill, border=border)
    pad = 150_000
    yy = y + 110_000
    if head:
        textbox(s, x + pad, yy, w - 2 * pad, 330_000, [head], size=13,
                color=head_color, bold=True)
        yy += 380_000
    textbox(s, x + pad, yy, w - 2 * pad, h - (yy - y) - 80_000, lines,
            size=size, color=color, bullets=bullets, space=space)


def quote(s, x, y, w, h, text, who):
    """Слова заказчика или организатора — дословно, с источником."""
    box(s, x, y, w, h, fill=PALE, border=None)
    bar = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Emu(x), Emu(y), Emu(55_000), Emu(h))
    bar.fill.solid()
    bar.fill.fore_color.rgb = PINK
    bar.line.fill.background()
    textbox(s, x + 180_000, y + 90_000, w - 300_000, h - 150_000,
            [[("«" + text + "»", False)], [("— " + who, True)]],
            size=11, color=PLUM, space=3)


def number(s, x, y, w, h, big, small, big_size=26):
    box(s, x, y, w, h)
    textbox(s, x + 150_000, y + 90_000, w - 300_000, 520_000, [big],
            size=big_size, color=PINK, bold=True)
    textbox(s, x + 150_000, y + 90_000 + int(big_size * 12_700 * 1.45), w - 300_000,
            h - 700_000, [small], size=10.5, color=INK)


def shot(s, path, x, y, w, h, crop=None, label=None):
    """Снимок экрана; нет файла — рамка-заглушка «[СКРИН: …]», чтобы
    место было видно и не забылось."""
    if path is not None and Path(path).exists():
        return picture(s, path, x, y, w, h, crop=crop, border=LILAC)
    ph = box(s, x, y, w, h, fill=PALE, border=LILAC, radius=0.02)
    ph.line.dash_style = MSO_LINE_DASH_STYLE.DASH
    textbox(s, x + 200_000, y + h // 2 - 250_000, w - 400_000, 500_000,
            [f"[СКРИН: {label}]"], size=13, color=PH_INK, bold=True,
            align=PP_ALIGN.CENTER)
    return ph


def caption(s, x, y, w, text):
    textbox(s, x, y, w, 300_000, [text], size=10, color=GREY)


def table(s, rows, x, y, widths, size=10, row_h=300_000, status_col=None):
    t = s.shapes.add_table(len(rows), len(widths), Emu(x), Emu(y),
                           Emu(sum(widths)), Emu(row_h * len(rows))).table
    for j, cw in enumerate(widths):
        t.columns[j].width = Emu(cw)
    for i, row in enumerate(rows):
        t.rows[i].height = Emu(row_h)
        for j, text in enumerate(row):
            c = t.cell(i, j)
            c.fill.solid()
            c.fill.fore_color.rgb = PLUM if i == 0 else (WHITE if i % 2 else PALE)
            c.margin_left = c.margin_right = Emu(80_000)
            c.margin_top = c.margin_bottom = Emu(25_000)
            c.vertical_anchor = MSO_ANCHOR.MIDDLE
            tf = c.text_frame
            tf.word_wrap = True
            p = tf.paragraphs[0]
            p.text = ""
            r = p.add_run()
            r.text = text
            r.font.name = FONT
            r.font.size = Pt(size)
            r.font.bold = i == 0 or (status_col is not None and j == status_col)
            if i == 0:
                r.font.color.rgb = WHITE
            elif status_col is not None and j == status_col:
                r.font.color.rgb = STATUS.get(text.split(" ")[0], PINK)
            else:
                r.font.color.rgb = INK
    return t


# Номера слайдов для карты возможностей проставляются при сборке
NUM = {}

FINAL = SCREENS / "final"
GAP = 250_000


def lead(s, text, y=TOP, h=700_000, size=12):
    """Вводная фраза слайда светлым по фону макета."""
    textbox(s, X0, y, W, h, [text], size=size, color=ON_BG)


def arrow(s, x, y, w, h):
    a = s.shapes.add_shape(MSO_SHAPE.RIGHT_ARROW, Emu(x), Emu(y), Emu(w), Emu(h))
    a.fill.solid()
    a.fill.fore_color.rgb = PINK
    a.line.fill.background()
    a.shadow.inherit = False
    return a


def c_problem(s):
    pill(s, "Зачем это нужно")
    lead(s, "Диспетчер ДДС учится на живых карточках: принять происшествие, выдержать "
            "норматив, направить бригаду, доложить. Преподаватель один на группу и не "
            "может сидеть рядом с каждым.", h=800_000)
    w = (W - 2 * GAP) // 3
    y = TOP + 950_000
    h = 3_400_000
    card(s, X0, y, w, h, "Что нужно курсанту", [
        "Поток карточек своей службы, а не одна учебная задача",
        "Бригада, которая докладывает и звонит сама",
        "Разбор сразу после карточки: что не так и сколько это стоило",
        "Следующее задание под свою ошибку",
    ], size=11.5, space=7)
    card(s, X0 + w + GAP, y, w, h, "Что нужно преподавателю", [
        "Задать службы, телефоны и порядок занятия один раз",
        "Видеть ход занятия по каждому рабочему месту",
        "Слушать разговоры и ставить свою оценку рядом с автоматической",
        "Не писать карточки вручную",
    ], size=11.5, space=7)
    card(s, X0 + 2 * (w + GAP), y, w, h, "Что нужно учебному центру", [
        "Интерфейс, к которому не придётся переучиваться",
        "Работа без интернета, на своём сервере",
        "Нейросети без выдумок: эталон считает классификатор",
        "Документация и развёртывание одной командой",
    ], size=11.5, space=7)
    textbox(s, X0, y + h + 200_000, W, 600_000, [
        [("Ответ: ", True),
         ("тренажёр, который сам замыкает цикл обучения, а преподаватель им управляет.",
          False)]], size=12, color=ON_BG)


def c_cycle(s):
    pill(s, "Решение: замкнутый цикл обучения")
    lead(s, "Каждый шаг делает тренажёр, преподаватель задаёт правила и видит всё.")
    steps = [
        ("ИИ собирает задание", "позиция классификатора, опросная карта, текст заявителя"),
        ("Диспетчер работает", "карточка, статусы, бригада, IP-телефон, доклад"),
        ("Разбор", "нормативы 30 с и 3 мин, статусы по факту, цена ошибки"),
        ("Рекомендация", "что тренировать дальше; проверяется следующей карточкой"),
        ("Следующее задание", "ИИ готовит происшествие под рекомендацию"),
    ]
    n = len(steps)
    aw = 260_000
    bw = (W - (n - 1) * (aw + 2 * 60_000)) // n
    y = TOP + 850_000
    bh = 1_750_000
    x = X0
    for i, (head, body) in enumerate(steps):
        box(s, x, y, bw, bh)
        textbox(s, x + 120_000, y + 120_000, bw - 240_000, 330_000, [str(i + 1)],
                size=18, color=PINK, bold=True)
        textbox(s, x + 120_000, y + 480_000, bw - 240_000, 400_000, [head],
                size=12, color=PLUM, bold=True)
        textbox(s, x + 120_000, y + 900_000, bw - 240_000, bh - 950_000, [body],
                size=10.5, color=INK)
        if i < n - 1:
            arrow(s, x + bw + 60_000, y + bh // 2 - 130_000, aw, 260_000)
        x += bw + aw + 2 * 60_000
    # Замыкание: от пятого шага обратно ко второму
    textbox(s, X0, y + bh + 120_000, W, 350_000,
            ["5 → 2: следующее задание сразу попадает в ленту диспетчера, "
             "цикл идёт без преподавателя"], size=10.5, color=GREY,
            align=PP_ALIGN.CENTER)
    w2 = (W - GAP) // 2
    y2 = y + bh + 550_000
    h2 = 1_750_000
    card(s, X0, y2, w2, h2, "Преподаватель", [
        "Службы, справочник телефонов, занятия и экзамены, веса оценки",
        "Ход занятия по рабочим местам, записи разговоров, экспертная оценка",
        "Аналитика по группе и по курсанту, доля сработавших рекомендаций",
    ], size=11, space=5)
    card(s, X0 + w2 + GAP, y2, w2, h2, "Администратор", [
        "Учётные записи и роли, параметры тренажёра",
        "Рабочее место оператора 112 включает только он",
        "Журнал действий, резервные копии",
    ], size=11, space=5)


def c_workplace(s):
    pill(s, "Рабочее место диспетчера ДДС")
    iw = 7_350_000
    ih = int(iw * 720 / 1280)
    shot(s, FINAL / "06-карточка-подсказки-телефон.png", X0, TOP, iw, ih)
    caption(s, X0, TOP + ih + 40_000, iw,
            "Карточка происшествия с включёнными подсказками: окна пронумерованы, "
            "у текущего — пояснение")
    x = X0 + iw + GAP
    w = W - iw - GAP
    card(s, x, TOP, w, 2_250_000, "Что на экране", [
        "Лента происшествий своей службы, карточка с реквизитами и описанием",
        "Вкладки служб, статусы реагирования, бригада, доклад начальнику",
        "Зелёный кружок в углу: IP-телефон",
    ], size=11, space=6)
    card(s, x, TOP + 2_350_000, w, 2_250_000, "Подсказки по кнопке «?»", [
        "Окна и кнопки обведены и пронумерованы в порядке работы по памятке ДДС",
        "«Далее» и «Назад» ведут по шагам, у каждого — что это и когда этим пользоваться",
        "Накладка не перестраивает интерфейс: учишься на том же экране",
    ], size=11, space=6)


def c_ai(s):
    pill(s, "Происшествие от ИИ")
    lead(s, "Преподавателю или курсанту достаточно выбрать службу: карточку собирает "
            "тренажёр. Эталон считает классификатор, модель пишет только текст.")
    iw = 6_300_000
    ih = int(iw * 720 / 1280)
    y = TOP + 800_000
    path = FINAL / "12-материал-от-ии.png"
    if not path.exists():
        path = FRONT / "material-04-происшествие-ддс.png"
        ih = int(iw * 900 / 1600)
    shot(s, path, X0, y, iw, ih)
    caption(s, X0, y + ih + 40_000, iw,
            "Учебный материал преподавателя: «Происшествие от ИИ», по описанию, вручную")
    x = X0 + iw + GAP
    w = W - iw - GAP
    card(s, x, y, w, 2_000_000, "Как собирается карточка", [
        "Берётся позиция классификатора ЕКП, по которой выезжает служба",
        "Ответы опросной карты собираются и проверяются классификатором",
        "Модель пишет заявителя, адрес и описание под эти ответы",
        "Преподаватель правит и утверждает; курсант получает карточку сразу в ленту",
    ], size=10.5, space=4)
    card(s, x, y + 2_100_000, w, 1_850_000, "Почему карточкам можно верить", [
        "Эталон служб и признаков — детерминированный, из 1 283 позиций ЕКП",
        "Текст модели не влияет на оценку: проверяется то, что задал классификатор",
        "Нормативы для такой карточки идут как для обычной",
    ], size=10.5, space=4)


def c_phone(s):
    pill(s, "Бригада и IP-телефон")
    iw = 6_300_000
    ih = int(iw * 720 / 1280)
    shot(s, FINAL / "03-карточка-телефон-открыт.png", X0, TOP, iw, ih)
    caption(s, X0, TOP + ih + 40_000, iw,
            "Окно IP-телефона поверх карточки: линия, телефонная книга, набор, журнал")
    x = X0 + iw + GAP
    w = W - iw - GAP
    card(s, x, TOP, w, 2_350_000, "Телефон", [
        "Статус линии и внутренний номер, телефонная книга, набор номера",
        "Входящий от бригады с АОН: кружок мигает, окно открывается само",
        "Удержание, микрофон, журнал вызовов: принят, пропущен, сброшен",
        "Запись разговора в WAV, преподаватель слушает целиком",
    ], size=10.5, space=4)
    card(s, x, TOP + 2_450_000, w, 2_150_000, "Собеседники", [
        "Справочник до 10 номеров на службу формирует преподаватель",
        "Руководитель организации, начальник отдела реагирования, начальник и "
        "специалист аварийной бригады: каждый отвечает по-своему",
        "Заявитель — только если преподаватель его включил",
        "Говорить в микрофон или текстом; ответ голосом: GigaAM v3 и TeraTTS",
    ], size=10.5, space=4)
    textbox(s, X0, TOP + ih + 380_000, iw, 700_000, [
        [("Бригада: ", True),
         ("направляется после статуса «Принята»; старший докладывает по этапам, "
          "статус засчитывается, если поставлен по факту доклада.", False)]],
        size=10.5, color=ON_BG)


def c_score(s):
    pill(s, "Оценка и разбор")
    w3 = (W - 2 * GAP) // 3
    tiles = [
        ("30 с", "открыть карточку с момента поступления: отметка «Получена службой»"),
        ("3 мин", "первая запись: статус и текст"),
        ("по факту", "статус засчитывается, только если бригада действительно "
                     "доложила об этом этапе"),
    ]
    for i, (big, small) in enumerate(tiles):
        number(s, X0 + i * (w3 + GAP), TOP, w3, 1_300_000, big, small, big_size=24)
    y = TOP + 1_450_000
    iw = 6_300_000
    ih = 3_200_000
    # Из разбора — только блок анализа текста, чтобы цитаты читались
    shot(s, SCREENS / "p04-анализ-текста.png", X0, y, iw, ih,
         crop=(0.245, 0.26, 0.245, 0.31))
    caption(s, X0, y + ih + 40_000, iw,
            "Анализ текста: каждое замечание с цитатой, которую проверяет код")
    x = X0 + iw + GAP
    w = W - iw - GAP
    card(s, x, y, w, ih, "Что в разборе", [
        "Вовремя ли открыта карточка и сделана первая запись",
        "Верны ли статусы и их порядок; отказ только с причиной",
        "Комментарии диспетчера против того, что слышал: упущено, противоречит, "
        "добавлено от себя",
        "У каждой ошибки своя цена в баллах; веса задаёт преподаватель",
        "Балл модель не меняет: замечания подтверждает преподаватель",
    ], size=10.5, space=4)


def c_advice(s):
    pill(s, "Рекомендации и следующее задание")
    lead(s, "После разбора закрытой карточки тренажёр говорит, что тренировать дальше, "
            "и по кнопке сам готовит происшествие под эту рекомендацию.")
    w3 = (W - 2 * GAP) // 3
    y = TOP + 800_000
    h = 2_300_000
    card(s, X0, y, w3, h, "Откуда рекомендация", [
        "Из замечаний разбора: нормативы, статусы, доклады, текст",
        "Формулировки — из памятки ДДС и нормативов",
        "Фиксируется с временем в карточке курсанта",
    ], size=10.5, space=4)
    card(s, X0 + w3 + GAP, y, w3, h, "Как проверяется", [
        "Следующая карточка той же службы ставит вердикт: ушла ли ошибка",
        "Преподавателю — доля сработавших рядом с базовой линией",
        "Курсант видит прошлые рекомендации и их итог",
    ], size=10.5, space=4)
    card(s, X0 + 2 * (w3 + GAP), y, w3, h, "«Следующее задание от ИИ»", [
        "Кнопка у открытой рекомендации",
        "ИИ собирает происшествие службы курсанта с отметкой, какую "
        "рекомендацию оно отрабатывает",
        "Карточка сразу в ленте, преподаватель не нужен",
    ], size=10.5, space=4)
    y2 = y + h + 250_000
    w4 = (W - 3 * GAP) // 4
    tiles = [("≈70", "попыток ДДС за 8–18 октября"),
             ("34", "рекомендации выданы"),
             ("24", "сработали: ошибка ушла в следующей карточке"),
             ("400+", "звонков в журнале IP-телефона")]
    for i, (big, small) in enumerate(tiles):
        number(s, X0 + i * (w4 + GAP), y2, w4, 1_150_000, big, small, big_size=22)
    caption(s, X0, y2 + 1_200_000, W,
            "Статистика демонстрационного стенда: учебная группа из 10 симулированных "
            "курсантов прошла занятия через настоящий API, баллы и рекомендации "
            "посчитал тренажёр")


def c_teacher(s):
    pill(s, "Преподаватель")
    iw = (W - GAP) // 2
    ih = int(iw * 720 / 1280)
    shot(s, TEACH / "01б-ход-занятия-ддс.png", X0, TOP, iw, ih)
    caption(s, X0, TOP + ih + 40_000, iw, "Ход занятия по рабочим местам: таймеры против норматива")
    shot(s, FINAL / "11-телефоны-ддс.png", X0 + iw + GAP, TOP, iw, ih,
         label="справочник «Телефоны ДДС»")
    caption(s, X0 + iw + GAP, TOP + ih + 40_000, iw,
            "«Телефоны ДДС»: до 10 абонентов на службу, роли и номера")
    y = TOP + ih + 400_000
    h = BOTTOM - y
    w2 = (W - GAP) // 2
    card(s, X0, y, w2, h, "Готовит и проводит", [
        "Учебный материал: от ИИ, по описанию, вручную; правка и утверждение",
        "Занятия, экзамены, тренировка по частям; поимённый выбор материала",
        "Веса оценки: семь групп проверок с пересчётом «было → стало»",
    ], size=10.5, space=4)
    card(s, X0 + w2 + GAP, y, w2, h, "Видит и оценивает", [
        "Ход занятия по местам в реальном времени, режим проектора",
        "Записи разговоров, замечания модели: подтвердить или отклонить",
        "Экспертная оценка рядом с автоматической; аналитика по группе и курсанту",
    ], size=10.5, space=4)


def c_admin(s):
    pill(s, "Администратор и вход")
    iw = 6_700_000
    ih = int(iw * 720 / 1280)
    shot(s, FINAL / "10-вход.png", X0, TOP, iw, ih)
    caption(s, X0, TOP + ih + 40_000, iw,
            "Экран входа: учётные записи стенда одним щелчком, переключатель «Система 112»")
    x = X0 + iw + GAP
    w = W - iw - GAP
    card(s, x, TOP, w, 2_250_000, "Роли", [
        "Курсант: диспетчер ДДС своей службы или оператор 112",
        "Преподаватель: материал, занятия, телефоны, аналитика",
        "Администратор: учётные записи, параметры, классификатор, журнал действий",
    ], size=10.5, space=4)
    card(s, x, TOP + 2_350_000, w, 2_250_000, "Параметры", [
        "Рабочее место 112 свёрнуто по умолчанию; включает администратор в "
        "«Параметрах» или на экране входа паролем администратора",
        "IP-телефон, запись разговоров, время звонка и ответа абонента",
        "Нормативы и веса; пределы проверяет сервер",
    ], size=10.5, space=4)


def c_contour(s):
    pill(s, "Закрытый контур и технологии")
    lead(s, "Всё работает на сервере учебного центра, внешних обращений нет. "
            "Адрес каждой модели задаётся одной переменной.")
    rows = [
        ("", "Сервер без видеокарты", "Сервер с видеокартой"),
        ("Языковая модель", "GigaChat 3.1 10B-A1.8B, llama.cpp со спекулятивным "
                            "декодированием MTP (+44 % скорости)",
         "Gemma 4 26B-A4B INT4 на vLLM"),
        ("Сверка текста с фактами", "кросс-энкодер OpenJEV на процессоре", "Gemma 4"),
        ("Голос", "GigaAM v3 (распознавание) и TeraTTSv2 (синтез), ONNX на процессоре",
         "то же"),
        ("Сервер и клиент", "Rust (axum) и PostgreSQL; React", "то же"),
    ]
    y = TOP + 800_000
    table(s, rows, X0, y, [2_700_000, 5_000_000, 3_800_000], size=10.5, row_h=430_000)
    y2 = y + 5 * 430_000 + 300_000
    w3 = (W - 2 * GAP) // 3
    number(s, X0, y2, w3, 1_150_000, "437", "серверных проверок", big_size=22)
    number(s, X0 + w3 + GAP, y2, w3, 1_150_000, "120", "сценариев в браузере",
           big_size=22)
    number(s, X0 + 2 * (w3 + GAP), y2, w3, 1_150_000, "1 команда",
           "deploy/up.sh поднимает сервер, базу и модели", big_size=22)


def c_changes(s):
    pill(s, "Что сделано после отбора")
    items = [
        ("В центре — диспетчер ДДС.",
         "Приём вызовов 112 свёрнут и включается администратором; у курсанта "
         "ДДС своя лента, свои задания и кнопка «Новое происшествие от ИИ»."),
        ("Происшествие от ИИ.",
         "По выбранной службе: позиция классификатора, проверенная опросная карта, "
         "текст от модели."),
        ("IP-телефон за зелёным кружком.",
         "Справочник от преподавателя, входящие от бригады с АОН, журнал, "
         "запись разговора, голос."),
        ("Подсказки по рабочему месту.",
         "Кнопка «?»: окна пронумерованы и объяснены в порядке памятки ДДС."),
        ("Рекомендации с проверкой и следующее задание от ИИ.",
         "Цикл «задание → разбор → рекомендация → задание» без преподавателя."),
        ("Вход одним щелчком и четыре документа.",
         "Учётные записи на экране входа; описание, руководства администратора, "
         "преподавателя и обучаемого в PDF и DOCX."),
    ]
    y = TOP
    for head, body in items:
        box(s, X0, y, W, 720_000)
        textbox(s, X0 + 200_000, y + 150_000, W - 400_000, 520_000,
                [[(head + " ", True), (body, False)]], size=11.5, color=INK)
        y += 820_000


def c_links(s):
    pill(s, "Как посмотреть")
    w2 = (W - GAP) // 2
    card(s, X0, TOP, w2, 3_600_000, "Стенды и материалы", [
        "Стенд без видеокарты: https://87.242.101.125:25566",
        "Стенд с видеокартой: https://87.242.101.125:25580",
        "Сертификат самоподписанный: браузер попросит подтвердить",
        "Репозиторий: sourcecraft.dev/lct-hackaton-2026/"
        "case-09-112-dispatcher-simulator-team-88",
        "Документация: docs/1…4 (описание, администратор, преподаватель, обучаемый)",
        "Видео: docs/video/",
    ], size=12, space=7)
    card(s, X0 + w2 + GAP, TOP, w2, 3_600_000, "Учётные записи, пароль arm112", [
        "Все записи — на экране входа, вход одним щелчком",
        "teacher1 — преподаватель",
        "dds01…dds05 — диспетчеры ДДС: Служба 101, 104, 103, ЦОДД, Мослифт",
        "dds06…dds10 — то же, десятый — Деп. ЖКХ",
        "admin — администратор; он же включает «Систему 112»",
        "op01…op10 — операторы 112, когда система 112 включена",
    ], size=12, space=7)
    textbox(s, X0, TOP + 3_800_000, W, 900_000, [
        [("С чего начать: ", True),
         ("войти как dds01, нажать «Новое происшествие от ИИ», открыть карточку "
          "за 30 с, принять входящий от бригады за зелёным кружком, закрыть карточку "
          "и посмотреть разбор с рекомендацией.", False)]],
        size=12, color=ON_BG)


PLAN = [
    ("problem", c_problem), ("cycle", c_cycle), ("workplace", c_workplace),
    ("ai", c_ai), ("phone", c_phone), ("score", c_score), ("advice", c_advice),
    ("teacher", c_teacher), ("admin", c_admin), ("contour", c_contour),
    ("changes", c_changes), ("links", c_links),
]

# ─── Сборка ─────────────────────────────────────────────────────────────

def fixed_size(slide, ids, size):
    """Явный кегль без автоподгонки: иначе шаблон ужимает текст в каждой
    рамке по-своему и соседние карточки выходят разного размера."""
    for sid in ids:
        tf = shape(slide, sid).text_frame
        tf.auto_size = MSO_AUTO_SIZE.NONE
        for para in tf.paragraphs:
            for r in para.runs:
                r.font.size = Pt(size)


def uniform_fonts(slides):
    """Один набор кеглей на всех описательных слайдах: основной текст 11,
    заголовки карточек 13, таблицы 9,5, крупные цифры 24. Плашку-заголовок
    слайда (14–17, её ширина посчитана под кегль) не трогаем."""
    for s in slides:
        for sh in s.shapes:
            if sh.has_table:
                for row in sh.table.rows:
                    for cell in row.cells:
                        for para in cell.text_frame.paragraphs:
                            for r in para.runs:
                                r.font.size = Pt(9.5)
                continue
            if not sh.has_text_frame:
                continue
            for para in sh.text_frame.paragraphs:
                for r in para.runs:
                    if r.font.size is None:
                        continue
                    pt = r.font.size.pt
                    if pt >= 18:
                        r.font.size = Pt(24)
                    elif pt < 13:
                        r.font.size = Pt(11)


def main(template):
    prs = Presentation(template)
    slides = list(prs.slides)
    tpl = lambda n: slides[n - 1]  # номер слайда в шаблоне, с единицы

    # Обязательный блок — на своих местах, только текст
    s07_title(tpl(7))
    s08_about(tpl(8))
    s09_team(tpl(9))
    s10_history(tpl(10))
    s11_short(tpl(11))
    fixed_size(tpl(8), [5, 8, 14], 12)
    fixed_size(tpl(9), [9, 57], 11)
    fixed_size(tpl(9), [15, 58], 16)
    fixed_size(tpl(10), [37, 43, 40], 12)
    fixed_size(tpl(11), [3, 7], 12)

    # Номера: обязательный блок — слайды 1–5, дальше описательная часть
    for i, (key, _) in enumerate(PLAN):
        NUM[key] = 6 + i
    new = []
    for key, build in PLAN:
        s = blank(prs)
        build(s)
        new.append(s)

    sld_ids = prs.slides._sldIdLst
    items = list(sld_ids)
    template_items, new_items = items[:len(slides)], items[len(slides):]
    keep = [template_items[n - 1] for n in (7, 8, 9, 10, 11)]
    for el in items:
        sld_ids.remove(el)
    for el in keep + new_items:
        sld_ids.append(el)
    # Слайды шаблона вне обязательного блока отцепляем — вместе с их
    # медиа они не попадут в файл
    for el in template_items:
        if el not in keep:
            prs.part.drop_rel(el.rId)

    uniform_fonts(new)
    prs.save(OUT)
    print(f"собрано: {OUT} ({OUT.stat().st_size / 1e6:.1f} МБ, "
          f"{len(keep) + len(new_items)} слайдов)")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
