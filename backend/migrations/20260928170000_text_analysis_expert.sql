-- Нейросетевой анализ ручного ввода и экспертная оценка преподавателя.
--
-- ТЗ: «оценка уровня подготовки специалистов, в том числе на основе
-- анализа ручного ввода текста», «внедрить систему оценки действий
-- операторов на основе нейросетевого анализа», «просматривать
-- результаты выполнения заданий (автоматическая оценка И + экспертная
-- оценка)».
--
-- Обе таблицы — рядом с автоматическим баллом, а не вместо него:
-- балл правил не переписывается ни моделью, ни преподавателем.

-- Анализ свободного текста курсанта моделью.
--
-- `kind`: card112 — описание со слов заявителя против стенограммы;
-- dds — комментарии диспетчера к статусам против докладов бригады.
-- Одна строка на попытку: повторный анализ перезаписывает её.
CREATE TABLE text_analyses (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    kind        text NOT NULL CHECK (kind IN ('card112', 'dds')),
    card_id     uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    -- У ДДС попытка — это служба на карточке; у 112 службы нет
    service_id  text REFERENCES services(id),
    -- queued — ждёт очереди; ready — готов; unavailable — модель не
    -- подключена; failed — модель ответила, но разобрать не удалось
    status      text NOT NULL DEFAULT 'queued'
                CHECK (status IN ('queued', 'ready', 'unavailable', 'failed')),
    -- Замечания после проверки цитат: [{kind, text, quoteInput, quoteSource}]
    findings    jsonb NOT NULL DEFAULT '[]'::jsonb,
    -- Сколько замечаний модели отброшено: цитата не нашлась в тексте
    dropped     int NOT NULL DEFAULT 0,
    note        text,
    -- Вердикты преподавателя по номерам замечаний:
    -- {"0": {"verdict": "confirmed", "by": uuid, "at": time}}
    verdicts    jsonb NOT NULL DEFAULT '{}'::jsonb,
    model_ms    int,
    created_at  timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    UNIQUE NULLS NOT DISTINCT (kind, card_id, service_id),
    CHECK ((kind = 'dds') = (service_id IS NOT NULL))
);

-- Экспертная оценка преподавателя. Та же адресация попытки, что у анализа
CREATE TABLE expert_grades (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    kind        text NOT NULL CHECK (kind IN ('card112', 'dds')),
    card_id     uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    service_id  text REFERENCES services(id),
    score       int NOT NULL CHECK (score BETWEEN 0 AND 100),
    comment     text NOT NULL DEFAULT '',
    teacher_id  uuid NOT NULL REFERENCES users(id),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE NULLS NOT DISTINCT (kind, card_id, service_id),
    CHECK ((kind = 'dds') = (service_id IS NOT NULL))
);
