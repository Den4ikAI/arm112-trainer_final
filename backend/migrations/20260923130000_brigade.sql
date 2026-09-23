-- Бригада диспетчера ДДС и её доклады о ходе работ.
--
-- До сих пор диспетчер ДДС просто нажимал статусы: источника
-- информации, по которой он их ставит, в тренажёре не было. Памятка
-- ДДС требует проставлять статусы по факту получения информации о
-- соответствующем этапе работ, а проверить это было не на чем.
--
-- Устройство взято из ответов заказчика в чате задачи (21.09.2026,
-- сообщения 684 и 691, см. refs/чат-ответы-заказчика-20-22.09.txt):
--
--   «Для ДДС необходимые бригады выбираются в ручную».
--   «Старший группы звонит в ДДС докладывает об обстановке и ходе
--    работ. Или диспетчер сам через определённое время набирает по
--    телефону старшего. И уточняет ход работ. Работают как правило
--    два варианта».
--   «Специфики нет. <…> Пусть будет универсальный алгоритм для всех».
--
-- Бригады здесь — учебная абстракция тренажёра: «Бригада 1», «Бригада 2».
-- Настоящих составов, номеров и телефонов служб у нас нет, и выдавать
-- их за настоящие нельзя.

-- Бригада, которую диспетчер назначил на карточку. Одна на службу:
-- карточку ведут несколько ДДС сразу, и у каждой своя бригада
CREATE TABLE brigade_assignments (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    card_id     uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    service_id  text NOT NULL REFERENCES services(id),
    brigade_no  int  NOT NULL CHECK (brigade_no > 0),
    user_id     uuid REFERENCES users(id),
    assigned_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (card_id, service_id)
);

-- Этап работ бригады и доклад о нём.
--
-- `due_at` — когда этап наступает на самом деле. Это скрытая истина,
-- как факты заявителя у оператора 112: курсант её не видит, пока не
-- узнает. `delivered_at` — когда диспетчер узнал: старший позвонил сам
-- (`incoming`) или диспетчер позвонил ему (`outgoing`). Разбор считает
-- от момента, когда диспетчер узнал, а не от момента, когда этап
-- наступил: поставить статус раньше доклада — значит угадать
CREATE TABLE brigade_reports (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    assignment_id uuid NOT NULL REFERENCES brigade_assignments(id) ON DELETE CASCADE,
    stage         response_status NOT NULL,
    text          text NOT NULL,
    due_at        timestamptz NOT NULL,
    delivered_at  timestamptz,
    channel       text CHECK (channel IN ('incoming', 'outgoing')),
    UNIQUE (assignment_id, stage),
    CHECK ((delivered_at IS NULL) = (channel IS NULL))
);

-- Доставку ищут на каждом такте фоновой задачи
CREATE INDEX brigade_reports_pending_idx ON brigade_reports (due_at)
    WHERE delivered_at IS NULL;

-- Звонки диспетчера старшему. Нужны разбору: звонил ли диспетчер,
-- когда бригада надолго замолчала, и сколько раз
CREATE TABLE brigade_calls (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    assignment_id uuid NOT NULL REFERENCES brigade_assignments(id) ON DELETE CASCADE,
    user_id       uuid REFERENCES users(id),
    at            timestamptz NOT NULL DEFAULT now(),
    -- Что узнал: последний наступивший этап или NULL, если пока без изменений
    learned       response_status
);

-- Параметры шкалы. Секунды короткие нарочно: это учебное занятие, а не
-- смена, и ждать выезда бригады полчаса курсант не будет. Меняет
-- администратор в разделе настроек, в пределах из api/forecast.rs
INSERT INTO settings (key, value, title, hint) VALUES
    ('brigade.count', '3'::jsonb,
     'Учебных бригад у службы',
     'Из скольких бригад диспетчер ДДС выбирает, кого направить'),
    ('brigade.start_sec', '20'::jsonb,
     'Выезд бригады, секунд после назначения',
     'Когда наступает этап «Начало реагирования»'),
    ('brigade.arrive_sec', '40'::jsonb,
     'Путь до места, секунд',
     'Сколько проходит от выезда до этапа «Прибытие»'),
    ('brigade.work_start_sec', '20'::jsonb,
     'Начало работ, секунд после прибытия',
     'Когда наступает этап «Проведение работ»'),
    ('brigade.work_sec', '60'::jsonb,
     'Работы, секунд',
     'Сколько проходит от начала работ до этапа «Работы завершены»'),
    ('brigade.report_delay_sec', '25'::jsonb,
     'Задержка доклада старшего, секунд',
     'Старший звонит сам спустя столько после этапа. Диспетчер, который звонит ему сам, узнаёт раньше'),
    ('brigade.update_norm_sec', '30'::jsonb,
     'Норматив обновления статуса, секунд',
     'За сколько после доклада диспетчер должен проставить статус этапа')
ON CONFLICT (key) DO NOTHING;
