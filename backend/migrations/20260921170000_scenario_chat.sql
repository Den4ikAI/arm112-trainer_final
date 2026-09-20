-- Переписка преподавателя с моделью о сценарии.
--
-- ТЗ: «в случае некорректного формирования вопроса или ответа
-- у Преподавателя имеется контекстное поле, куда он может ввести
-- комментарий, который должен быть отработан системой для
-- корректировки вопроса или ответа».
--
-- Хранится именно переписка, а не последний комментарий: правка
-- редко получается с первого раза, и модели нужен контекст
-- предыдущих ходов. Заодно видно, как сценарий дошёл до нынешнего
-- вида — это и есть «валидация» из требований к преподавателю.

CREATE TYPE chat_role AS ENUM ('teacher', 'model', 'system');

CREATE TABLE scenario_chat (
    id          bigserial PRIMARY KEY,
    scenario_id uuid NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    role        chat_role NOT NULL,
    content     text NOT NULL,
    -- Что именно модель предложила изменить. Нужен, чтобы показать
    -- правку до применения и чтобы можно было откатиться
    patch       jsonb,
    user_id     uuid REFERENCES users(id),
    at          timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX scenario_chat_idx ON scenario_chat (scenario_id, at, id);

-- Откуда сценарий взялся: из билета, от модели или написан руками.
-- Преподаватель должен видеть разницу, прежде чем утверждать
ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS source text NOT NULL DEFAULT 'manual';
ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS source_ref text;
-- Чего не хватило классификатору. Непустое означает, что эталон
-- неполон и утверждать сценарий рано
ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS classify_reason text;
