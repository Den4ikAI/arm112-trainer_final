-- Номер карточки.
--
-- В боевой системе это восьмизначное число (на кадрах — 36814851).
-- Формат воспроизводим, чтобы курсант видел привычный номер, а не uuid.
-- Стартовое значение взято с кадра и продолжается дальше.
CREATE SEQUENCE card_number_seq START WITH 36814851;

ALTER TABLE cards ALTER COLUMN number SET DEFAULT nextval('card_number_seq')::text;
