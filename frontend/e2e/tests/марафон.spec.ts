import { test, expect, type Page, type TestInfo } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { go, login, PASSWORD } from './helpers';

/**
 * Марафон, блиц и удаление учётной записи.
 *
 * Пожелания команды: «блиц — это обучение на конкретных сегментах
 * карточки, а марафон — это экзамен на 5–10 карточек»; администратор
 * «может удалять и добавлять учеников и преподавателей».
 *
 * У каждого сценария свой курсант, своя группа и свои копии вызова:
 * идущее занятие ограничивает список вызовов всей группы, и на общей
 * учётке соседние проверки мешали бы друг другу.
 */

const БАЗА =
  (process.env.TEST_DATABASE_URL ?? 'arm112_test').split('/').pop() ?? 'arm112_test';

const sql = (запрос: string): string =>
  execFileSync('docker', [
    'exec', 'arm112-db', 'psql', '-U', 'arm112', '-d', БАЗА, '-qAtc', запрос,
  ]).toString().trim();

interface Стенд { login: string; group: string; groupId: string; вызовы: string[] }

/** Курсант (пароль как у учебных), группа преподавателя teacher1 и копии вызова. */
const стенд = (вызовов: number): Стенд => {
  const метка = `${Date.now()}`;
  const login = `марафонец${метка}`;
  const group = `марафон-e2e-${метка}`;
  const userId = sql(
    // Без службы — оператор 112: марафон и блиц по карточке — приём вызова
    `INSERT INTO users (login, password_hash, fio, role)
     SELECT '${login}', password_hash, 'Марафонцев М.М.', 'student'
       FROM users WHERE login = 'student1' RETURNING id`);
  const groupId = sql(
    `INSERT INTO training_groups (title, teacher_id)
     SELECT '${group}', id FROM users WHERE login = 'teacher1' RETURNING id`);
  sql(`INSERT INTO group_members (group_id, user_id) VALUES ('${groupId}', '${userId}')`);

  const образец = sql(
    `SELECT id FROM scenarios WHERE status = 'approved' AND ticket = 3 AND call_no = 1 LIMIT 1`);
  const вызовы: string[] = [];
  for (let i = 0; i < вызовов; i++) {
    const id = sql(
      `INSERT INTO scenarios (legend, applicant_name, applicant_phone, applicant_status,
                              norm_sec, expected, version_id, status, created_by)
       SELECT legend, applicant_name, applicant_phone, applicant_status,
              norm_sec, expected, version_id, 'approved', created_by
         FROM scenarios WHERE id = '${образец}' RETURNING id`);
    sql(`INSERT INTO scenario_hidden_facts (scenario_id, key, question, answer, unlocks, sort_order)
         SELECT '${id}', key, question, answer, unlocks, sort_order
           FROM scenario_hidden_facts WHERE scenario_id = '${образец}'`);
    вызовы.push(id);
  }
  return { login, group, groupId, вызовы };
};

const прибрать = (с: Стенд) => {
  sql(`UPDATE lessons SET finished_at = now()
        WHERE group_id = '${с.groupId}' AND started_at IS NOT NULL AND finished_at IS NULL`);
  sql(`DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE login = '${с.login}')`);
  sql(`DELETE FROM assignments WHERE lesson_id IN (SELECT id FROM lessons WHERE group_id = '${с.groupId}')`);
  sql(`DELETE FROM lessons WHERE group_id = '${с.groupId}'`);
  sql(`DELETE FROM training_groups WHERE id = '${с.groupId}'`);
  sql(`UPDATE audit_log SET user_id = NULL
        WHERE user_id = (SELECT id FROM users WHERE login = '${с.login}')`);
  // Сохранение карточки ставит анализ текста в очередь от имени курсанта
  sql(`DELETE FROM ml_jobs WHERE created_by = (SELECT id FROM users WHERE login = '${с.login}')`);
  sql(`DELETE FROM users WHERE login = '${с.login}'`);
  sql(`DELETE FROM scenarios WHERE id IN (${с.вызовы.map((v) => `'${v}'`).join(', ')})`);
};

const токен = async (page: Page, login: string): Promise<string> => {
  const вход = await page.request.post('/api/auth/login', { data: { login, password: PASSWORD } });
  return (await вход.json()).token;
};

/** Сохранить пустую карточку единственного доступного вызова. */
const пройти_вызов = async (page: Page) => {
  const кнопки = page.locator('section[aria-label="Учебные вызовы"] button');
  await expect(кнопки).toHaveCount(1);
  await кнопки.first().click();
  await page.getByTestId('card-timer').waitFor();
  await page.getByTestId('save-card').click();
};

test.describe('Марафон', () => {
  test('пять вызовов подряд, разбор — только после последнего', async ({ page }, info: TestInfo) => {
    test.setTimeout(180_000);
    const с = стенд(5);
    try {
      // Преподаватель собирает марафон из пяти выбранных вызовов
      await login(page, info, 'teacher1');
      await page.getByTestId('lessons-link').click();
      await page.getByTestId('lesson-group').selectOption({ label: с.group });
      await page.getByTestId('lesson-title').fill('Экзамен');
      await page.getByTestId('lesson-kind').selectOption('marathon');
      for (const v of с.вызовы) await page.getByTestId(`lesson-pick-${v}`).check();
      await expect(page.getByTestId('lesson-count')).toHaveValue('5');
      await page.getByTestId('lesson-create').click();
      await expect(page.getByTestId('lesson-kind-value')).toHaveText('марафон');
      await page.getByTestId('lesson-start').click();
      await expect(page.getByTestId('lesson-state')).toHaveText('идёт');

      // Курсант идёт по порядку; разбора между вызовами нет
      await login(page, info, с.login);
      for (let n = 1; n <= 4; n++) {
        await expect(page.getByTestId('lesson-banner')).toContainText(`вызов ${n} из 5`);
        await пройти_вызов(page);
        // После сохранения — сразу к следующему вызову, не на разбор
        await expect(page.getByTestId('lesson-banner')).toContainText(`вызов ${n + 1} из 5`);
        await expect(page).not.toHaveURL(/debrief/);
      }
      await пройти_вызов(page);
      // Последний вызов — разбор открылся
      await expect(page.getByTestId('score')).toBeVisible();
      await page.getByRole('button', { name: 'Вернуться в журнал' }).click();
      await expect(page.getByTestId('marathon-result')).toContainText('пройден');
      await expect(page.getByTestId('marathon-result').locator('li')).toHaveCount(5);

      // Преподаватель видит протокол
      await login(page, info, 'teacher1');
      await page.getByTestId('lessons-link').click();
      await page.getByRole('button', { name: /Экзамен/ }).first().click();
      await expect(page.getByTestId('lesson-protocol')).toContainText('5 / 5');
    } finally {
      прибрать(с);
    }
  });
});

test.describe('Блиц', () => {
  test('разбор сразу после задания и только по сегменту', async ({ page }, info) => {
    const с = стенд(3);
    try {
      const teacher = await токен(page, 'teacher1');
      const создано = await page.request.post('/api/lessons', {
        headers: { authorization: `Bearer ${teacher}` },
        data: { groupId: с.groupId, title: 'Блиц адреса', mode: 'card112',
                kind: 'blitz', segment: 'address', scenarioIds: с.вызовы },
      });
      expect(создано.status()).toBe(201);
      const { id } = await создано.json();
      const пуск = await page.request.post(`/api/lessons/${id}/start`,
        { headers: { authorization: `Bearer ${teacher}` } });
      expect(пуск.status()).toBe(200);

      await login(page, info, с.login);
      await expect(page.getByTestId('lesson-banner')).toContainText('оценивается только «Адрес»');
      await page.locator('section[aria-label="Учебные вызовы"] button').first().click();
      await page.getByTestId('card-timer').waitFor();
      await page.getByTestId('save-card').click();

      await expect(page.getByRole('heading', { name: 'Разбор блица' })).toBeVisible();
      await expect(page.getByTestId('blitz-segment')).toContainText('Адрес');
      // Службы в блице по адресу не оцениваются — и не показываются
      await expect(page.getByText('Требуемые службы направлены')).toHaveCount(0);
      await page.getByRole('button', { name: 'К следующему заданию' }).click();
      await expect(page.getByTestId('lesson-banner')).toContainText('Пройдено 1 из 3');
    } finally {
      прибрать(с);
    }
  });
});

test.describe('Удаление учётной записи', () => {
  test('администратор удаляет запись с подтверждением', async ({ page }, info) => {
    const admin = await токен(page, 'admin');
    const логин = `удаляемый${Date.now()}`;
    const ответ = await page.request.post('/api/admin/users', {
      headers: { authorization: `Bearer ${admin}` },
      data: { login: логин, password: 'длинныйПароль', fio: 'Удаляев У.У.',
              role: 'teacher', serviceId: null },
    });
    expect(ответ.status()).toBe(201);

    await login(page, info, 'admin');
    await go(page, info, '/admin');
    await page.getByTestId('admin-tab-users').click();
    const строка = page.getByTestId(`user-${логин}`);
    await expect(строка).toBeVisible();

    page.once('dialog', (d) => void d.accept());
    await page.getByTestId(`delete-${логин}`).click();
    await expect(page.getByTestId('user-notice')).toContainText('удалена');
    await expect(строка).toHaveCount(0);
  });
});
