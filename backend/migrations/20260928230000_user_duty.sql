-- Профиль рабочего места курсанта: оператор 112 или диспетчер ДДС.
--
-- Организатор, чат задачи 18.09 (сообщение 624): «Один интерфейс, одна
-- учетка, две мини роли — либо пользователь заносит данные от
-- потерпевшего <…>, либо пользователь передает карточку в ДДС и там её
-- уже принимают». Обучающегося «могут направить на практику в одно или
-- другое место» — поэтому профиль назначает преподаватель или
-- администратор, а не выбирает сам курсант.
--
-- Интерфейс общий, функционал разный: оператор 112 принимает вызовы и
-- заполняет карточки, диспетчер ДДС работает с карточками своей службы.
-- Права проверяет сервер; служба нужна только диспетчеру — по ней
-- отбирается его лента.

CREATE TYPE user_duty AS ENUM ('op112', 'dds');

ALTER TABLE users ADD COLUMN duty user_duty;

-- Профиль по умолчанию выводится из службы: со службой — ДДС, без
-- неё — оператор 112. Так заведение курсанта без явного профиля
-- (импорт, старые скрипты) не упирается в проверку ниже
CREATE FUNCTION users_default_duty() RETURNS trigger AS $$
BEGIN
  IF NEW.role = 'student' AND NEW.duty IS NULL THEN
    NEW.duty := CASE WHEN NEW.service_id IS NULL THEN 'op112' ELSE 'dds' END;
  END IF;
  IF NEW.role <> 'student' THEN
    NEW.duty := NULL;
  END IF;
  RETURN NEW;
END
$$ LANGUAGE plpgsql;

CREATE TRIGGER users_default_duty
  BEFORE INSERT ON users
  FOR EACH ROW EXECUTE FUNCTION users_default_duty();

-- Учебные записи: student1 — оператор 112 (служба ему больше не
-- нужна), student2…student5 — диспетчеры ДДС своих служб
UPDATE users SET service_id = NULL WHERE login = 'student1' AND role = 'student';

UPDATE users
   SET duty = CASE WHEN service_id IS NULL THEN 'op112'::user_duty ELSE 'dds'::user_duty END
 WHERE role = 'student';

ALTER TABLE users ADD CONSTRAINT users_duty_check CHECK (
  (role <> 'student' AND duty IS NULL)
  OR (role = 'student' AND duty = 'dds' AND service_id IS NOT NULL)
  OR (role = 'student' AND duty = 'op112' AND service_id IS NULL)
);
