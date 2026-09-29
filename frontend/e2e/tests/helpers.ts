import type { Page, TestInfo } from '@playwright/test';
import { execFileSync } from 'node:child_process';

/**
 * Переход по маршруту приложения.
 *
 * Приложение на HashRouter, поэтому «#» обязан быть частью пути.
 * Если держать его в baseURL, абсолютный путь вроде `/admin` его
 * затрёт, и вместо маршрута откроется корень — что и случилось:
 * тесты входа и журнала это не замечали, потому что корень и так
 * перебрасывает куда надо.
 */
export const go = async (page: Page, _info: TestInfo, path: string): Promise<void> => {
  await page.goto(`/#${path}`);
};

/**
 * Учебные пользователи из миграций 20260921120000 и 20260928230000.
 * У курсанта — профиль рабочего места: оператор 112 принимает вызовы
 * и заполняет карточки, диспетчер ДДС работает с лентой своей службы.
 */
export const USERS = {
  /** Оператор 112: принимает вызовы и заполняет карточки. Службы нет. */
  operator: 'student1',
  /** Диспетчер ДДС, Служба 101 (МЧС): принимает пожары. */
  mchs: 'student3',
  /** Диспетчер ДДС, Мослифт: пожары в его ленту не попадают. */
  lift: 'student2',
} as const;

export const PASSWORD = 'arm112';

/**
 * Вход и переход в журнал происшествий.
 *
 * Перед входом гасим сохранённый токен: в одном сценарии мы меняем
 * пользователя (оператор заполнил карточку — диспетчер её принял),
 * а вошедшего приложение с формы входа уводит обратно в журнал.
 */
export const login = async (
  page: Page,
  info: TestInfo,
  user: string = USERS.operator,
): Promise<void> => {
  await go(page, info, '/login');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.locator('input[name="login"]').fill(user);
  await page.locator('input[name="password"]').fill(PASSWORD);
  await page.getByTestId('login-submit').click();
  // У курсанта — разделы выданного, у преподавателя и администратора —
  // журнал с поиском. Оператору 112 списка происшествий не положено
  await page.getByTestId('student-work').or(page.getByLabel('Поиск происшествий'))
    .first().waitFor();
};

/** Логин вошедшего пользователя — по его токену. */
const кто = (page: Page): Promise<string> => page.evaluate(async () => {
  const t = localStorage.getItem('arm112.token');
  const r = await fetch('/api/auth/me', { headers: { Authorization: `Bearer ${t}` } });
  return (await r.json()).login as string;
});

/**
 * Выдать оператору 112 вызов билета — как это делает преподаватель
 * занятием (Денис, 29.09: вызовы выдаёт только преподаватель).
 * Прежние выдачи этим помощником закрываются, чтобы в разделе была одна
 * кнопка начала этого вызова.
 */
export const выдатьВызов = (login: string, ticket = 3, callNo = 1): void => {
  sql(`
    DO $$
    DECLARE u uuid; g uuid; t uuid; sc uuid; l uuid;
    BEGIN
      SELECT id INTO u FROM users WHERE login = '${login}';
      SELECT id INTO t FROM users WHERE login = 'teacher1';
      SELECT id INTO sc FROM scenarios WHERE ticket = ${ticket} AND call_no = ${callNo}
        AND status = 'approved' LIMIT 1;
      SELECT group_id INTO g FROM group_members WHERE user_id = u LIMIT 1;
      IF g IS NULL THEN
        INSERT INTO training_groups (title, teacher_id)
          VALUES ('выдача ' || gen_random_uuid(), t) RETURNING id INTO g;
        INSERT INTO group_members (group_id, user_id) VALUES (g, u);
      END IF;
      UPDATE lessons SET finished_at = now()
       WHERE group_id = g AND title = 'выдача вызова' AND finished_at IS NULL;
      INSERT INTO lessons (group_id, teacher_id, title, mode, started_at)
        VALUES (g, t, 'выдача вызова', 'card112', now()) RETURNING id INTO l;
      INSERT INTO assignments (lesson_id, group_id, user_id, scenario_id, created_by, seq)
        VALUES (l, g, u, sc, t, 1);
    END $$;`);
};

/**
 * Выдать диспетчеру ДДС карточку как учебное происшествие преподавателя.
 * Карточки курсантов 112 к диспетчерам сами не попадают (Денис, 29.09).
 */
export const выдатьПроисшествие = (number: string, login: string = USERS.mchs): void => {
  sql(`
    DO $$
    DECLARE u uuid; g uuid; t uuid; c uuid; l uuid;
    BEGIN
      SELECT id INTO u FROM users WHERE login = '${login}';
      SELECT id INTO t FROM users WHERE login = 'teacher1';
      SELECT id INTO c FROM cards WHERE number = '${number}';
      UPDATE cards SET origin = 'teacher' WHERE id = c;
      SELECT group_id INTO g FROM group_members WHERE user_id = u LIMIT 1;
      IF g IS NULL THEN
        INSERT INTO training_groups (title, teacher_id)
          VALUES ('выдача ' || gen_random_uuid(), t) RETURNING id INTO g;
        INSERT INTO group_members (group_id, user_id) VALUES (g, u);
      END IF;
      INSERT INTO lessons (group_id, teacher_id, title, mode, started_at, finished_at)
        VALUES (g, t, 'выдача происшествия', 'dds', now(), now()) RETURNING id INTO l;
      INSERT INTO lesson_cards (lesson_id, card_id, release_at) VALUES (l, c, now());
    END $$;`);
};

/** Взять вызов билета 3 (горит крыша частного дома) и дождаться карточки. */
export const launchTicket3 = async (page: Page): Promise<void> => {
  выдатьВызов(await кто(page), 3, 1);
  await page.reload();
  await page.getByTestId('section-training').click();
  await page.getByTestId('launch-t3-c1').first().click();
  await page.getByTestId('card-timer').waitFor();
};

/**
 * Заполнить признаки пожара в частном доме.
 * Комбинация даёт «пожар: частный дом» — проверено на настоящем
 * классификаторе (backend/tests/grade_api.rs).
 */
export const fillFireCard = async (page: Page): Promise<void> => {
  await page.locator('#type-search').fill('101');
  await page.getByRole('option', { name: '101', exact: true }).click();

  // exact: иначе «Нет» в газификации совпадает и с «Нет данных»
  const pick = async (group: string, option: string) => {
    await page.getByRole('group', { name: group })
      .getByRole('button', { name: option, exact: true }).click();
  };
  await pick('Где', 'Дом');
  await pick('Признак пожара (дом)', 'Открытое пламя / Дым');
  await pick('Дом (пламя, дым)', 'Дом частный');
  await pick('Внутридомовые объекты (пламя, дым)', 'Крыша');
  await pick('Угроза людям', 'Нет');
  await pick('Проведена ли газификация', 'Нет');
};

/**
 * Сдвинуть момент сохранения карточки в прошлое.
 *
 * Норматив подтверждения — 30 секунд, норматив завершения — 48 часов.
 * Ждать их в тесте нельзя, поэтому двигаем время в базе. Тот же приём
 * используют серверные тесты (backend/tests/watchdog.rs).
 */
/**
 * Состарить карточку, чтобы проверить сторож нормативов.
 *
 * База берётся та же, в которой работает прогон: она отдельная от
 * стенда, иначе проверка меняла бы данные, которые кто-то смотрит.
 */
const БАЗА_ПРОГОНА =
  (process.env.TEST_DATABASE_URL ?? 'arm112_test').split('/').pop() ?? 'arm112_test';

export const ageCard = (number: string, seconds: number): void => {
  sql(`UPDATE cards SET saved_at = now() - interval '${seconds} seconds' WHERE number = '${number}';
       UPDATE lesson_cards SET release_at = now() - interval '${seconds} seconds'
        WHERE card_id = (SELECT id FROM cards WHERE number = '${number}')
          AND lesson_id IN (SELECT id FROM lessons WHERE title = 'выдача происшествия')`);
};

/** SQL в базе прогона. */
export function sql(query: string): string {
  return execFileSync('docker', [
    'exec', 'arm112-db',
    'psql', '-U', 'arm112', '-d', БАЗА_ПРОГОНА, '-qAtc', query,
  ]).toString();
}
