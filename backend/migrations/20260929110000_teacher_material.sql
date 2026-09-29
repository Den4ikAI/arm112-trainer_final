-- Учебный материал, который преподаватель собирает сам.
--
-- Учебное происшествие ДДС — это готовая карточка 112, заведённая
-- преподавателем, а не оператором в занятии. Такие карточки к
-- диспетчерам попадают только через занятие (п. 10 замечаний команды:
-- «карточки, заполненные оператором 112, не должны попадать к ДДС,
-- все карточки выдаёт преподаватель»).
ALTER TABLE cards ADD COLUMN IF NOT EXISTS origin text NOT NULL DEFAULT 'session' CHECK (origin IN ('session','teacher'));
-- Тема происшествия — как оно называется в списке преподавателя
ALTER TABLE cards ADD COLUMN IF NOT EXISTS title text;
-- Утверждено преподавателем: только такие видны в составлении занятия
ALTER TABLE cards ADD COLUMN IF NOT EXISTS approved_at timestamptz;
ALTER TABLE cards ADD COLUMN IF NOT EXISTS approved_by uuid REFERENCES users(id);

CREATE INDEX IF NOT EXISTS cards_teacher_idx ON cards (created_at DESC) WHERE origin = 'teacher';

-- Тема учебного вызова: у заведённого вручную или по описанию нет
-- билета, и «Вызов № … от …» ничего не говорит преподавателю
ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS title text;
