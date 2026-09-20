-- Типы происшествий и опросные карты.
-- Формулировки сняты со скриншотов боевой системы и менять их нельзя:
-- курсант должен видеть ровно тот текст, что на рабочем АРМ.

CREATE TABLE incident_types (
    id       text PRIMARY KEY,           -- '101', 'dtp', 'bpla'
    title    text NOT NULL,
    frequent boolean NOT NULL DEFAULT false,
    group_no int
);

CREATE TYPE answer_kind AS ENUM ('single', 'multi', 'text', 'number');

CREATE TABLE survey_questions (
    id         text NOT NULL,            -- 'where', 'fire-sign'
    type_id    text NOT NULL REFERENCES incident_types(id) ON DELETE CASCADE,
    label      text NOT NULL,
    kind       answer_kind NOT NULL,
    sort_order int NOT NULL,
    PRIMARY KEY (type_id, id)
);

CREATE TABLE survey_options (
    type_id     text NOT NULL,
    question_id text NOT NULL,
    value       text NOT NULL,
    sort_order  int NOT NULL,
    PRIMARY KEY (type_id, question_id, value),
    FOREIGN KEY (type_id, question_id)
        REFERENCES survey_questions(type_id, id) ON DELETE CASCADE
);
