"""Сборка презентации к сдаче из шаблона организаторов ЛЦТ-2026.

Запуск (нужен python-pptx):

    python docs/presentation/build_pptx.py "путь/к/ЛЦТ2026 Шаблон презентации.pptx"

Результат — docs/презентация.pptx. Шаблон в репозиторий не кладётся
(refs/ в .gitignore, 33 МБ медиа), поэтому путь к нему передаётся явно.

Правила организаторов (CHAT-FINDINGS.md, «Третья выгрузка»): слайды 7–11
шаблона — строго в исходном дизайне и сетке, меняется только текст в
существующих фигурах; остальные слайды — на наше усмотрение. Поэтому
обязательный блок заполняется заменой текста в прогонах с сохранением
форматирования, а описательная часть собирается на макетах шаблона
12–27. Всё, чего мы о команде не знаем, помечено «[ЗАПОЛНИТЬ: …]», а
то, что зависит от работы этой ночи, — «[ПРОВЕРИТЬ]».

Каждое утверждение о решении сверено с кодом и
docs/СОПРОВОДИТЕЛЬНАЯ-ДОКУМЕНТАЦИЯ.md; числа — только измеренные.
"""

import copy
import sys
from pathlib import Path

from pptx import Presentation
from pptx.chart.data import CategoryChartData
from pptx.dml.color import RGBColor
from pptx.enum.text import MSO_ANCHOR, PP_ALIGN
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


# ─── Слайды ─────────────────────────────────────────────────────────────

def s07_title(s):
    """Титул: название задачи и решения, логотип постановщика."""
    fill_lines(shape(s, 3), ["Тренажёр оператора ДДС и АРМ-112 с ИИ"])
    fill_lines(shape(s, 5), [
        "Задача №9 · Департамент ГОЧСиПБ города Москвы",
        "Команда " + TODO.format("название команды")])
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
    # Заголовок-заполнитель справа вверху в шаблоне пуст, а его текст
    # белый на светлом фоне — вписанное в него не видно. Менять цвет
    # на обязательном слайде нельзя, поэтому оставляем пустым, как в
    # шаблоне; название команды — на титуле
    about = shape(s, 14).text_frame.paragraphs
    set_run(about[0], 1, TODO.format("ФИО, специальность"))
    set_run(about[1], 1, TODO.format("число") + " человек")
    set_run(about[3], 0, TODO.format("как образовалась команда"))
    set_run(about[4], 0, TODO.format("место работы/учёбы участников"))
    city = about[5]
    r = copy.deepcopy(about[1].runs[1]._r)
    city._p.append(r)
    city.runs[-1].text = " " + TODO.format("город и регион")

    fill_lines(shape(s, 5), [
        "Веб-тренажёр в интерфейсе боевой ПОВ-112: оператор 112 принимает "
        "учебный вызов и заполняет карточку, диспетчер ДДС ведёт реагирование "
        "бригады. Эталон считает классификатор ЕКП, модель работает локально "
        "на процессоре."])
    fill_lines(shape(s, 8), [
        "Тренажёр отличает выясненное у заявителя от угаданного. Замечания "
        "нейросети подтверждены цитатами, которые проверяет код, а прогноз и "
        "рекомендации сверяются с фактом."])


def s09_team(s):
    fill_lines(shape(s, 7), ["КОМАНДА"])
    for name_id, info_id in [(15, 9), (58, 57), (61, 60), (64, 63), (67, 66)]:
        fill_lines(shape(s, name_id), [TODO.format("Имя Фамилия")])
        fill_lines(shape(s, info_id), [
            TODO.format("роль в команде"),
            TODO.format("ник в мессенджере"),
            TODO.format("телефон"),
            TODO.format("место работы/учёбы"),
        ])


def s10_history(s):
    fill_lines(shape(s, 7), ["ИСТОРИЯ КОМАНДЫ"])
    fill_lines(shape(s, 37), [TODO.format(
        "как собрались, участвовали ли вместе в хакатонах и проектах, "
        "интересные факты о команде")])
    fill_lines(shape(s, 43), [TODO.format("что вдохновило в этой задаче")])
    fill_lines(shape(s, 40), [
        "Требования уточнялись по ходу: нормативы ДДС заказчик определил "
        "27.09 — перестроили отсчёт времени и поток карточек. Модель должна "
        "работать без видеокарты — добавили в llama.cpp поддержку MTP для "
        "GigaChat 3.1 (+44 % скорости). Опросных карт в материалах было три — "
        "ещё 41 вывели из классификатора, ничего не сочиняя.",
        TODO.format("личные истории команды, если есть"),
    ])


def s11_short(s):
    fill_lines(shape(s, 3), [
        "Сервер на Rust (axum) и PostgreSQL, клиент на React; всё в закрытом "
        "контуре, развёртывание одной командой.",
        "Классификация и эталон — детерминированно по ЕКП (1 283 позиции), "
        "без нейросети.",
        "Разбор сверяет скрытую истину, стенограмму и карточку: «выяснил» "
        "или «угадал», у каждой ошибки — цена.",
        "ДДС: статусы по памятке, нормативы заказчика 30 с и 3 мин, бригада "
        "и доклады старшего.",
        "Модель GigaChat 3.1 10B-A1.8B на llama.cpp с MTP (+44 %), ответы по "
        "схеме JSON; анализ текста курсанта с проверкой цитат.",
    ])
    fill_lines(shape(s, 7), [
        "Для учебного центра: подготовка операторов 112 и ДДС на копии "
        "боевого интерфейса, без доступа к боевой системе.",
        "Преподаватель готовит вызовы из билетов перепиской с моделью, "
        "проводит занятия, ставит экспертную оценку.",
        "Аналитика группы: ошибки, прогноз результата и рекомендации — "
        "каждое сверено с фактом и базовой линией.",
        "Развитие: голосовой канал, подбор служб по территории — после "
        "данных заказчика.",
    ])


# Описательная часть

def d_roles(s):
    title_pill(s, 10, shape(s, 14), "РОЛИ И РЕЖИМЫ")
    cards = [
        ("01", "Оператор 112",
         "Принимает учебный вызов, выясняет обстоятельства у заявителя, "
         "заполняет карточку и опросную карту. Службы подбирает "
         "классификатор, оператор может добавить вручную."),
        ("02", "Диспетчер ДДС",
         "Получает поток карточек своей службы, принимает или "
         "мотивированно отклоняет, назначает бригаду и ставит статусы "
         "по докладам старшего."),
        ("03", "Преподаватель",
         "Готовит учебные вызовы с помощью модели и утверждает их, проводит "
         "занятия в режимах 112 и ДДС, ставит экспертную оценку, смотрит "
         "аналитику группы."),
        ("04", "Администратор",
         "Учётные записи и роли, версии классификатора, журнал действий, "
         "состояние сервера. Оценок не ставит."),
    ]
    ids = [(15, 2, 3), (16, 4, 5), (17, 6, 7), (18, 8, 9)]
    for (num, head, body), (n_id, h_id, b_id) in zip(cards, ids):
        fill(shape(s, n_id), [num], color=PINK)
        fill(shape(s, h_id), [head], color=PLUM, bold=True)
        fill(shape(s, b_id), [body], size=13)


def d_path(s):
    title_pill(s, 12, shape(s, 15), "СКВОЗНОЙ ПУТЬ")
    steps = {
        (2, 3): ("Вызов", "Заявитель сообщает часть фактов, остальное — только на вопрос"),
        (8, 9): ("Карточка", "Опросная карта; службы подбирает классификатор ЕКП"),
        (4, 5): ("ДДС", "Поток карточек: открыть за 30 с, первая запись — за 3 мин"),
        (10, 11): ("Бригада", "Назначение, доклады старшего, статусы по факту доклада"),
        (6, 7): ("Разбор", "Выяснил или угадал, анализ текста, экспертная оценка"),
    }
    for (h_id, b_id), (head, body) in steps.items():
        fill(shape(s, h_id), [head], color=PINK, bold=True, size=16)
        fill(shape(s, b_id), [body], size=13)


def d_operator(s):
    fill_lines(shape(s, 2), ["ОПЕРАТОР 112"], color=WHITE)
    card = shape(s, 4)
    remove(shape(s, 3))
    x0, y0, w0, h0 = card.left, card.top, card.width, card.height
    pic_w = int(w0 * 0.70)
    picture(s, SCREENS / "p02-карточка-112.png", x0 + 150_000, y0 + 150_000,
            pic_w, h0 - 300_000, border=LILAC)
    textbox(s, x0 + pic_w + 330_000, y0 + 260_000, w0 - pic_w - 480_000, h0 - 400_000, [
        "Интерфейс снят с кадров боевой ПОВ-112",
        "Заявитель отвечает только на заданный вопрос",
        "Опросная карта растёт по мере ответов — карты у 44 из 51 типа",
        "Службы подбирает классификатор: 1 283 позиции ЕКП",
        "Норматив времени — на экране",
    ], size=13, bullets=True, space=10)


def d_dds(s):
    fill(shape(s, 3), ["ДИСПЕТЧЕР ДДС"], color=PLUM, bold=True)
    body = shape(s, 7)
    fill(body, [
        "Поток карточек своей службы: у каждой ожидающей свой таймер.",
        "Нормативы заказчика: открыть карточку за 30 с, первая запись "
        "(статус и текст) — за 3 мин.",
        "Статусы строго по памятке; «Не принята» и «Отказ» — только "
        "с комментарием.",
        "Бригада: назначение, доклады старшего, звонок старшему.",
        "Разбор: статус поставлен по факту доклада, а не наугад.",
    ], size=14)
    for p in body.text_frame.paragraphs:
        p.space_after = Pt(10)
    ph = shape(s, 4)
    x0, w0 = ph.left, ph.width
    remove(ph)
    # Две вырезки экрана ДДС в натуральную величину: панель бригады
    # (звонок старшему, входящий доклад, разбор) и вкладка своей службы
    # в полосе служб со статусом. Экран целиком на полслайда нечитаем
    shot = SCREENS / "p06-карточка-ддс.png"
    x, w = x0 + 250_000, w0 - 500_000
    textbox(s, x, 1_350_000, w, 400_000, ["Панель бригады"], size=13,
            color=WHITE, bold=True)
    picture(s, shot, x, 1_750_000, w, 1_300_000,
            crop=(0.0, 0.4306, 0.525, 0.4056), border=LILAC)
    textbox(s, x, 3_450_000, w, 400_000, ["Своя служба в полосе служб"],
            size=13, color=WHITE, bold=True)
    picture(s, shot, x, 3_850_000, int(w * 0.62), 500_000,
            crop=(0.3672, 0.924, 0.3719, 0.0204), border=LILAC)


def d_review(s):
    title_pill(s, 23, shape(s, 14), "РАЗБОР И ЭКСПЕРТНАЯ ОЦЕНКА")
    for sid in [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 17, 18, 19, 20]:
        remove(shape(s, sid))
    picture(s, SCREENS / "p08-экспертная-оценка.png", 346_075, 1_150_000,
            7_300_000, 5_150_000, crop=(0.004, 0.315, 0.525, 0.105), border=LILAC)
    textbox(s, 7_950_000, 1_350_000, 3_900_000, 4_900_000, [
        [("Правила. ", True), ("«Угадал» — значение верное, но заявитель его "
                               "не сообщал; «не заполнено», «лишняя служба» — "
                               "с ценой ошибки.", False)],
        [("Нейросеть. ", True), ("Сверяет описание курсанта с тем, что он "
                                 "слышал. Каждое замечание — с цитатой, цитату "
                                 "проверяет код.", False)],
        [("Преподаватель. ", True), ("Подтверждает или отклоняет замечания и "
                                     "ставит экспертную оценку рядом с "
                                     "автоматической. Балл модель не меняет.",
                                     False)],
        [("На кадре: ", True), ("описание «горит баня, есть пострадавший, дом "
                                "газифицирован» при разговоре о горящей крыше "
                                "дома без пострадавших.", False)],
    ], size=13, space=12)


def d_ai(s):
    title_pill(s, 2, shape(s, 14), "КАК ИСПОЛЬЗУЕТСЯ ИИ")
    left = shape(s, 4)
    fill(left, [
        "Модель — GigaChat 3.1 10B-A1.8B, локально на процессоре.",
        "Эталон для оценки она не считает никогда — его считает "
        "классификатор.",
        "Ответ ограничен схемой JSON на уровне декодирования.",
        "Пока курсант ведёт вызов, модель не вызывается: ответы заявителя "
        "подготовлены заранее и утверждены преподавателем.",
    ], size=15)
    for p in left.text_frame.paragraphs:
        p.space_after = Pt(12)
    for sid, text in zip([5, 6, 7, 8, 9], [
        "Подготовка учебных вызовов из 96 обращений билетов",
        "Правка вызова перепиской с преподавателем — видна фактическая разница",
        "Анализ свободного текста курсанта с проверкой цитат",
        "Словесная сводка по группе из уже посчитанных чисел",
        "Прогноз результата — гребневая регрессия, рядом базовая линия",
    ]):
        sh = shape(s, sid)
        fill(sh, [text], size=14)
        sh.text_frame.vertical_anchor = MSO_ANCHOR.MIDDLE


def d_teacher(s):
    title_pill(s, 2, shape(s, 55), "ПРЕПОДАВАТЕЛЬ")
    # Рамки-браузеры шаблона мелкие для экранов тренажёра: снимки крупно
    for sid in [8, 31, 5, 6, 3, 4]:
        remove(shape(s, sid))
    w, h, y = 5_600_000, 3_300_000, 1_250_000
    for x, name, caption in [
        (346_075, "p10-подготовка-вызовов.png",
         [("Подготовка вызовов. ", True),
          ("Переписка с моделью: «поменяй ФИО», «добавь, что идёт пламя». "
           "Эталон пересчитывает классификатор; курсанты видят вызов только "
           "после утверждения.", False)]),
        (6_246_075, "p11-аналитика.png",
         [("Аналитика группы. ", True),
          ("Выяснил или угадал, тепловая карта ошибок, прогноз результата и "
           "рекомендации — рядом с базовой линией.", False)]),
    ]:
        picture(s, SCREENS / name, x, y, w, h, border=LILAC)
        textbox(s, x, y + h + 250_000, w, 1_300_000, [caption], size=13)


def d_arch(s):
    fill(shape(s, 14), ["АРХИТЕКТУРА И ЗАКРЫТЫЙ КОНТУР"], color=PLUM, bold=True)
    cols = [
        (15, 2, 3, "01", "Клиент",
         "React 19, TypeScript. Работает в браузере, ставить на рабочие "
         "места ничего не нужно."),
        (16, 4, 5, "02", "Сервер",
         "Rust (axum), PostgreSQL 16. Роли, классификатор, статусы, сторож "
         "нормативов, отчёты, OpenAPI. Ежесуточные резервные копии."),
        (17, 6, 7, "03", "Модель",
         "Сервер с API, совместимым с OpenAI; адрес — ML_BASE_URL. Профиль "
         "ЦП: GigaChat 3.1 в llama.cpp. Профиль GPU: Qwen3.8-27B в vLLM "
         "[ПРОВЕРИТЬ]. Внешних обращений нет."),
    ]
    for n_id, h_id, b_id, num, head, body in cols:
        fill(shape(s, n_id), [num], color=PINK)
        fill(shape(s, h_id), [head], color=PLUM, bold=True)
        fill(shape(s, b_id), [body], size=14)
    textbox(s, 623_888, 6_260_000, 11_000_000, 380_000, [
        "Всё разворачивается одной командой deploy/up.sh (docker compose) "
        "без выхода в интернет."], size=13, color=PLUM, bold=True)


def d_tz(s):
    title_pill(s, 2, shape(s, 10), "СООТВЕТСТВИЕ ТЗ")
    for sid in [3, 4, 5, 6]:
        remove(shape(s, sid))
    rows = [
        ("Требование ТЗ", "Как закрыто", "Статус"),
        ("Интерфейс АРМ-112, участки 112 и ДДС",
         "Копия по кадрам боевой системы; журнал, карточка 112, карточка ДДС", "да"),
        ("Классификация и оповещение по ЕКП",
         "Детерминированный поиск по 1 283 позициям", "да"),
        ("Контроль временных нормативов",
         "Норматив вызова 112; ДДС — 30 с и 3 мин по ответу заказчика", "да"),
        ("Оценка на основе нейросетевого анализа, анализ ручного ввода",
         "Анализ текста с проверкой цитат; с экспертами на выборке не сверен",
         "частично"),
        ("Автоматическая и экспертная оценка",
         "Оба балла рядом, каждое изменение — в журнале действий", "да"),
        ("Генерация и утверждение сценариев нейросетью",
         "Подготовка из билетов, правка перепиской, утверждение преподавателем",
         "да"),
        ("Закрытый контур, сервер ИИ без видеокарты",
         "Локальная модель на процессоре; адрес задаётся ML_BASE_URL", "да"),
        ("Голос (VoIP)", "[ПРОВЕРИТЬ после слияния голоса]", "[ПРОВЕРИТЬ]"),
        ("Нагрузка: 2 с при 100 пользователях, 20 сессий, 100 записей/с",
         "[ПРОВЕРИТЬ после нагрузочных замеров]", "[ПРОВЕРИТЬ]"),
        ("Резервное копирование не реже раза в сутки",
         "Ежесуточный pg_dump с ротацией", "да"),
        ("Дообучение по поправкам преподавателя",
         "Переписка подаётся модели в контекст; веса не меняются", "частично"),
    ]
    x, y, w = 346_075, 1_130_000, 11_500_000
    tbl = s.shapes.add_table(len(rows), 3, Emu(x), Emu(y), Emu(w),
                             Emu(420_000 * len(rows))).table
    tbl.columns[0].width = Emu(4_300_000)
    tbl.columns[1].width = Emu(5_700_000)
    tbl.columns[2].width = Emu(1_500_000)
    for i, row in enumerate(rows):
        tbl.rows[i].height = Emu(400_000)
        for j, text in enumerate(row):
            c = tbl.cell(i, j)
            c.fill.solid()
            c.fill.fore_color.rgb = PLUM if i == 0 else (WHITE if i % 2 else PALE)
            c.margin_left = c.margin_right = Emu(90_000)
            c.margin_top = c.margin_bottom = Emu(30_000)
            c.vertical_anchor = MSO_ANCHOR.MIDDLE
            tf = c.text_frame
            tf.word_wrap = True
            p = tf.paragraphs[0]
            p.text = ""
            r = p.add_run()
            r.text = text
            r.font.name = FONT
            r.font.size = Pt(10.5 if i else 11)
            r.font.bold = i == 0 or j == 2
            if i == 0:
                r.font.color.rgb = WHITE
            elif j == 2:
                r.font.color.rgb = {"да": GREEN, "частично": AMBER}.get(text, PINK)
            else:
                r.font.color.rgb = INK


def d_numbers(s):
    title_pill(s, 2, shape(s, 13), "ЗАМЕРЫ")
    chart = shape(s, 10).chart
    data = CategoryChartData()
    # Значения и в подписях категорий: подписи данных LibreOffice при
    # экспорте в PDF не рисует
    data.categories = ["без MTP: 10,3 ток/с", "с MTP: 14,8 ток/с"]
    data.add_series("токенов в секунду", (10.3, 14.8))
    chart.replace_data(data)
    chart.has_legend = False
    plot = chart.plots[0]
    plot.has_data_labels = True
    labels = plot.data_labels
    labels.font.size = Pt(18)
    labels.font.bold = True
    labels.font.color.rgb = WHITE
    labels.number_format = '0.0" ток/с"'
    labels.number_format_is_linked = False
    from pptx.enum.chart import XL_LABEL_POSITION
    labels.position = XL_LABEL_POSITION.OUTSIDE_END
    chart.value_axis.maximum_scale = 18
    textbox(s, 346_075, 5_880_000, 5_749_925, 700_000, [
        "Скорость генерации GigaChat 3.1 на процессоре: EPYC 7532, 6 потоков, "
        "8 каналов памяти — потолок, не пол. На i7 не мерили."],
        size=11, color=WHITE)
    for (h_id, b_id), (head, body) in zip([(3, 4), (5, 6), (7, 8)], [
        ("+44 %", "скорость от спекулятивного декодирования MTP; принимается "
                  "73 % черновых токенов"),
        ("26–33 с", "анализ текста курсанта на процессоре — в фоновой очереди, "
                    "разбор правил приходит сразу"),
        ("278 + 61", "проверок сервера и сценариев в браузере [ПРОВЕРИТЬ "
                     "числа после слияния]"),
    ]):
        fill(shape(s, h_id), [head], size=28, bold=True, color=PINK)
        fill(shape(s, b_id), [body], size=13)


def d_limits(s):
    fill(shape(s, 6), ["ОГРАНИЧЕНИЯ — ЧЕСТНО"], color=PLUM, bold=True)
    items = [
        "Службы подбираются только по типу происшествия: справочника "
        "«адрес → службы» у заказчика нет.",
        "Анализ текста не сверен с экспертами на выборке; вид замечания "
        "модель путает — решение за преподавателем.",
        "Скорость модели на настольном i7 не измерена: 14,8 ток/с — на EPYC "
        "с восемью каналами памяти.",
        "Эталонная версия классификатора (v046_24 или v046_11) заказчиком "
        "не определена; активна v046_24.",
        "Сообщения ДДС в 112 об ошибке в карточке нет: примеров ошибок "
        "заказчик не дал.",
    ]
    for sid, text in zip([8, 9, 10, 11, 12], items):
        sh = shape(s, sid)
        sh.width = Emu(11_300_000)
        fill(sh, [text], size=15)
        sh.text_frame.vertical_anchor = MSO_ANCHOR.MIDDLE


def d_next(s):
    fill(shape(s, 28), ["ЧТО ДАЛЬШЕ"], color=PLUM, bold=True)
    obj = shape(s, 29)
    fill(obj, [
        "[ПРОВЕРИТЬ после слияния ночной работы:]",
        "Голос на процессоре: распознавание GigaAM-v3, синтез TeraTTSv2 — "
        "разговор ДДС со старшим бригады",
        "Доклад диспетчера вышестоящему начальнику",
        "Марафон — экзамен из 5–10 вызовов; блиц — отработка части карточки",
        "Профиль GPU: Qwen3.8-27B в vLLM",
        "После данных заказчика: подбор служб по территории и подчинённости",
        "Репозиторий и стенд: " + TODO.format("ссылки"),
    ], size=15)
    paras = obj.text_frame.paragraphs
    for p in paras:
        p.space_after = Pt(10)
    paras[0].runs[0].font.color.rgb = PINK
    paras[0].runs[0].font.bold = True


# ─── Сборка ─────────────────────────────────────────────────────────────

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

    # Описательная часть на макетах шаблона; каждый используется раз
    plan = [
        (16, d_roles), (25, d_path), (13, d_operator), (14, d_dds),
        (18, d_review), (20, d_ai), (27, d_teacher), (17, d_arch),
        (19, d_tz), (21, d_numbers), (15, d_limits), (12, d_next),
    ]
    for n, build in plan:
        build(tpl(n))

    keep = [7, 8, 9, 10, 11] + [n for n, _ in plan]
    sld_ids = prs.slides._sldIdLst
    by_num = {i + 1: el for i, el in enumerate(list(sld_ids))}
    for el in list(sld_ids):
        sld_ids.remove(el)
    for n in keep:
        sld_ids.append(by_num[n])
    # Лишние слайды отцепляем от презентации — вместе с их медиа они
    # не попадут в файл
    for n, el in by_num.items():
        if n not in keep:
            prs.part.drop_rel(el.rId)

    prs.save(OUT)
    print(f"собрано: {OUT} ({OUT.stat().st_size / 1e6:.1f} МБ, {len(keep)} слайдов)")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
