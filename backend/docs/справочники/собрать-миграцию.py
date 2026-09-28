# -*- coding: utf-8 -*-
"""Миграция справочников по кадрам боевой системы.

Источник — выписки в этом каталоге:
  службы-по-кадрам.txt  окно «Добавьте службы», refs/СЛУЖБЫ 112/image1…55,
                        по порядку прокрутки, «кадр|название» дословно;
  типы-по-кадрам.txt    выпадающий список «что случилось?»,
                        refs/КАРТОЧКА 112/image2…13;
  службы-в-базе-до.txt  службы тренажёра до правки: «id|название|заголовок
                        столбца классификатора» — чтобы у служб, которые
                        уже есть, сохранились их id.

Запуск: python3 собрать-миграцию.py > ../../migrations/20260929100000_catalog_by_frames.sql
"""
from pathlib import Path

HERE = Path(__file__).parent

# Транслитерация — та же, что в src/ingest/slug.rs: id службы должен
# совпадать с тем, что дал бы импорт классификатора
TR = {'а': 'a', 'б': 'b', 'в': 'v', 'г': 'g', 'д': 'd', 'е': 'e', 'ё': 'e', 'ж': 'zh',
      'з': 'z', 'и': 'i', 'й': 'j', 'к': 'k', 'л': 'l', 'м': 'm', 'н': 'n', 'о': 'o',
      'п': 'p', 'р': 'r', 'с': 's', 'т': 't', 'у': 'u', 'ф': 'f', 'х': 'h', 'ц': 'c',
      'ч': 'ch', 'ш': 'sh', 'щ': 'sch', 'ъ': '', 'ы': 'y', 'ь': '', 'э': 'e', 'ю': 'yu',
      'я': 'ya'}


def slug(title: str) -> str:
    s = title.lower().replace('\n', ' ')
    for noise in ('классификатор ', 'классификатор.'):
        if s.startswith(noise):
            s = s[len(noise):]
    out, prev_dash = '', True
    for ch in s:
        if ch.isascii() and ch.isalnum():
            piece = ch.lower()
        elif ch.isalpha():
            piece = TR.get(ch, '')
        else:
            piece = ''
        if not piece:
            if not prev_dash and out:
                out += '-'
                prev_dash = True
        else:
            out += piece
            prev_dash = False
    out = out.strip('-')
    return out[:48].strip('-') if len(out) > 48 else out


def q(s: str) -> str:
    return "'" + s.replace("'", "''") + "'"


# Типы: наш id → название на кадре. Все 51 тип уже заведены; меняется
# только написание там, где оно расходится с кадром
TYPE_IDS = {
    '101': '101', '102': '102', '103': '103', '104': '104',
    'Аварии и происшествия в городском хозяйстве': 'city-utilities',
    'Аварии и происшествия на транспортных объектах': 'transport-objects',
    'Аварии на гидротехнических сооружениях': 'hydro',
    'Аварии на опасных и производственных объектах': 'hazard-obj',
    'Благодарность службам': 'gratitude',
    'БПЛА': 'bpla',
    'Взрыв': 'explosion',
    'Внутренний звонок (звонки от работников)': 'internal-call',
    'Вызов на иностранном языке': 'foreign',
    'Дополнительный звонок от заявителя': 'extra-call',
    'Дорожные помехи': 'road-hazards',
    'ДТП': 'dtp',
    'Жалоба на действие или бездействие служб': 'complaint',
    'Животные': 'animals',
    'Консультация': 'consult',
    'Нецелевой вызов': 'non-target',
    'Обрушение': 'collapse',
    'Отзыв о работе 112 Москва': 'feedback-112',
    'Отмена вызова': 'cancel',
    'Ошибочно набран номер': 'wrong-number',
    'Передача дежурства': 'shift-handover',
    'Помощь службам': 'help-services',
    'Природная стихия': 'natural-disaster',
    'Прочие происшествия': 'other',
    'Радиация': 'radiation',
    'Разбитый градусник': 'broken-thermometer',
    'Ребенок в опасности': 'child-danger',
    'Сбор': 'gathering',
    'Скопление воды': 'water-pooling',
    'Смертельный исход': 'death',
    'Социальная помощь': 'social-help',
    'Справка-101': 'ref-101',
    'Справка-102': 'ref-102',
    'Справка-103': 'ref-103',
    'Справка-104': 'ref-104',
    'Справка-ГИБДД': 'ref-gibdd',
    'Справка Городское Хозяйство': 'ref-city',
    'Справка-МЧС': 'ref-mchs',
    'Тестовый вызов': 'test-call',
    'Технический сбой (сбой в работе оборудования 112 Москва)': 'tech-failure',
    'Тренировка': 'training',
    'Уведомление о ЧС': 'emergency-notice',
    'Угроза взрыва/террористического акта': 'blast-threat',
    'Угроза выброса опасных веществ и радиации': 'hazmat-threat',
    'Угроза обрушения': 'collapse-threat',
    'Человек в опасности': 'person-danger',
    'Экологические происшествия': 'ecology',
}

# Быстрые плашки пустой карточки, слева направо, по кадру
# frontend/screenshots/reference/kartochka_01 (СКРИНШОТ КАРТОЧКИ 112, 17.09.2026)
QUICK = ['dtp', 'wrong-number', '104', 'person-danger', 'cancel', 'test-call',
         'shift-handover', 'consult', 'foreign', 'ref-101']


def main() -> None:
    before = {}
    for line in (HERE / 'службы-в-базе-до.txt').read_text(encoding='utf-8').splitlines():
        sid, title, _src = line.split('|')
        before[title] = sid

    rows, ids = [], set(before.values())
    for line in (HERE / 'службы-по-кадрам.txt').read_text(encoding='utf-8').splitlines():
        if line.startswith('#') or not line.strip():
            continue
        frame, title = line.split('|', 1)
        short = title.split(' (', 1)[0]
        if title in before:
            sid, new = before[title], False
        else:
            sid, new = slug(short), True
            assert sid not in ids, (sid, title)
            ids.add(sid)
        rows.append((sid, title, short, int(frame), new))

    types = []
    for line in (HERE / 'типы-по-кадрам.txt').read_text(encoding='utf-8').splitlines():
        frame, title = line.split('|', 1)
        types.append((TYPE_IDS[title], title, int(frame)))
    assert len(types) == len(TYPE_IDS) == 51

    hidden = sorted(t for t in before if t not in {r[1] for r in rows})

    print(f'''-- Справочники служб и типов происшествий — ровно как в боевой системе.
--
-- ИСТОЧНИК: материалы экспертов из чата задачи (21.09.2026):
--   refs/СЛУЖБЫ 112.docx   — окно «Добавьте службы», пролистанное целиком
--                            (image1…image55);
--   refs/КАРТОЧКА 112.docx — выпадающий список «что случилось?»
--                            (image2…image13).
-- Выписки дословно, с номером кадра: backend/docs/справочники/*.txt;
-- этот файл собран скриптом собрать-миграцию.py оттуда же.
--
-- Службы: {len(rows)} на кадрах, из них {sum(r[4] for r in rows)} у нас не было (районные ДДС,
-- префектуры, ОГДЦ, АСУ НС, «112 Мос. обл.», окружные ГБУ АД …). Наши
-- службы, которых в окне нет ({len(hidden)}, столбцы классификатора вроде
-- «Территориальные ОИВ»), не удаляются — на них ссылаются карточки и
-- матрица оповещения, — а скрываются: их нет в service_catalog.
--
-- Скрытая служба не попадает ни в автоподбор, ни в эталон разбора
-- (domain/classify.rs): иначе курсант не смог бы её добавить, а разбор
-- штрафовал бы за её отсутствие. Службы только с кадров — без столбца
-- в классификаторе — автоматически не подбираются и в эталон не входят:
-- их добавляют вручную.
--
-- Между image3 и image4 часть списка в кадр не попала — см.
-- OPEN-QUESTIONS.md, вопрос 23.

-- Порядок окна «Добавьте службы»: позиция — как при прокрутке сверху вниз
CREATE TABLE service_catalog (
    service_id  text PRIMARY KEY REFERENCES services(id),
    position    int  NOT NULL UNIQUE,
    -- Как на плашке в полосе оповещения: часть до скобки
    short_title text NOT NULL,
    -- Номер кадра refs/СЛУЖБЫ 112/imageN
    frame       int  NOT NULL
);

-- Все службы окна — в справочник служб. Название — дословно с кадра.
-- Уже заведённые не трогаем (ON CONFLICT DO NOTHING): их названия сняты
-- с тех же кадров (20260928153000_service_names.sql). На чистом
-- развёртывании миграции идут раньше импорта классификатора, и без этой
-- вставки ссылка из service_catalog упала бы; импорт потом лишь
-- допишет заголовок столбца (source_title)
INSERT INTO services (id, title) VALUES''')
    print(',\n'.join(f'  ({q(sid)}, {q(title)})' for sid, title, _s, _f, _n in rows))
    print('ON CONFLICT (id) DO NOTHING;\n')
    print('INSERT INTO service_catalog (service_id, position, short_title, frame) VALUES')
    print(',\n'.join(f'  ({q(sid)}, {i}, {q(short)}, {frame})'
                     for i, (sid, _t, short, frame, _n) in enumerate(rows, 1)))
    print(';\n')

    print('''-- Типы: все 51 с кадров у нас уже есть. Меняется написание — дословно
-- по выпадающему списку (дефисы, «звонки от работников», «Экологические
-- происшествия») — и порядок: список боевой системы, быстрые плашки —
-- слева направо по кадру пустой карточки.
ALTER TABLE incident_types
    ADD COLUMN listed      boolean NOT NULL DEFAULT true,
    ADD COLUMN sort_order  int,
    ADD COLUMN quick_order int;
''')
    for i, (tid, title, frame) in enumerate(types, 1):
        quick = QUICK.index(tid) + 1 if tid in QUICK else 'NULL'
        print(f'UPDATE incident_types SET title = {q(title)}, sort_order = {i}, '
              f'quick_order = {quick}, frequent = {"true" if tid in QUICK else "false"} '
              f'WHERE id = {q(tid)};  -- image{frame}')
    print('''
-- Типы, которых нет в выпадающем списке боевой системы, скрываем
UPDATE incident_types SET listed = false, frequent = false, quick_order = NULL
 WHERE sort_order IS NULL;''')


if __name__ == '__main__':
    main()
