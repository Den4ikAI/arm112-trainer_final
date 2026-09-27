-- Карточки занятия ДДС приходят потоком, а не все сразу.
--
-- Ответ заказчика в чате задачи (23.09.2026, сообщение 709): карточки
-- приходят «одновременно, как в текущей работе. В этом ещё доп
-- мотивация — когда работаешь с карточкой, тайминг идёт и для этой
-- работы и для тех карточек, что в очереди». Темп заказчик не задал:
-- интервал — параметр занятия, его выбирает преподаватель. Значение
-- по умолчанию, 60 секунд, — наше допущение (PROVENANCE.md).
ALTER TABLE lessons
    ADD COLUMN card_interval_sec integer NOT NULL DEFAULT 60
        CHECK (card_interval_sec BETWEEN 10 AND 900);

-- Когда карточка появляется в ленте курсанта. От этого же момента
-- считаются нормативы ДДС: 30 секунд на открытие, 3 минуты на первую
-- запись. Считать их от сохранения карточки нельзя: в материал занятия
-- идут и старые карточки, у них нормативы вышли бы с первой секунды.
--
-- `announced` — разослано ли событие о появлении карточки: фоновая
-- задача рассылает его один раз, когда наступает `release_at`.
ALTER TABLE lesson_cards
    ADD COLUMN release_at timestamptz,
    ADD COLUMN announced  boolean NOT NULL DEFAULT false;

-- Уже розданные карточки считаем поступившими в момент старта занятия:
-- раньше они так и появлялись — все сразу
UPDATE lesson_cards lc
   SET release_at = COALESCE(l.started_at, now()),
       announced  = true
  FROM lessons l
 WHERE l.id = lc.lesson_id;

ALTER TABLE lesson_cards
    ALTER COLUMN release_at SET NOT NULL,
    ALTER COLUMN release_at SET DEFAULT now();

CREATE INDEX lesson_cards_pending_idx ON lesson_cards (release_at) WHERE NOT announced;
