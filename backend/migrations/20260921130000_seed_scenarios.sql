-- Учебные сценарии из бумажных билетов.
--
-- ПРОИСХОЖДЕНИЕ. Источник — «Билеты-задачи по С112», скан распознан,
-- полный текст в extracted/bilety_96_scenarios.md.
--   Дословно из билета: ситуация, ФИО заявителя, телефон, адрес
--   и скрытая часть адреса (в билете она напечатана в скобках).
--   Написано нами: прямая речь. Билет даёт сухое описание, реплики
--   заявителя — наша реконструкция того же содержания. Фактов сверх
--   билета в них нет, но это не цитаты.
--
-- expected.services НЕ придуман: он прогнан через движок
-- domain::classify на импортированном классификаторе v046_24
-- (POST /api/classifier/evaluate), сюда записан его ответ —
-- только службы реагирования, без информационных копий.
--
-- Билет 1 (горящий мусорный контейнер на улице) здесь НЕТ: опросная
-- карта 101 в нашей версии не содержит ветку «Улица», и признак
-- второго уровня задать нечем. Записано в OPEN-QUESTIONS.md.

INSERT INTO scenarios
  (ticket, call_no, legend, applicant_name, applicant_phone, applicant_status,
   norm_sec, expected, status)
VALUES
  (3, 1,
   'Алло! Горит крыша частного дома! Пожалуйста, приезжайте скорее, это на Станционной улице, дом 28!',
   'Иванова Инна Степановна', '916-126-34-71', 'очевидец', 60,
   '{"typeIds":["101"],
     "answers":{"where":["Дом"],"fire-sign":["Открытое пламя / Дым"],
                "house-kind":["Дом частный"],"people-threat":["Нет"],
                "inhouse":["Крыша"],"gasified":["Нет"]},
     "address":{"region":"Московская область","settlement":"Королёв",
                "street":"Станционная","house":"28"},
     "services":["gup-msr","departament-rbipk-gku-mosbez","mvd","mchs","codd"]}'::jsonb,
   'approved'),

  (2, 1,
   'Здравствуйте, у нас в доме мусоропровод задымил, дым в подъезде идёт. Берзарина, 21, корпус 1.',
   'Ким Олег Юрьевич', '916-126-34-71', 'очевидец', 60,
   '{"typeIds":["101"],
     "answers":{"where":["Дом"],"fire-sign":["Открытое пламя / Дым"],
                "house-kind":["Дом многоквартирный"],"inhouse":["Мусоропровод"],
                "people-threat":["Нет"],"storeys":"17"},
     "address":{"region":"Москва","street":"Берзарина","house":"21",
                "building":"1","entrance":"3","code":"68"},
     "services":["departament-rbipk-gku-mosbez","mvd","mchs","codd"]}'::jsonb,
   'approved');

-- Скрытые факты: заявитель сообщает их ТОЛЬКО в ответ на вопрос.
-- Механика заложена авторами билетов: настоящий адрес напечатан
-- в скобках, а заявитель называет ориентир. Курсант, заполнивший
-- поле без вопроса, угадал, а не выяснил.
INSERT INTO scenario_hidden_facts (scenario_id, key, question, answer, unlocks, sort_order)
SELECT s.id, v.key, v.question, v.answer, v.unlocks, v.sort_order
FROM scenarios s
JOIN (VALUES
  (3, 1, 'city',     'Уточните, пожалуйста, город — вы в Москве?',
                     'Нет, что вы, это Королёв, Московская область! Улица Станционная, 28.',
                     ARRAY['address.settlement','address.region'], 1),
  (3, 1, 'victims',  'Пострадавшие есть?',
                     'Нет, пострадавших нет, все вышли.',
                     ARRAY['victims'], 2),
  (3, 1, 'gas',      'Дом газифицирован?',
                     'Нет, дом не газифицирован.',
                     ARRAY['answers.gasified'], 3),
  (3, 1, 'house',    'Что за дом — частный, многоквартирный?',
                     'Частный дом, одноэтажный. Горит крыша.',
                     ARRAY['answers.house-kind','answers.inhouse'], 4),
  (3, 1, 'flame',    'Открытое пламя видно или только дым?',
                     'Пламя видно, крыша горит открыто.',
                     ARRAY['answers.fire-sign'], 5),

  (2, 1, 'floor',    'На каком вы этаже, сколько этажей в доме?',
                     'Я на седьмом, в доме семнадцать этажей.',
                     ARRAY['answers.storeys','address.floor'], 1),
  (2, 1, 'flame',    'Открытое пламя видите?',
                     'Нет, открытого пламени не вижу, только дым.',
                     ARRAY['answers.fire-sign'], 2),
  (2, 1, 'victims',  'Пострадавшие есть?',
                     'Нет, пострадавших нет.',
                     ARRAY['victims'], 3),
  (2, 1, 'entrance', 'Номер подъезда и домофон подскажете?',
                     'Подъезд третий, домофон 68.',
                     ARRAY['address.entrance','address.code'], 4)
) AS v(ticket, call_no, key, question, answer, unlocks, sort_order)
  ON v.ticket = s.ticket AND v.call_no = s.call_no
ON CONFLICT (scenario_id, key) DO NOTHING;
