-- Учебные пользователи.
--
-- Пароли захешированы argon2id. У всех один пароль: arm112
-- Это учебный стенд в изолированном контуре, а не продуктив.
--
-- Курсанты привязаны к разным службам: от этого зависит, какие карточки
-- каждый видит в журнале. Требование ТЗ — «чтобы в их ленту попадали
-- только профильные события». Плюс это делает осмысленным мультиплеер:
-- одно происшествие уходит сразу в несколько служб.

-- Службы, к которым привязаны учебные курсанты.
--
-- Строки создаёт импорт классификатора (ingest::classifier::store), но он
-- запускается уже после миграций, а users.service_id ссылается на services.
-- Поэтому нужные пять служб создаём здесь. Идентификаторы и названия взяты
-- из самого классификатора (лист «Службы»), ничего не придумано:
-- при импорте строки будут обновлены тем же значением.
INSERT INTO services (id, title) VALUES
  ('dep-zhkh', 'Деп. ЖКХ'),
  ('moslift',  'Мослифт'),
  ('mchs',     'Классификатор МЧС'),
  ('codd',     'ЦОДД'),
  ('mosgaz',   'Классификатор МОСГАЗ')
ON CONFLICT (id) DO NOTHING;

INSERT INTO users (login, password_hash, fio, role, service_id, workstation) VALUES
  ('admin',    '$argon2id$v=19$m=19456,t=2,p=1$FFg46coaKUo1D4z/HdyQng$PtRWe4/PMRMsKF/K5xb0nQaFf2x7h1yvBSHTqESkAPU',
   'Администратор системы', 'admin',   NULL,        NULL),
  ('teacher1', '$argon2id$v=19$m=19456,t=2,p=1$FFg46coaKUo1D4z/HdyQng$PtRWe4/PMRMsKF/K5xb0nQaFf2x7h1yvBSHTqESkAPU',
   'Преподаватель УМЦ',     'teacher', NULL,        NULL),
  ('student1', '$argon2id$v=19$m=19456,t=2,p=1$FFg46coaKUo1D4z/HdyQng$PtRWe4/PMRMsKF/K5xb0nQaFf2x7h1yvBSHTqESkAPU',
   'Иванов И.И.',           'student', 'dep-zhkh',  '4'),
  ('student2', '$argon2id$v=19$m=19456,t=2,p=1$FFg46coaKUo1D4z/HdyQng$PtRWe4/PMRMsKF/K5xb0nQaFf2x7h1yvBSHTqESkAPU',
   'Петров П.П.',           'student', 'moslift',   '5'),
  ('student3', '$argon2id$v=19$m=19456,t=2,p=1$FFg46coaKUo1D4z/HdyQng$PtRWe4/PMRMsKF/K5xb0nQaFf2x7h1yvBSHTqESkAPU',
   'Сидоров С.С.',          'student', 'mchs',      '6'),
  ('student4', '$argon2id$v=19$m=19456,t=2,p=1$FFg46coaKUo1D4z/HdyQng$PtRWe4/PMRMsKF/K5xb0nQaFf2x7h1yvBSHTqESkAPU',
   'Кузнецов К.К.',         'student', 'codd',      '7'),
  ('student5', '$argon2id$v=19$m=19456,t=2,p=1$FFg46coaKUo1D4z/HdyQng$PtRWe4/PMRMsKF/K5xb0nQaFf2x7h1yvBSHTqESkAPU',
   'Смирнов С.С.',          'student', 'mosgaz',    '8')
ON CONFLICT (login) DO NOTHING;
