-- Рекомендации курсанту и проверка того, сработали ли они.
--
-- Подпункт критерия финальной экспертизы: «проверка рекомендаций
-- сервиса и реальных рекомендаций в соответствии с реальными
-- условиями». Устройство то же, что у прогноза: рекомендация пишется
-- один раз после разбора, где ошибка повторилась, и не переписывается;
-- вердикт ставится один раз — в первом следующем разборе, где у
-- ошибки была возможность повториться. Иначе проверка превратилась бы
-- в самоподтверждение.
CREATE TABLE advice (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id           uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Разбор, после которого рекомендация выдана
    source_session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    -- Ошибка: поле карточки и вид замечания, как в grades.findings
    field             text NOT NULL,
    kind              text NOT NULL CHECK (kind IN ('guessed', 'wrong', 'missing', 'extra')),
    -- Подпись поля с экрана — чтобы формулировка читалась и через
    -- месяц, когда подписи в справочнике могли поменяться
    label             text NOT NULL,
    text              text NOT NULL,
    -- Сколько раз ошибка встретилась и в каком окне разборов:
    -- рекомендация по двум случаям и по пяти — разного веса
    occurrences       int  NOT NULL,
    window_size       int  NOT NULL,
    made_at           timestamptz NOT NULL DEFAULT now(),
    -- Проверка: разбор, в котором решилось, и итог
    check_session_id  uuid REFERENCES sessions(id) ON DELETE SET NULL,
    worked            boolean,
    checked_at        timestamptz,
    CHECK ((worked IS NULL) = (checked_at IS NULL))
);

CREATE INDEX advice_user_idx ON advice (user_id, made_at DESC);
CREATE INDEX advice_check_idx ON advice (check_session_id);

-- На одну ошибку одна открытая рекомендация: пока прежняя не
-- проверена, повторять её бессмысленно
CREATE UNIQUE INDEX advice_open_idx ON advice (user_id, field, kind)
    WHERE checked_at IS NULL;

INSERT INTO settings (key, value, title, hint) VALUES
    ('advice.window', '5'::jsonb,
     'Окно рекомендаций, разборов',
     'Ошибка считается повторяющейся, если встретилась дважды за столько последних разборов');
