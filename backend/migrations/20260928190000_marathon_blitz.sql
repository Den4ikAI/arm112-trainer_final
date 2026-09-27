-- Марафон и блиц — виды занятия, а не отдельные подсистемы.
--
-- Пожелание команды: «блиц — это обучение на конкретных сегментах
-- карточки, а марафон — это экзамен на 5–10 карточек». Оба живут
-- на том же механизме занятий: группа, задания, запуск и завершение
-- преподавателем, ход и отчёт. Отличаются правилами:
--
--   марафон — вызовы идут строго по порядку, одна попытка на вызов,
--             разбора и рекомендаций между вызовами нет, в конце —
--             итоговый протокол;
--   блиц    — серия коротких заданий, оценивается только выбранный
--             сегмент карточки, разбор — сразу после каждого.
CREATE TYPE lesson_kind AS ENUM ('training', 'marathon', 'blitz');

ALTER TABLE lessons
    ADD COLUMN kind lesson_kind NOT NULL DEFAULT 'training',
    -- Сегмент блица. У приёма вызова — адрес, тип с опросной картой
    -- или службы; у ДДС — статусы по докладам бригады
    ADD COLUMN segment text
        CHECK (segment IN ('address', 'type', 'services', 'statuses')),
    -- Сколько вызовов раздать, если преподаватель не выбрал их сам
    ADD COLUMN call_count int CHECK (call_count BETWEEN 3 AND 10),
    -- Вызовы, выбранные преподавателем, по порядку прохождения.
    -- Пусто — подбирает система по категориям занятия
    ADD COLUMN scenario_ids uuid[] NOT NULL DEFAULT '{}',
    ADD CONSTRAINT lessons_blitz_segment CHECK ((kind = 'blitz') = (segment IS NOT NULL)),
    ADD CONSTRAINT lessons_marathon_card112 CHECK (kind <> 'marathon' OR mode = 'card112');

-- Порядок вызова внутри занятия. В марафоне по нему курсант идёт
-- от первого вызова к последнему
ALTER TABLE assignments ADD COLUMN seq int;

-- Удаление учётной записи администратором.
--
-- ТЗ запрещает удалять критически важные данные без резервного
-- копирования. Учётная запись с историей обучения не стирается:
-- её данные сначала копируются сюда, а сама запись обезличивается —
-- оценки и журнал действий остаются читаемыми, человек из системы
-- исчезает
ALTER TABLE users ADD COLUMN deleted_at timestamptz;

CREATE TABLE user_backups (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Без внешнего ключа: копия должна пережить саму запись
    user_id    uuid NOT NULL,
    data       jsonb NOT NULL,
    created_by uuid REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX user_backups_user_idx ON user_backups (user_id);
