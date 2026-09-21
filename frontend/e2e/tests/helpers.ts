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
 * Учебные пользователи из миграции 20260921120000.
 * Курсанты сидят в разных ДДС — от этого зависит, что каждый видит.
 */
export const USERS = {
  /** Деп. ЖКХ. В роли оператора 112 заполняет карточки. */
  operator: 'student1',
  /** Классификатор МЧС. В роли диспетчера принимает пожары. */
  mchs: 'student3',
  /** Мослифт. Пожары в его ленту не попадают. */
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
  await page.getByRole('heading', { name: 'Учебные вызовы' }).waitFor();
};

/** Взять вызов билета 3 (горит крыша частного дома) и дождаться карточки. */
export const launchTicket3 = async (page: Page): Promise<void> => {
  await page.getByTestId('launch-t3-c1').click();
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
  execFileSync('docker', [
    'exec', 'arm112-db',
    'psql', '-U', 'arm112', '-d', БАЗА_ПРОГОНА, '-qAtc',
    `UPDATE cards SET saved_at = now() - interval '${seconds} seconds' WHERE number = '${number}'`,
  ]);
};
