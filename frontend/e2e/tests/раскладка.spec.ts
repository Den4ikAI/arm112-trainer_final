import { expect, test, type Page } from '@playwright/test';
import { fillFireCard, launchTicket3, login } from './helpers';

/**
 * Карточка 112 рядом с панелью обучающегося на типовых экранах класса.
 *
 * UX-проход 1, находка 7: на 1280×720 панель стояла под карточкой на
 * всю ширину и забирала ~280 px из 720 — от карточки оставалось ~300 px,
 * опросная карта уходила под панель. Панель должна стоять сбоку и не
 * заходить на колонки карточки, а колонкам — хватать высоты.
 */
const панель = (page: Page) => page.getByRole('complementary', { name: 'Панель обучающегося' });
const карточка = (page: Page) => page.getByLabel('ФИО заявителя').locator('xpath=ancestor::div[2]');

for (const [w, h] of [[1280, 720], [1920, 1080]] as const) {
  test(`${w}×${h}: панель обучающегося сбоку и не закрывает карточку`, async ({ page }, info) => {
    await page.setViewportSize({ width: w, height: h });
    await login(page, info);
    await launchTicket3(page);
    await fillFireCard(page);

    const hud = await панель(page).boundingBox();
    const card = await карточка(page).boundingBox();
    expect(hud && card).toBeTruthy();
    if (!hud || !card) return;

    // Панель справа от карточки, а не под ней
    expect(hud.x).toBeGreaterThanOrEqual(card.x + card.width - 1);
    // Колонки карточки занимают почти всю высоту между шапкой и подвалом
    expect(card.height).toBeGreaterThan(h * 0.55);
    // Опросная карта после выбора типа — на экране, а не под панелью
    await expect(page.getByRole('button', { name: 'Крыша', exact: true })).toBeInViewport();

    await page.screenshot({ path: info.outputPath(`карточка-${w}x${h}.png`) });
  });
}

/** UX-проход 1, находка 9: в шапке было «Опер. , АРМ 4, Иванов И.И.». */
test('шапка карточки: оператор и АРМ без пустого места перед запятой', async ({ page }, info) => {
  await login(page, info);
  await launchTicket3(page);
  const строка = page.getByTestId('card-operator');
  await expect(строка).toHaveText(/^Опер\. \S/);
  // Строкой, а не toContainText: тот обрезает пробелы в ожидаемом
  await expect(строка).not.toHaveText(/\s,|,\s*,|,\s*$/);
  await expect(строка).toHaveText(/, АРМ \d+$/);
});
