-- Учебные группы и задания.

CREATE TABLE training_groups (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    title      text NOT NULL,
    teacher_id uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE group_members (
    group_id uuid NOT NULL REFERENCES training_groups(id) ON DELETE CASCADE,
    user_id  uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);

CREATE TABLE assignments (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id    uuid REFERENCES training_groups(id),
    user_id     uuid REFERENCES users(id),
    scenario_id uuid NOT NULL REFERENCES scenarios(id),
    norm_sec    int,                     -- переопределяет сценарный
    due_at      timestamptz,
    created_by  uuid NOT NULL REFERENCES users(id),
    created_at  timestamptz NOT NULL DEFAULT now(),
    -- задание выдаётся либо группе, либо конкретному человеку
    CHECK (group_id IS NOT NULL OR user_id IS NOT NULL)
);
