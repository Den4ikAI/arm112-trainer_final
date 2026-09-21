-- Занятие, которым управляет преподаватель.
--
-- Осторожно с терминами. В ТЗ «занятие» — это то, что ведёт
-- преподаватель: он выбирает тему, запускает, завершает. У нас
-- в коде `sessions` — это один вызов, отработанный курсантом.
-- Поэтому здесь `lessons` — занятие, а `sessions` — вызов внутри него.
--
-- Источник вызовов: ТЗ (§10, сценарий «действия с карточками») даёт
-- преподавателю выбор из сгенерированных системой, составленных
-- самими обучающимися и смешанного набора.
CREATE TYPE lesson_source AS ENUM ('generated', 'by_students', 'mixed');

CREATE TABLE lessons (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id    uuid NOT NULL REFERENCES training_groups(id) ON DELETE CASCADE,
    teacher_id  uuid NOT NULL REFERENCES users(id),
    title       text NOT NULL,
    mode        session_mode NOT NULL,
    -- Категории происшествий, выбранные под тему занятия. Пустой
    -- список означает «любые»: ТЗ допускает множественный выбор,
    -- но не требует выбирать хоть что-то
    type_ids    text[] NOT NULL DEFAULT '{}',
    source      lesson_source NOT NULL DEFAULT 'generated',
    started_at  timestamptz,
    finished_at timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now(),
    -- Завершить можно только начатое
    CHECK (finished_at IS NULL OR started_at IS NOT NULL)
);

CREATE INDEX lessons_group_idx ON lessons (group_id, created_at DESC);
-- Идущее занятие ищется на каждом запросе списка вызовов у курсанта
CREATE INDEX lessons_running_idx ON lessons (group_id)
    WHERE started_at IS NOT NULL AND finished_at IS NULL;

-- Задания раздаются занятием. Существующие задания без занятия
-- остаются: их можно выдать и вне занятия, персонально.
ALTER TABLE assignments
    ADD COLUMN lesson_id uuid REFERENCES lessons(id) ON DELETE CASCADE;

CREATE INDEX assignments_lesson_idx ON assignments (lesson_id);

-- Материал занятия в режиме ДДС — набор карточек, с которыми
-- работает группа. ТЗ (§10, «действия с карточками»): преподаватель
-- выбирает, брать ли карточки, сформированные обучающимися, или
-- созданные системой, или и те и другие.
--
-- Для режима «карточка 112» материал раздаётся заданиями: там
-- у каждого курсанта свой вызов. Здесь набор общий: одну карточку
-- ведут несколько ДДС сразу, и это часть упражнения.
CREATE TABLE lesson_cards (
    lesson_id uuid NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    card_id   uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    PRIMARY KEY (lesson_id, card_id)
);
