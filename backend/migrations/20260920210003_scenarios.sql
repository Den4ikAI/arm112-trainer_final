-- Учебные сценарии.
-- Важно: expected НЕ пишется моделью, он выводится из позиции
-- классификатора. Модель заполняет только текстовую обвязку.

CREATE TYPE scenario_status AS ENUM ('draft', 'approved', 'archived');

CREATE TABLE scenarios (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    ticket           int,
    call_no          int,
    legend           text NOT NULL,      -- первая фраза заявителя
    applicant_name   text NOT NULL,
    applicant_phone  text NOT NULL,
    applicant_status text NOT NULL,
    norm_sec         int  NOT NULL DEFAULT 60,
    expected         jsonb NOT NULL,     -- {typeIds, answers, address, services}
    version_id       uuid REFERENCES classifier_versions(id),
    status           scenario_status NOT NULL DEFAULT 'draft',
    created_by       uuid REFERENCES users(id),
    approved_by      uuid REFERENCES users(id),
    created_at       timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX scenarios_status_idx ON scenarios (status);

-- Скрытые факты: заявитель сообщает их только в ответ на вопрос.
-- Так составлены бумажные билеты, где настоящий адрес напечатан
-- в скобках, а заявитель называет лишь ориентир.
CREATE TABLE scenario_hidden_facts (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    scenario_id uuid NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    key         text NOT NULL,           -- 'city', 'victims', 'gas'
    question    text NOT NULL,
    answer      text NOT NULL,
    unlocks     text[] NOT NULL,         -- ['address.settlement']
    sort_order  int NOT NULL,
    UNIQUE (scenario_id, key)
);
