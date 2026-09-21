-- Прогноз результата курсанта и проверка самого прогноза.
--
-- Критерий финальной экспертизы требует сравнения прогнозных
-- значений с реальными. Чтобы такое сравнение что-то значило,
-- прогноз должен быть зафиксирован ДО события и не переписываться
-- задним числом. Отсюда устройство таблицы: `predicted` и `made_at`
-- пишутся один раз при старте вызова, `actual` и `checked_at` —
-- один раз после разбора.
CREATE TABLE predictions (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Один прогноз на вызов: повторный старт того же вызова прогноз
    -- не меняет, иначе можно было бы подогнать его под факт
    session_id uuid UNIQUE NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    predicted  int NOT NULL CHECK (predicted BETWEEN 0 AND 100),
    -- Признаки, на которых построен прогноз, и вклад каждого:
    -- прогноз без объяснения преподаватель не может ни проверить,
    -- ни оспорить
    features   jsonb NOT NULL,
    -- Сколько разборов легло в основу и была ли модель обучена.
    -- Прогноз на трёх занятиях и на пятидесяти — разного веса,
    -- и это должно быть видно
    basis      int  NOT NULL,
    trained    boolean NOT NULL,
    made_at    timestamptz NOT NULL DEFAULT now(),
    actual     int CHECK (actual BETWEEN 0 AND 100),
    checked_at timestamptz,
    CHECK ((actual IS NULL) = (checked_at IS NULL))
);

CREATE INDEX predictions_user_idx ON predictions (user_id, made_at DESC);
-- Достоверность считается по проверенным прогнозам
CREATE INDEX predictions_checked_idx ON predictions (checked_at)
    WHERE checked_at IS NOT NULL;

-- Настраиваемые параметры.
--
-- Отдельный подпункт критерия финальной экспертизы — «реализация
-- дополнительных настраиваемых параметров». Заодно сюда переезжают
-- нормативы, которые до сих пор были зашиты константами.
CREATE TABLE settings (
    key        text PRIMARY KEY,
    value      jsonb NOT NULL,
    title      text NOT NULL,
    hint       text NOT NULL DEFAULT '',
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid REFERENCES users(id)
);

INSERT INTO settings (key, value, title, hint) VALUES
    ('forecast.tolerance', '10'::jsonb,
     'Допуск прогноза, баллов',
     'Прогноз считается попавшим, если разошёлся с фактом не больше чем на столько'),
    ('forecast.depth', '5'::jsonb,
     'Глубина истории, занятий',
     'Сколько последних разборов курсанта учитывается в признаках'),
    ('forecast.lambda', '0.1'::jsonb,
     'Штраф регрессии',
     'Больше значение — осторожнее модель. При нуле коэффициенты разлетаются'),
    ('attestation.threshold', '70'::jsonb,
     'Порог аттестации, баллов',
     'Ниже этого балла курсант считается не готовым'),
    ('norm.ack_sec', '30'::jsonb,
     'Норматив подтверждения ДДС, секунд',
     'Памятка ДДС, раздел «Проставление статусов реагирования»'),
    ('norm.completion_hours', '48'::jsonb,
     'Норматив завершения работ, часов',
     'После этого срока незавершённая карточка переходит в «Не завершено»');
