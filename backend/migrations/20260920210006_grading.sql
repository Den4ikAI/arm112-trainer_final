-- Стенограмма разговора, выясненные факты, оценка.
-- Три слоя, которые сравнивает движок оценки:
--   scenario.expected  — скрытая истина
--   transcript_lines   — что курсант реально выяснил
--   cards              — что он ввёл

CREATE TABLE transcript_lines (
    id         bigserial PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    who        text NOT NULL,            -- 'оператор' | 'заявитель'
    text       text NOT NULL,
    at_sec     int  NOT NULL             -- смещение от старта занятия
);

CREATE INDEX transcript_session_idx ON transcript_lines (session_id, at_sec);

CREATE TABLE revealed_facts (
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    fact_id    uuid NOT NULL REFERENCES scenario_hidden_facts(id),
    at_sec     int  NOT NULL,
    PRIMARY KEY (session_id, fact_id)
);

CREATE TABLE grades (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id  uuid UNIQUE NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    score       int  NOT NULL,
    elapsed_sec int  NOT NULL,
    norm_sec    int  NOT NULL,
    over_norm   boolean NOT NULL,
    findings    jsonb NOT NULL,          -- Finding[]
    services    jsonb NOT NULL,          -- {expected, actual, missed, extra}
    created_at  timestamptz NOT NULL DEFAULT now()
);
