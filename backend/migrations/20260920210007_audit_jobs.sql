-- Аудит и очередь фоновых задач.

-- Партиционирование по месяцам: требование ТЗ — хранить журналы
-- безопасности не менее 6 месяцев.
CREATE TABLE audit_log (
    id        bigserial,
    at        timestamptz NOT NULL DEFAULT now(),
    user_id   uuid REFERENCES users(id),
    action    text NOT NULL,             -- 'card.save', 'grade.override'
    entity    text,
    entity_id text,
    payload   jsonb,
    ip        inet,
    PRIMARY KEY (id, at)
) PARTITION BY RANGE (at);

-- Партиции на ближайшие месяцы; дальше их создаёт фоновая задача
CREATE TABLE audit_log_2026_09 PARTITION OF audit_log
    FOR VALUES FROM ('2026-09-01') TO ('2026-10-01');
CREATE TABLE audit_log_2026_10 PARTITION OF audit_log
    FOR VALUES FROM ('2026-10-01') TO ('2026-11-01');
CREATE TABLE audit_log_2026_11 PARTITION OF audit_log
    FOR VALUES FROM ('2026-11-01') TO ('2026-12-01');

CREATE INDEX audit_log_user_idx ON audit_log (user_id, at DESC);
CREATE INDEX audit_log_action_idx ON audit_log (action, at DESC);

CREATE TYPE job_status AS ENUM ('queued', 'running', 'done', 'failed');

-- Очередь на таблице: брокер для MVP избыточен.
-- Воркер забирает задачи через FOR UPDATE SKIP LOCKED.
CREATE TABLE ml_jobs (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    kind        text NOT NULL,           -- 'scenario.generate', 'insights'
    status      job_status NOT NULL DEFAULT 'queued',
    input       jsonb NOT NULL,
    output      jsonb,
    error       text,
    created_by  uuid REFERENCES users(id),
    created_at  timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz
);

CREATE INDEX ml_jobs_queue_idx ON ml_jobs (status, created_at) WHERE status = 'queued';
