-- Рекомендации диспетчеру ДДС и проверка того, сработали ли они.
--
-- Рекомендации финалистам, п. 5 и 10: цепочка «ИИ формирует задание →
-- обучаемый ДДС выполняет → результат → рекомендации для дальнейшего
-- обучения» без преподавателя. Устройство то же, что у рекомендаций
-- оператору 112 (таблица advice): рекомендация пишется один раз после
-- разбора закрытой карточки и не переписывается; вердикт ставится один
-- раз — первой следующей карточкой, где у ошибки была возможность
-- повториться. Отдельная таблица, потому что попытка ДДС — не сеанс
-- (sessions), а работа службы по карточке, и виды замечаний другие.
CREATE TABLE dds_advice (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id          uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    service_id       text NOT NULL,
    -- Карточка, по разбору которой рекомендация выдана
    source_card_id   uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    -- Вид замечания разбора ДДС (domain::brigade::DdsFindingKind)
    kind             text NOT NULL CHECK (kind IN ('late', 'notByFact', 'missed', 'noBrigade',
                                                   'openedLate', 'firstRecordLate', 'firstRecordNoText')),
    -- Этап работ, на котором замечание случилось в последний раз
    stage            text NOT NULL,
    label            text NOT NULL,
    text             text NOT NULL,
    occurrences      int  NOT NULL,
    window_size      int  NOT NULL,
    made_at          timestamptz NOT NULL DEFAULT now(),
    -- Проверка: карточка, по которой решилось, и итог
    check_card_id    uuid REFERENCES cards(id) ON DELETE SET NULL,
    worked           boolean,
    checked_at       timestamptz,
    -- Задание от ИИ, выданное для отработки этой рекомендации
    next_card_id     uuid REFERENCES cards(id) ON DELETE SET NULL,
    CHECK ((worked IS NULL) = (checked_at IS NULL))
);

CREATE INDEX dds_advice_user_idx ON dds_advice (user_id, made_at DESC);

-- На одну ошибку одна открытая рекомендация
CREATE UNIQUE INDEX dds_advice_open_idx ON dds_advice (user_id, kind)
    WHERE checked_at IS NULL;
