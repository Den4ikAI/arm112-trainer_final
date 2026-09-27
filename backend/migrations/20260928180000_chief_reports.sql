-- Доклад диспетчера ДДС вышестоящему начальнику.
--
-- Заказчик в чате задачи, 27.09.2026 (сообщение 781, п. 7): голосовое
-- общение «ДДС с вышестоящим начальником и ДДС с руководителем
-- бригады». С бригадой тренажёр уже разговаривает (brigade_*), здесь —
-- второй собеседник. Что именно докладывают начальнику, в материалах
-- не сказано; состав доклада — наше допущение, OPEN-QUESTIONS.md,
-- вопрос 22.
--
-- Один доклад — один звонок: диспетчер докладывает, начальник задаёт
-- уточняющие вопросы по тому, чего не услышал, и говорит «Принял».
-- Звонить можно повторно: разбор показывает последний доклад.
CREATE TABLE chief_reports (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    card_id     uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    service_id  text NOT NULL REFERENCES services(id),
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    started_at  timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    -- Разбор доклада: какие сведения доложены, какие нет, какие неверно.
    -- Пишется, когда начальник сказал «Принял»
    review      jsonb
);

CREATE INDEX chief_reports_card_idx ON chief_reports (card_id, service_id, started_at DESC);

-- Незаконченный звонок у диспетчера по карточке один: второй «начать»
-- возвращает тот же
CREATE UNIQUE INDEX chief_reports_open_idx ON chief_reports (card_id, service_id, user_id)
    WHERE finished_at IS NULL;

CREATE TABLE chief_report_lines (
    id        bigserial PRIMARY KEY,
    report_id uuid NOT NULL REFERENCES chief_reports(id) ON DELETE CASCADE,
    who       text NOT NULL CHECK (who IN ('dispatcher', 'chief')),
    text      text NOT NULL,
    -- О каком сведении спросил начальник: разбор не повторяет вопрос
    -- и видит, на что диспетчер отвечал
    topic     text,
    -- Сколько думала модель над этой репликой начальника
    model_ms  int,
    at        timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX chief_report_lines_report_idx ON chief_report_lines (report_id, id);

-- Модель в самом разговоре. На процессоре её реплика — 20–50 секунд
-- (замер: backend/docs/замеры/доклад-начальнику.md), в телефонном
-- разговоре такая пауза недопустима. Поэтому по умолчанию начальник
-- переспрашивает по ключевым словам без задержки, а модель разбирает
-- доклад один раз, когда он сказал «Принял». На видеокарте модель
-- отвечает за секунды — там разговор можно вести моделью целиком
INSERT INTO settings (key, value, title, hint) VALUES
    ('chief.live_model', '0'::jsonb,
     'Модель в разговоре с начальником',
     '1 — начальник переспрашивает по разбору модели после каждой реплики (нужна быстрая модель, на видеокарте). 0 — во время звонка вопросы по ключевым словам, модель разбирает доклад один раз после «Принял»')
ON CONFLICT (key) DO NOTHING;
