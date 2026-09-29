-- Экзамен (марафон) бывает и у диспетчеров ДДС: снимаем ограничение
-- «марафон только у приёма вызова 112».
ALTER TABLE lessons DROP CONSTRAINT IF EXISTS lessons_marathon_card112;

-- Учебные происшествия ДДС, выбранные преподавателем поимённо, в порядке
-- выдачи. Пусто — подбираются случайно из созданных преподавателем.
ALTER TABLE lessons ADD COLUMN IF NOT EXISTS card_ids uuid[] NOT NULL DEFAULT '{}';
