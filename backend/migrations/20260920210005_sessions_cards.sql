-- Занятия, карточки происшествий, статусы реагирования.

CREATE TYPE session_mode AS ENUM ('card112', 'dds');

CREATE TABLE sessions (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       uuid NOT NULL REFERENCES users(id),
    scenario_id   uuid REFERENCES scenarios(id),
    assignment_id uuid REFERENCES assignments(id),
    mode          session_mode NOT NULL,
    started_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz,
    norm_sec      int NOT NULL
);

CREATE INDEX sessions_user_idx ON sessions (user_id, started_at DESC);
-- Быстрый поиск активных занятий: админ не может менять данные во время них
CREATE INDEX sessions_active_idx ON sessions (user_id) WHERE finished_at IS NULL;

CREATE TYPE card_status AS ENUM (
    'Зарегистрирована', 'Отработана', 'Проверена',
    'Не оповещено', 'Отказ', 'Не завершено', 'Завершена'
);

CREATE TABLE cards (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    number           text UNIQUE NOT NULL,
    session_id       uuid REFERENCES sessions(id) ON DELETE CASCADE,
    created_by       uuid REFERENCES users(id),
    workstation      text,
    phone_aon        text NOT NULL DEFAULT '',
    phone_provided   text NOT NULL DEFAULT '',
    phone_on_site    text NOT NULL DEFAULT '',
    applicant_name   text NOT NULL DEFAULT '',
    applicant_status text NOT NULL DEFAULT '',
    address          jsonb NOT NULL DEFAULT '{}'::jsonb,
    description      text NOT NULL DEFAULT '',
    victims          int,
    no_contact       boolean NOT NULL DEFAULT false,
    call_dropped     boolean NOT NULL DEFAULT false,
    type_ids         text[] NOT NULL DEFAULT '{}',
    answers          jsonb NOT NULL DEFAULT '{}'::jsonb,
    outcome_type     text,               -- рассчитан классификатором
    status           card_status NOT NULL DEFAULT 'Зарегистрирована',
    created_at       timestamptz NOT NULL DEFAULT now(),
    saved_at         timestamptz
);

CREATE INDEX cards_created_idx ON cards (created_at DESC);
CREATE INDEX cards_answers_idx ON cards USING gin (answers);

CREATE TYPE service_source AS ENUM ('auto', 'manual', 'vis');

CREATE TABLE card_services (
    card_id    uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    service_id text NOT NULL REFERENCES services(id),
    -- auto нельзя удалить вручную: требование инструкции, раздел 6
    source     service_source NOT NULL DEFAULT 'auto',
    added_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (card_id, service_id)
);

-- Фильтр ленты по своей ДДС: «в ленту попадают только профильные события»
CREATE INDEX card_services_service_idx ON card_services (service_id);

CREATE TYPE response_status AS ENUM (
    'Добавлена', 'Получена службой', 'Принята', 'Не принята',
    'Начало реагирования', 'Прибытие', 'Проведение работ',
    'Работы завершены', 'Отказ от выполнения работ'
);

CREATE TABLE response_entries (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    card_id    uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    service_id text NOT NULL REFERENCES services(id),
    status     response_status NOT NULL,
    at         timestamptz NOT NULL DEFAULT now(),
    operator   text NOT NULL,            -- 'оп. 0' у ВИС, ФИО у АРМ-112
    user_id    uuid REFERENCES users(id),
    order_no   text,
    comment    text
);

CREATE INDEX response_entries_card_idx ON response_entries (card_id, service_id, at);
