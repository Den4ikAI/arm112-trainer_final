import { expect, test, type Locator } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { Screen, Studio } from './студия';

/**
 * Ролик финала: преподаватель и два обучаемых на трёх рабочих местах
 * одновременно. Пишется тремя контекстами браузера на одном экземпляре
 * тренажёра; собирается docs/demo/assemble_multi.py.
 *
 *   DEMO_OUT=<каталог>      кадры и звук (по умолчанию /tmp/arm112-demo/final)
 *   DEMO_DB=<база>          база экземпляра (нужна, чтобы прочитать выданное)
 *   DEMO_PARTS=main|phone|admin   какой ролик снимать (по умолчанию main)
 */

const PASSWORD = 'arm112';
const DB = process.env.DEMO_DB ?? 'arm112_demo_final';

const sql = (query: string): string =>
  execFileSync('docker', ['exec', 'arm112-db', 'psql', '-U', 'arm112', '-d', DB, '-qAtc', query])
    .toString().trim();

const visible = async (l: Locator): Promise<boolean> => (await l.count()) > 0 && l.first().isVisible();

const login = async (s: Screen, user: string): Promise<void> => {
  const p = s.page;
  await p.goto('/#/login');
  await p.evaluate(() => localStorage.clear());
  await p.reload();
  await s.type(p.locator('input[name="login"]'), user, 55);
  await s.type(p.locator('input[name="password"]'), PASSWORD, 35);
  await s.click(p.getByTestId('login-submit'));
  await p.getByTestId('student-work').or(p.getByLabel('Поиск происшествий'))
    .first().waitFor({ timeout: 60_000 });
  await s.pause(400);
};

/** Запись статуса своей службы. */
const status = async (s: Screen, svc: string, value: string, comment?: string): Promise<void> => {
  const p = s.page;
  await s.click(p.getByTestId(`pencil-${svc}`));
  await s.select(p.getByRole('combobox', { name: 'Статус' }), value);
  if (comment) await s.type(p.getByRole('textbox', { name: 'Комментарий' }), comment, 22);
  await s.click(p.getByRole('button', { name: 'Сохранить статус' }));
  await expect(p.getByTestId(`service-tab-${svc}`)).toContainText(value);
};

/** Входящий от старшего бригады: дождаться, открыть, принять, выслушать, положить. */
const answerBrigade = async (st: Studio, s: Screen, timeoutSec: number): Promise<string> => {
  const p = s.page;
  const launcher = p.getByTestId('phone-launcher');
  try {
    await st.fast(3, () => expect(launcher).toHaveAttribute('data-ringing', 'true', { timeout: timeoutSec * 1000 }));
  } catch {
    return '';
  }
  if (!(await visible(p.getByTestId('ip-phone')))) await s.click(launcher);
  await s.click(p.getByTestId('phone-accept'));
  await expect(p.getByTestId('phone-timer')).toBeVisible({ timeout: 10_000 });
  await s.pause(600);
  const lines = p.getByTestId('phone-lines');
  const said = (await lines.innerText().catch(() => '')).split('\n').filter(Boolean).pop() ?? '';
  await st.app(said.replace(/^[^:]{0,40}:\s*/, ''), 'brigade');
  await s.click(p.getByTestId('phone-hangup'));
  return said;
};

/** Один этап работ бригады: доклад по телефону → статус по факту. */
const stage = async (st: Studio, s: Screen, svc: string, value: string, text?: string): Promise<void> => {
  const said = await answerBrigade(st, s, 60);
  if (text) await st.sayWhile(text, () => status(s, svc, value));
  else await status(s, svc, value);
  void said;
};

interface Student { s: Screen; login: string; svc: string; number?: string }

/** Обучаемый входит и берёт у ИИ новое происшествие для своей службы. */
const takeAiIncident = async (st: Studio, u: Student, intro: string, during: string): Promise<void> => {
  const p = u.s.page;
  st.show(u.s);
  await st.sayWhile(intro, () => login(u.s, u.login));
  await st.note(u.s, 'ИИ готовит происшествие…', async () => {
    await st.sayWhile(during, async () => {
      await u.s.click(p.getByTestId('practice-ai'));
      await st.fast(2, () => expect(p).toHaveURL(/#\/card-dds\//, { timeout: 180_000 }));
      await expect(p.getByTestId('brigade-panel')).toBeVisible({ timeout: 60_000 });
    });
  });
  u.number = (await p.getByTestId('card-number').innerText().catch(() => '')).trim();
  await u.s.ring(p.getByTestId('card-class'));
  await st.say('Карточка пришла в его ленту, пошёл норматив: тридцать секунд на открытие, три минуты на первую запись.');
  await u.s.ring(null);
};

test('финал: преподаватель и два обучаемых', async ({ browser, baseURL }) => {
  test.setTimeout(2_400_000);
  const out = process.env.DEMO_OUT ?? path.join('/tmp/arm112-demo', 'final');
  const st = new Studio(out);
  const T = await st.add(browser, 'teacher', 'ПК преподавателя · teacher1', baseURL!);
  const A = await st.add(browser, 'dds01', 'ПК обучаемого 1 · dds01 · Служба 101, пожарная охрана', baseURL!);
  const B = await st.add(browser, 'dds02', 'ПК обучаемого 2 · dds02 · Служба 104, газовая служба', baseURL!);
  const a: Student = { s: A, login: 'dds01', svc: 'mchs' };
  const b: Student = { s: B, login: 'dds02', svc: 'mosgaz' };

  const errors: string[] = [];
  const scene = async (name: string, run: () => Promise<void>) => {
    try { await run(); } catch (e) {
      errors.push(`${name}: ${String(e).split('\n')[0]}`);
      console.error(`сцена ${name} упала:`, e);
    }
  };

  await st.start();
  try {
    // Заставка
    st.card('Тренажёр диспетчера ДДС', 'Преподаватель и два обучаемых — три рабочих места в одной сети');
    await st.say('Три рабочих места в одной сети: преподаватель и два диспетчера разных служб — пожарной охраны и газовой.');
    await st.say('Покажем, как ИИ выдаёт каждому своё задание, оценивает работу и советует, что тренировать дальше.');
    st.hideCard();

    // Преподаватель собирает и запускает занятие
    await scene('teacher-start', async () => {
      const p = T.page;
      st.show(T);
      await st.sayWhile('Преподаватель входит под своей учётной записью.', () => login(T, 'teacher1'));
      await T.click(p.getByTestId('lessons-link'));
      await st.sayWhile('Собирает занятие для группы диспетчеров: режим «работа со статусами».', async () => {
        await T.select(p.getByTestId('lesson-group'), { label: 'Диспетчеры ДДС — группа 1' });
        await T.type(p.getByTestId('lesson-title'), 'Пожары и утечки газа', 45);
        await T.select(p.getByTestId('lesson-mode'), 'dds');
      });
      await T.click(p.getByTestId('lesson-create'));
      await expect(p.getByTestId('lesson-current')).toHaveText('Пожары и утечки газа', { timeout: 30_000 });
      await st.sayWhile('И запускает его. Монитор занятия показывает каждое рабочее место группы.', async () => {
        await T.click(p.getByTestId('lesson-start'));
        await expect(p.getByTestId('lesson-state')).toHaveText('идёт', { timeout: 30_000 });
        await T.click(p.getByTestId('lesson-monitor-link'));
        await expect(p.getByTestId('lesson-monitor')).toBeVisible({ timeout: 30_000 });
      });
      await T.ring(p.getByTestId('monitor-status'));
      await st.say('Занятие само раздаёт группе карточки по очереди. А задание можно взять и у ИИ самому — это и покажем.');
      await T.ring(null);
    });

    // Обучаемый 1 — подсказки и задание от ИИ
    await scene('a-task', async () => {
      const p = A.page;
      st.show(A);
      await st.sayWhile('Первый обучаемый — диспетчер пожарной охраны, служба [101|сто один]. Входит под своим логином.',
        () => login(A, 'dds01'));
      const hints = p.getByTestId('hints-toggle');
      if (await visible(hints)) {
        await st.sayWhile('Новичку помогут подсказки: каждое окно обведено и объяснено.', async () => {
          await A.click(hints);
          await expect(p.getByTestId('hints-layer')).toBeVisible();
          await A.pause(1200);
          const next = p.getByTestId('hints-next');
          if (await visible(next)) await A.click(next);
          await A.pause(1200);
        });
        await A.click(p.getByTestId('hints-close'));
      }
      await A.ring(p.getByTestId('practice-ai'));
      await st.say('Кнопка «Новое происшествие от ИИ»: задание собирает модель, без преподавателя.');
      await A.ring(null);
      await st.note(A, 'ИИ готовит происшествие…', async () => {
        await st.sayWhile('Признаки и службы — по классификатору, легенду пишет языковая модель — под службу этого диспетчера.', async () => {
          await A.click(p.getByTestId('practice-ai'));
          await st.fast(2, () => expect(p).toHaveURL(/#\/card-dds\//, { timeout: 180_000 }));
          await expect(p.getByTestId('brigade-panel')).toBeVisible({ timeout: 60_000 });
        });
      });
      a.number = (await p.getByTestId('card-number').innerText().catch(() => '')).trim();
      await A.ring(p.getByTestId('card-class'));
      await st.say('Карточка пришла в ленту, пошёл норматив: тридцать секунд на открытие, три минуты на первую запись.');
      await A.ring(null);
    });

    // Обучаемый 2 — своё задание
    await scene('b-task', async () => {
      await takeAiIncident(st, b,
        'Второй обучаемый — диспетчер газовой службы, служба [104|сто четыре]. Входит на своём месте.',
        'Нажимает ту же кнопку — и получает другое задание: ИИ собирает происшествие под его службу, утечку газа.');
    });

    // Обучаемый 1 — работа по карточке
    await scene('a-work', async () => {
      const p = A.page;
      st.show(A);
      await st.sayWhile('Первый диспетчер принимает карточку: статус «Принята» и комментарий — это первая запись.',
        () => status(A, a.svc, 'Принята', 'Принято, направляю бригаду'));
      await st.sayWhile('И направляет бригаду.', async () => {
        await A.select(p.getByTestId('brigade-select'), '1');
        await A.click(p.getByTestId('brigade-assign'));
        await expect(p.getByTestId('brigade-panel')).toContainText('№ 1', { timeout: 30_000 });
      });
      await A.ring(p.getByTestId('phone-launcher'));
      await st.say('Зелёный кружок — IP-телефон. Старший бригады позвонит сам, как только выедет.');
      await A.ring(null);
      await stage(st, A, a.svc, 'Начало реагирования',
        'Доклад принят по телефону — диспетчер ставит статус по факту.');
      await stage(st, A, a.svc, 'Прибытие', 'Бригада прибыла — следующий доклад, следующий статус.');
      await stage(st, A, a.svc, 'Проведение работ', 'Приступили к работам.');
    });

    // Обучаемый 2 — параллельно работает по своей карточке
    await scene('b-work', async () => {
      const p = B.page;
      st.show(B);
      await st.sayWhile('Второй диспетчер в это же время работает по своей карточке: принимает и направляет бригаду.', async () => {
        await status(B, b.svc, 'Принята', 'Принято, бригада направляется');
        await B.select(p.getByTestId('brigade-select'), '1');
        await B.click(p.getByTestId('brigade-assign'));
        await expect(p.getByTestId('brigade-panel')).toContainText('№ 1', { timeout: 30_000 });
      });
      await stage(st, B, b.svc, 'Начало реагирования', 'Его бригада тоже докладывает по телефону.');
    });

    // Преподаватель видит ход занятия
    await scene('teacher-monitor', async () => {
      const p = T.page;
      st.show(T);
      await T.page.bringToFront().catch(() => {});
      const seats = p.locator('[data-testid^="seat-"]');
      if (await visible(seats)) await T.moveTo(seats.first());
      await st.say('У преподавателя на мониторе оба места: номер карточки, текущий статус, время от открытия и опоздания по нормативу.');
    });

    // Обучаемый 1 — доклад начальнику, завершение, разбор, рекомендация
    await scene('a-finish', async () => {
      const p = A.page;
      st.show(A);
      // Доклад начальнику по телефону — пока карточка в работе
      const launcher = p.getByTestId('phone-launcher');
      if (await visible(launcher)) {
        if (!(await visible(p.getByTestId('ip-phone')))) await A.click(launcher);
        const book = p.getByTestId('phone-tab-book');
        if (await visible(book)) await A.click(book);
        const chief = p.getByTestId('phone-call-response_head');
        if (await visible(chief) && await chief.isEnabled()) {
          await st.sayWhile('Пока бригада работает, диспетчер докладывает начальнику — по телефону, из телефонной книги.', async () => {
            await A.click(chief);
            await expect(p.getByTestId('phone-timer')).toBeVisible({ timeout: 15_000 });
          });
          const lines = p.getByTestId('phone-lines').locator('li');
          await st.app((await lines.first().innerText().catch(() => '')).replace(/^[^:]{0,40}:\s*/, ''), 'chief');
          await A.type(p.getByTestId('phone-text'),
            'Докладываю: пожар, бригада один отработала, работы завершены, пострадавших нет.', 20);
          await A.click(p.getByTestId('phone-send'));
          await expect(lines).toHaveCount(3, { timeout: 60_000 });
          await st.app((await lines.last().innerText().catch(() => '')).replace(/^[^:]{0,40}:\s*/, ''), 'chief');
          await A.click(p.getByTestId('phone-hangup'));
          await st.sayWhile('Положил трубку — модель сверяет доклад с карточкой: что сказано, что упущено.', async () => {
            await expect(p.getByTestId('chief-report')).toContainText('Принял', { timeout: 90_000 }).catch(() => {});
          });
          const review = p.getByTestId('chief-review');
          if (await visible(review)) { await A.moveTo(review); await A.ring(review); await st.pause(2500); await A.ring(null); }
          if (await visible(p.getByTestId('phone-close'))) await A.click(p.getByTestId('phone-close'));
        }
      }
      await stage(st, A, a.svc, 'Работы завершены', 'Работы завершены — карточка закрывается, и ИИ разбирает всё, что сделал диспетчер.');
      await expect(p.getByTestId('dds-score')).toBeVisible({ timeout: 60_000 });
      await A.moveTo(p.getByTestId('dds-score'));
      await A.ring(p.getByTestId('dds-score'));
      await st.say('Балл и замечания: вовремя ли открыл, ставил ли статусы по докладам, полны ли записи.');
      await A.ring(null);
      const advice = p.getByTestId('my-dds-advice');
      if (await visible(advice)) {
        await A.moveTo(advice);
        await A.ring(advice);
        await st.say('И рекомендация — что тренировать дальше.');
        await A.ring(null);
      }
    });

    // Обучаемый 2 — завершение и следующее задание от ИИ
    await scene('b-finish', async () => {
      const p = B.page;
      st.show(B);
      await st.sayWhile('Второй диспетчер доводит свою карточку до конца.', async () => {
        await answerBrigade(st, B, 40);
        await status(B, b.svc, 'Прибытие');
      });
      await answerBrigade(st, B, 40);
      await status(B, b.svc, 'Проведение работ');
      await stage(st, B, b.svc, 'Работы завершены', 'Работы завершены — и у него свой разбор.');
      await expect(p.getByTestId('dds-score')).toBeVisible({ timeout: 60_000 });
      await B.moveTo(p.getByTestId('dds-score'));
      await B.ring(p.getByTestId('dds-score'));
      await st.say('Свой балл, свои замечания — ИИ оценивает каждого по его карточке.');
      await B.ring(null);
      const next = p.getByTestId('dds-advice-next').first();
      if (await visible(next)) {
        await B.moveTo(p.getByTestId('my-dds-advice'));
        await B.ring(p.getByTestId('my-dds-advice'));
        await st.say('Под рекомендацией — кнопка «Следующее задание от ИИ».');
        await B.ring(null);
        await st.note(B, 'ИИ готовит задание…', async () => {
          await st.sayWhile('ИИ собирает новое происшествие именно под эту рекомендацию. Преподаватель не нужен — обучение идёт само.', async () => {
            const was = p.url();
            await B.click(next);
            await st.fast(2, () => expect.poll(() => p.url(), { timeout: 180_000 }).not.toBe(was));
            await expect(p.getByTestId('dds-advice-focus')).toBeVisible({ timeout: 60_000 }).catch(() => {});
          });
        });
        await B.ring(p.getByTestId('dds-advice-focus'));
        await st.say('Новая карточка помнит, какую рекомендацию отрабатывает.');
        await B.ring(null);
      }
    });

    // Преподаватель: результат, журнал звонков, аналитика
    await scene('teacher-review', async () => {
      const p = T.page;
      st.show(T);
      await st.sayWhile('Преподаватель видит результат каждого в журнале.', async () => {
        await p.goto('/#/journal');
        if (a.number) {
          const row = p.getByTestId(`journal-row-${a.number}`);
          if (await visible(row)) {
            await T.click(row);
            await expect(p.getByTestId('brigade-panel').or(p.getByTestId('dds-score')).first())
              .toBeVisible({ timeout: 30_000 }).catch(() => {});
          }
        }
      });
      const journal = p.getByTestId('phone-journal');
      if (await visible(journal)) {
        await T.moveTo(journal);
        await T.ring(journal);
        await st.say('Здесь же журнал звонков диспетчера: кому звонил, что говорил.');
        await T.ring(null);
      }
      await p.goto('/#/journal');
      await T.click(p.getByTestId('analytics-link'));
      await st.sayWhile('В аналитике — вся группа: ошибки, время, и исполнение рекомендаций диспетчерами.', async () => {
        await T.click(p.getByTestId('analytics-people-tab'));
        const tab = p.getByTestId('people-analytics').getByRole('button', { name: 'Диспетчеры ДДС' });
        if (await visible(tab)) await T.click(tab);
        const block = p.getByTestId('advice-block-dds');
        if (await visible(block)) { await T.moveTo(block); await T.ring(block); }
      });
      await st.pause(1500);
      await T.ring(null);
    });

    st.card('Тренажёр диспетчера ДДС',
      'Задания от ИИ — разные у каждого · оценка и рекомендации — без преподавателя · преподаватель видит всё');
    await st.say('Разные задания у каждого, оценка и рекомендации без преподавателя, а преподаватель видит всё. Спасибо за внимание.');
    await st.pause(800);
  } finally {
    await st.stop();
  }
  if (errors.length) console.error(`упали сцены: ${errors.join('; ')}`);
});
