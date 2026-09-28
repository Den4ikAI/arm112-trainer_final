import { expect, type Locator } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import type { Demo } from './kit';

/**
 * Шаги демо-ролика. Каждый — самостоятельный кусок сюжета: их можно
 * переставлять, выкидывать и добавлять новые (см. `voiceQuestion112` —
 * заготовка под свободный голосовой вопрос заявителю).
 */

export interface Ctx {
  /** Номер карточки, заведённой оператором 112 */
  number?: string;
  /** Профиль модели: на процессоре подготовку вызовов не показываем */
  profile: 'gpu' | 'cpu';
  db: string;
  /** Голос заявителя вызова 112 — как его выбирает приложение */
  applicantVoice?: string;
}

const PASSWORD = 'arm112';

const sql = (ctx: Ctx, query: string): string =>
  execFileSync('docker', ['exec', 'arm112-db', 'psql', '-U', 'arm112', '-d', ctx.db, '-qAtc', query])
    .toString().trim();

const visible = async (l: Locator): Promise<boolean> => (await l.count()) > 0 && l.first().isVisible();

/** Вход под учётной записью, с набором логина на экране. */
export const login = async (d: Demo, user: string): Promise<void> => {
  const p = d.page;
  await p.goto('/#/login');
  await p.evaluate(() => localStorage.clear());
  await p.reload();
  await d.type(p.locator('input[name="login"]'), user, 60);
  await d.type(p.locator('input[name="password"]'), PASSWORD, 40);
  await d.click(p.getByTestId('login-submit'));
  await p.getByRole('heading', { name: 'Учебные вызовы' }).waitFor({ timeout: 60_000 });
  await d.pause(500);
};

export const intro = async (d: Demo): Promise<void> => {
  await d.page.goto('/#/login');
  await d.card('Тренажёр <span>АРМ-112</span>',
    'Оператор 112 и диспетчер дежурно-диспетчерской службы — от звонка до разбора');
  await d.say('Тренажёр оператора [112|сто двенадцать] и диспетчера дежурно-диспетчерской службы.');
  await d.say('Покажем весь путь вызова: преподаватель готовит вызов, курсант принимает звонок, диспетчер ведёт реагирование, преподаватель разбирает работу.');
  await d.hideCard();
  await d.pause(400);
};

export const teacherPrepare = async (d: Demo, ctx: Ctx): Promise<void> => {
  const p = d.page;
  await d.caption('Преподаватель');
  await login(d, 'teacher1');
  await d.click(p.getByTestId('scenarios-link'));
  await expect(p.getByTestId('legend')).toBeVisible();
  await d.say('Преподаватель готовит учебные вызовы. Языковая модель пишет их по билетам заказчика, а эталон берётся из классификатора.');

  if (ctx.profile === 'gpu') {
    await d.sayWhile('Модель готовит новые вызовы из билетов.', async () => {
      await d.click(p.getByTestId('generate'));
      await expect(p.getByTestId('generate-note')).toBeVisible({ timeout: 300_000 });
    }, 'подготовка идёт в фоновой очереди');
    // Свежий черновик — в конце списка
    const draft = p.locator('[data-testid^="scenario-"]', { hasText: 'черновик' }).first();
    if (await visible(draft)) await d.click(draft);
  }

  await d.ring(p.getByTestId('legend'));
  await d.say('Вызов — это легенда заявителя и скрытые факты: их курсант должен выяснить вопросами.');
  await d.ring(p.getByTestId('facts'));
  await d.pause(1200);
  await d.ring(null);

  await d.sayWhile('Замечание преподавателя модель отрабатывает сама.', async () => {
    await d.type(p.getByTestId('chat-input'),
      'Пусть заявитель говорит взволнованно и коротко, как на настоящем звонке.', 28);
    await d.click(p.getByTestId('chat-send'));
  });
  await d.waitModel(() =>
    expect(p.getByTestId('msg-model').last()).toBeVisible({ timeout: 600_000 }));
  await d.ring(p.getByTestId('msg-model').last());
  await d.say('Правка применена к вызову, преподаватель видит, что изменилось, и утверждает вызов.');
  await d.ring(null);
  const approve = p.getByTestId('approve');
  if (await visible(approve)) await d.click(approve);
  await d.pause(600);
};

/** Последняя реплика заявителя в стенограмме. */
const lastApplicant = async (d: Demo): Promise<string> => {
  const lines = d.page.getByTestId('transcript').locator('li');
  const n = await lines.count();
  for (let i = n - 1; i >= 0; i--) {
    const who = (await lines.nth(i).locator('span').first().innerText()).toLowerCase();
    if (who.includes('заявитель')) return lines.nth(i).locator('span').nth(1).innerText();
  }
  return '';
};

/** Дождаться нового ответа заявителя и положить его голос в дорожку. */
const applicantAnswers = async (d: Demo, ctx: Ctx, before: number): Promise<void> => {
  const lines = d.page.getByTestId('transcript').locator('li');
  await expect.poll(() => lines.count(), { timeout: 60_000 }).toBeGreaterThanOrEqual(before + 2);
  await d.app(await lastApplicant(d), ctx.applicantVoice ?? 'applicant_f');
};

export const operator112 = async (d: Demo, ctx: Ctx): Promise<void> => {
  const p = d.page;
  await d.caption('Оператор 112');
  await login(d, 'student1');
  await d.say('Курсант в роли оператора [112|сто двенадцать] берёт вызов из журнала.');
  await d.click(p.getByTestId('launch-t3-c1'));
  await p.getByTestId('card-timer').waitFor();
  ctx.number = (await p.getByTestId('card-number').innerText()).trim();
  // Пол заявителя — по отчеству в шапке панели, как у самого приложения
  const hudHead = (await p.getByRole('complementary', { name: 'Панель обучающегося' }).innerText())
    .split('\n').slice(0, 3).join(' ');
  ctx.applicantVoice = /(вна|чна)(?![а-яё])|кызы/i.test(hudHead) ? 'applicant_f' : 'applicant_m';
  // Заявитель начинает разговор — приложение озвучивает легенду
  await d.app(await lastApplicant(d), ctx.applicantVoice);
  await d.ring(p.getByTestId('transcript'));
  await d.say('Карточка — копия боевой системы. Идёт таймер, заявитель говорит первую фразу.');
  await d.ring(null);

  const lines = p.getByTestId('transcript').locator('li');
  await d.say('Оператор уточняет обстоятельства — из списка вопросов…');
  let before = await lines.count();
  await d.click(p.getByTestId('ask-city'));
  await applicantAnswers(d, ctx, before);

  await voiceQuestion112(d, ctx);

  await d.ring(p.getByTestId('transcript'));
  await d.say('Выясненное вопросом засчитывается, угаданное без вопроса — нет.');
  await d.ring(null);

  await d.sayWhile('Адрес и данные заявителя.', async () => {
    await d.type(p.getByPlaceholder('Фамилия и имя заявителя'), 'Иванова Инна', 40);
    await d.type(p.getByRole('textbox', { name: 'Населенный пункт:' }), 'Королёв', 40);
    await d.type(p.getByRole('textbox', { name: 'Улица:' }), 'Станционная', 40);
    await d.type(p.getByRole('textbox', { name: 'Дом/Вл:' }), '28', 60);
  });

  await d.sayWhile('Тип происшествия и опросная карта. Карта растёт по мере ответов.', async () => {
    await d.type(p.locator('#type-search'), '101', 90);
    await d.click(p.getByRole('option', { name: '101', exact: true }));
    const pick = async (group: string, option: string) => {
      await d.click(p.getByRole('group', { name: group })
        .getByRole('button', { name: option, exact: true }));
    };
    await pick('Где', 'Дом');
    await pick('Признак пожара (дом)', 'Открытое пламя / Дым');
    await pick('Дом (пламя, дым)', 'Дом частный');
    await pick('Внутридомовые объекты (пламя, дым)', 'Крыша');
    await pick('Угроза людям', 'Нет');
    await pick('Проведена ли газификация', 'Нет');
  });

  const bar = p.getByRole('contentinfo', { name: 'Список оповещения' });
  if (await visible(bar)) await d.ring(bar);
  await d.say('Службы подтянулись сами: по классификатору заказчика — из [1283|тысячи двухсот восьмидесяти трёх] позиций.');
  await d.ring(null);

  await d.sayWhile('Описание со слов заявителя курсант пишет своими словами. Здесь он ошибся: заявитель сказал, что пострадавших нет.', async () => {
    await d.type(p.getByRole('textbox', { name: 'Описание со слов заявителя' }),
      'Горит крыша частного дома, открытое пламя. Есть пострадавший. Газа в доме нет.', 30);
  });

  await d.click(p.getByTestId('save-card'));
  await expect(p.getByTestId('score')).toBeVisible();
  await d.ring(p.getByTestId('score'));
  await d.say('Разбор приходит сразу: балл, что выяснено вопросом, что угадано, какие службы нужны были.');
  await d.ring(null);
  const guessed = p.getByTestId('finding-guessed').first();
  if (await visible(guessed)) {
    await d.moveTo(guessed);
    await d.ring(guessed);
    await d.say('Открытое пламя курсант отметил верно, но не спросил о нём, — значит, угадал.');
    await d.ring(null);
  }

  await d.sayWhile('Следом нейросеть сверяет описание курсанта с разговором.', async () => {
    await p.getByTestId('attempt-review').scrollIntoViewIfNeeded();
    await d.waitModel(() => expect(p.getByTestId('analysis-status'))
      .not.toContainText('готовится', { timeout: 600_000 }));
  });
  const finding = p.getByTestId('analysis-finding').first();
  if (await visible(finding)) {
    await d.moveTo(finding);
    await d.ring(p.getByTestId('attempt-review'));
    await d.say('Модель нашла противоречие и привела цитаты: что написал курсант и что на самом деле сказал заявитель. Каждую цитату проверяет код.');
    await d.ring(null);
  }
};

/**
 * Свободный вопрос заявителю — голосом и текстом. Включается, когда в
 * панели обучающегося есть режим «Своими словами».
 */
export const voiceQuestion112 = async (d: Demo, ctx: Ctx): Promise<void> => {
  const p = d.page;
  const free = p.getByTestId('hud-mode-free');
  if (!(await visible(free))) return;
  const lines = p.getByTestId('transcript').locator('li');

  await d.say('…или своими словами, голосом.');
  await d.click(free);
  const talk = p.getByTestId('hud-talk');
  if (await visible(talk)) {
    const sec = await d.mic('Скажите, а пострадавшие есть?', 'operator');
    const before = await lines.count();
    await d.click(talk);
    d.micStarted();
    await d.pause(sec * 1000 + 500);
    await d.click(talk);
    await applicantAnswers(d, ctx, before);
    await d.say('Речь распознана на сервере, модель поняла, о каком факте вопрос, — заявитель ответил тем, что записано в вызове.');
  }

  await d.say('Можно и напечатать.');
  const before = await lines.count();
  await d.type(p.getByTestId('hud-free-input'), 'Газ в дом проведён?', 55);
  await d.click(p.getByTestId('hud-free-send'));
  await applicantAnswers(d, ctx, before);
};

const status = async (d: Demo, value: string, comment?: string): Promise<void> => {
  const p = d.page;
  await d.click(p.getByTestId('pencil-mchs'));
  await d.select(p.getByRole('combobox', { name: 'Статус' }), value);
  if (comment) await d.type(p.getByRole('textbox', { name: 'Комментарий' }), comment, 25);
  await d.click(p.getByRole('button', { name: 'Сохранить статус' }));
  await expect(p.getByTestId('service-tab-mchs')).toContainText(value);
};

/** Этап бригады наступил только что: ролик не ждёт реальных минут. */
const stageNow = (ctx: Ctx, stage: string): void => {
  sql(ctx, `UPDATE brigade_reports r SET due_at = now() - interval '30 seconds'
              FROM brigade_assignments a JOIN cards c ON c.id = a.card_id
             WHERE r.assignment_id = a.id AND c.number = '${ctx.number}'
               AND r.stage = '${stage}'`);
};

export const dispatcherDds = async (d: Demo, ctx: Ctx): Promise<void> => {
  const p = d.page;
  await d.caption('Диспетчер ДДС');
  // В жизни оператор 112 и диспетчер работают одновременно: карточка
  // приходит, пока диспетчер у экрана. В ролике между ними разбор 112
  // и титры, поэтому момент поступления сдвигаем на начало сцены —
  // иначе норматив «30 с до открытия» истёк бы, пока шёл рассказ
  sql(ctx, `UPDATE cards SET saved_at = now() WHERE number = '${ctx.number}'`);
  await login(d, 'student3');
  const row = p.getByTestId(`journal-row-${ctx.number}`);
  await expect(row).toBeVisible();
  await d.ring(row);
  await d.say('Карточка ушла в службы. У диспетчера пожарной охраны она в ленте, пошёл отсчёт: тридцать секунд до открытия.');
  await d.ring(null);
  await d.click(row);
  await d.say('Открыл — система отметила: получена службой. Три минуты на первую запись: статус и текст.');

  await d.sayWhile('Диспетчер принимает карточку.', () =>
    status(d, 'Принята', 'Принято, направляю бригаду'));

  await d.sayWhile('И направляет бригаду.', async () => {
    await d.select(p.getByTestId('brigade-select'), '2');
    await d.click(p.getByTestId('brigade-assign'));
    await expect(p.getByTestId('brigade-panel')).toContainText('№ 2');
  });

  stageNow(ctx, 'Начало реагирования');
  await expect(p.getByTestId('brigade-incoming')).toContainText('выехали', { timeout: 60_000 });
  await d.ring(p.getByTestId('brigade-incoming'));
  await d.app((await p.getByTestId('brigade-incoming').innerText()).replace(/^.*?Старший/s, 'Старший'), 'brigade');
  await d.say('Старший бригады докладывает сам. Статус ставится по факту доклада.', 'этапы работ бригады в ролике ускорены');
  await d.ring(null);
  await status(d, 'Начало реагирования');

  const talk = p.getByTestId('brigade-talk');
  if (await visible(talk)) {
    stageNow(ctx, 'Прибытие');
    await d.say('Диспетчер может и сам позвонить старшему — голосом.');
    const sec = await d.mic('Старший, доложите обстановку.', 'dispatcher');
    await d.click(talk);
    d.micStarted();
    await d.pause(sec * 1000 + 500);
    await d.click(talk);
    await expect(p.getByTestId('brigade-answer')).toBeVisible({ timeout: 60_000 });
    await d.ring(p.getByTestId('brigade-panel'));
    await d.app(await p.getByTestId('brigade-answer').innerText(), 'brigade');
    await d.say('Речь распознана на процессоре, ответ старшего синтезирован. Без сервера голоса всё то же работает текстом.');
    await d.ring(null);
    await status(d, 'Прибытие');
  }

  await chiefReport(d);
};

/** Ответ начальнику на уточняющий вопрос — по ключевым словам вопроса. */
const answerChief = (q: string): string => {
  const t = q.toLowerCase();
  if (/адрес|где/.test(t)) return 'Королёв, Станционная улица, дом двадцать восемь.';
  if (/пострадав/.test(t)) return 'Пострадавших нет, все вышли.';
  if (/служб|оповест|кого ещё/.test(t)) return 'Карточка 112 ушла также в полицию, ЦОДД и Мосбез.';
  if (/бригад|этап|направ/.test(t)) return 'Вторая бригада прибыла на место.';
  if (/случил|что/.test(t)) return 'Пожар в частном доме, горит крыша.';
  return 'Докладываю по карточке: пожар в частном доме, бригада на месте.';
};

export const chiefReport = async (d: Demo): Promise<void> => {
  const p = d.page;
  const panel = p.getByTestId('chief-report');
  if (!(await visible(panel))) return;
  await d.moveTo(panel);
  await d.say('По указанию заказчика диспетчер докладывает вышестоящему начальнику.');
  await d.click(p.getByTestId('chief-start'));
  const lines = p.getByTestId('chief-lines').locator('li');
  await expect(lines.first()).toBeVisible();
  await d.app(await lines.last().innerText().then((t) => t.replace(/^Начальник:\s*/, '')), 'chief');

  const mic = panel.getByRole('button', { name: /Голосом/ });
  const first = 'Товарищ начальник, в Королёве пожар в частном доме, горит крыша. Направлена вторая бригада.';
  if (await visible(mic)) {
    const sec = await d.mic(first, 'dispatcher');
    await d.click(mic);
    d.micStarted();
    await d.pause(sec * 1000 + 500);
    await d.click(panel.getByRole('button', { name: /Закончить/ }));
  } else {
    await d.type(p.locator('textarea[name="chief-say"]'), first, 22);
    await d.click(p.getByTestId('chief-send'));
  }

  for (let i = 0; i < 5; i++) {
    const before = await lines.count();
    await expect.poll(async () => (await lines.last().innerText()).startsWith('Начальник'), { timeout: 120_000 })
      .toBe(true);
    await expect(lines).not.toHaveCount(before - 1);
    const last = (await lines.last().innerText()).replace(/^Начальник:\s*/, '');
    if (/слушает/.test(last)) { await d.pause(500); i--; continue; }
    await d.app(last, 'chief');
    if (/Принял/i.test(last) || !(await visible(p.locator('textarea[name="chief-say"]')))) break;
    await d.type(p.locator('textarea[name="chief-say"]'), answerChief(last), 22);
    await d.click(p.getByTestId('chief-send'));
  }

  await d.sayWhile('Модель разбирает доклад: что доложено, что упущено, что доложено неверно.', async () => {
    await d.waitModel(() =>
      expect(panel.getByTestId('chief-review')).toBeVisible({ timeout: 600_000 }));
  });
  await d.moveTo(panel.getByTestId('chief-review'));
  await d.ring(panel.getByTestId('chief-review'));
  await d.pause(3000);
  await d.ring(null);
};

export const teacherReview = async (d: Demo, ctx: Ctx): Promise<void> => {
  const p = d.page;
  await d.caption('Преподаватель');
  await login(d, 'teacher1');
  await d.click(p.getByTestId(`journal-row-${ctx.number}`));
  const attempt = p.getByTestId('attempt-card112');
  await expect(attempt).toBeVisible();
  await d.moveTo(attempt);
  await d.say('Преподаватель — конечный валидатор. Рядом с автоматической оценкой он ставит свою и подтверждает или отклоняет замечания нейросети.');
  await d.type(attempt.locator('input[name="expert-score"]'), '70', 120);
  await d.type(attempt.locator('textarea[name="expert-comment"]'),
    'Верно записал адрес, но в описании пострадавший, которого не было.', 22);
  await d.click(attempt.getByRole('button', { name: 'Поставить оценку' }));
  await expect(attempt.getByTestId('expert-score')).toHaveText('70');
  await d.pause(800);

  // Разделы преподавателя — в шапке журнала
  await p.goto('/#/journal');
  await d.click(p.getByTestId('analytics-link'));
  await d.pause(2500);
  await d.say('Аналитика по группе: ошибки, время, прогноз результата, который фиксируется до вызова и сверяется с фактом.');
  const forecast = p.getByTestId('forecast-block');
  if (await visible(forecast)) {
    await d.moveTo(forecast);
    await d.pause(1500);
  }

  await p.goto('/#/journal');
  await d.click(p.getByTestId('lessons-link'));
  await d.pause(800);
  await d.sayWhile('Занятия: обычное, марафон — экзамен из пяти–десяти вызовов с протоколом, и блиц по одному участку карточки.', async () => {
    const kind = p.getByTestId('lesson-kind');
    if (await visible(kind)) {
      await d.select(kind, 'marathon');
      await d.pause(1200);
      await d.select(kind, 'blitz');
      await d.pause(1000);
    }
  });
};

export const outro = async (d: Demo, ctx: Ctx): Promise<void> => {
  await d.card('Тренажёр <span>АРМ-112</span>',
    ctx.profile === 'gpu'
      ? 'Всё в закрытом контуре: модель, распознавание и синтез речи — на своих серверах'
      : 'Без видеокарты: модель, распознавание и синтез речи — на процессоре');
  await d.say(ctx.profile === 'gpu'
    ? 'Всё работает в закрытом контуре: модель, распознавание и синтез речи на своих серверах.'
    : 'Всё то же работает и без видеокарты, на процессоре.');
  await d.pause(1500);
};
