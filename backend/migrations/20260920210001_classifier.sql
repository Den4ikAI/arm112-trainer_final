-- Классификатор происшествий и его версии.
-- Версионирование обязательно: классификатор живой, между версиями
-- из материалов задачи появились 8 служб и позиция «БПЛА».

CREATE TABLE classifier_versions (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name            text NOT NULL,
    source_file     text,
    imported_at     timestamptz NOT NULL DEFAULT now(),
    imported_by     uuid REFERENCES users(id),
    is_active       boolean NOT NULL DEFAULT false,
    positions_count int NOT NULL DEFAULT 0
);

-- Активная версия может быть только одна
CREATE UNIQUE INDEX classifier_versions_active_idx
    ON classifier_versions (is_active) WHERE is_active;

CREATE TABLE classifier_positions (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    version_id   uuid NOT NULL REFERENCES classifier_versions(id) ON DELETE CASCADE,
    number       text NOT NULL,          -- '1010101', '24120200'
    group_no     int  NOT NULL,          -- 1..24
    group_title  text NOT NULL,
    sign1        text,                   -- 112-Признак.1
    sign2        text,
    sign3        text,
    extra_signs  text,
    outcome_type text,                   -- итоговый тип: 'пожар: мусор'
    ekp_type     text,                   -- ТИП происшествия ЕКП 35
    UNIQUE (version_id, number)
);

CREATE INDEX classifier_positions_group_idx ON classifier_positions (version_id, group_no);

-- Матрица «позиция × служба»: одна позиция может оповещать службу
-- по-разному в зависимости от условия («угроза людям», «пострадавшие»)
CREATE TABLE classifier_position_services (
    position_id uuid NOT NULL REFERENCES classifier_positions(id) ON DELETE CASCADE,
    service_id  text NOT NULL REFERENCES services(id),
    condition   text NOT NULL DEFAULT '',
    value       text,
    PRIMARY KEY (position_id, service_id, condition)
);
