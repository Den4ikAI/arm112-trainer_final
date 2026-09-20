-- Учебная группа для демонстрации отчётов.
--
-- Синтетические данные стенда, как и сами учебные пользователи:
-- реальных групп заказчик не присылал. Преподаватель — teacher1,
-- в группе все пятеро курсантов, каждый из своей ДДС.

INSERT INTO training_groups (title, teacher_id)
SELECT 'УМЦ-1, поток сентябрь 2026', u.id
  FROM users u WHERE u.login = 'teacher1'
ON CONFLICT DO NOTHING;

INSERT INTO group_members (group_id, user_id)
SELECT g.id, u.id
  FROM training_groups g
  CROSS JOIN users u
 WHERE g.title = 'УМЦ-1, поток сентябрь 2026'
   AND u.login IN ('student1', 'student2', 'student3', 'student4', 'student5')
ON CONFLICT DO NOTHING;
