-- Финал: телефонный справочник ДДС формирует преподаватель.
--
-- Рекомендации финалистам, п. 8: «перечень специалистов и номера
-- условных телефонов должен формироваться преподавателем не более
-- 10 номеров для одной ДДС»; абоненты — руководитель организации,
-- начальник отдела реагирования, начальник аварийной бригады,
-- специалист аварийной бригады. Заявителю диспетчер звонит только
-- если преподаватель включил его номер в перечень — тогда диспетчер
-- набирает номер заявителя из карточки.
--
-- Служба без своих записей получает справочник по умолчанию (код,
-- api::phone::defaults) — те же номера, что раздаются здесь, чтобы
-- заведённые позже службы не оставались без телефона.
CREATE TABLE phone_book (
    id         bigserial PRIMARY KEY,
    service_id text NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    -- org_head — руководитель организации, response_head — начальник
    -- отдела реагирования, brigade_head — начальник аварийной бригады,
    -- brigade_specialist — специалист аварийной бригады, applicant —
    -- заявитель: номер берётся из карточки, здесь он пустой
    role       text NOT NULL CHECK (role IN
                 ('org_head', 'response_head', 'brigade_head', 'brigade_specialist', 'applicant')),
    number     text NOT NULL,
    title      text NOT NULL,
    position   int  NOT NULL DEFAULT 0,
    UNIQUE (service_id, number)
);
CREATE INDEX phone_book_service ON phone_book (service_id, position);

-- Прежние собеседники телефона — старший бригады (brigade) и
-- вышестоящий начальник (chief) — остаются в журнале как есть
ALTER TABLE phone_calls DROP CONSTRAINT phone_calls_peer_check;
ALTER TABLE phone_calls ADD CONSTRAINT phone_calls_peer_check CHECK (peer IN
    ('brigade', 'chief', 'org_head', 'response_head', 'brigade_head', 'brigade_specialist', 'applicant'));
-- Как абонент назывался в справочнике в момент вызова: преподаватель
-- может переименовать запись, журнал от этого не меняется
ALTER TABLE phone_calls ADD COLUMN title text NOT NULL DEFAULT '';

-- Справочник по умолчанию для нынешних служб — по прежней схеме
-- номеров: сотня службы = 2000 + 20 × место в справочнике служб
-- (службы вне справочника — в общей сотне 2980), бригада — первые
-- номера, начальники — последние.
WITH base AS (
    SELECT s.id AS service_id,
           CASE WHEN c.position BETWEEN 1 AND 48 THEN 2000 + 20 * c.position ELSE 2980 END AS base
      FROM services s LEFT JOIN service_catalog c ON c.service_id = s.id
)
INSERT INTO phone_book (service_id, role, number, title, position)
SELECT service_id, role, (base + off)::text, title, pos
  FROM base, (VALUES
      ('brigade_head',       1,  'Начальник аварийной бригады',   1),
      ('brigade_specialist', 2,  'Специалист аварийной бригады',  2),
      ('response_head',      19, 'Начальник отдела реагирования', 3),
      ('org_head',           18, 'Руководитель организации',      4)
  ) AS r(role, off, title, pos)
ON CONFLICT DO NOTHING;
