import {
  emptyCard, classify, grade, getScenario, nextStatuses,
} from './src/index';

let failed = 0;
const check = (name: string, cond: boolean, extra = '') => {
  console.log(`${cond ? '  ok  ' : ' FAIL '} ${name}${extra ? ' — ' + extra : ''}`);
  if (!cond) failed++;
};

// --- классификатор ---
const fire = emptyCard('1');
fire.typeIds = ['101'];
fire.answers = { where: ['Дом'], 'house-kind': ['Дом многоквартирный'], 'people-threat': ['Да'] };
const r1 = classify(fire);
check('пожар в МКД → МЧС', r1.services.includes('s101'));
check('угроза людям → медицина', r1.services.includes('s103') && r1.services.includes('cemp'));
check('итоговый тип определён', r1.outcome === 'пожар: квартира', r1.outcome);

// Проверок на БПЛА и ДТП здесь нет намеренно: опросные карты для них
// со скриншотов не снимались, а сочинённые убраны. Настоящая позиция
// 24120200 «БПЛА» проверяется на бэкенде против импортированного
// классификатора — backend/tests/classifier_import.rs.

// --- машина статусов ---
check('первичные только Принята/Не принята',
  JSON.stringify(nextStatuses([])) === JSON.stringify(['Принята', 'Не принята']));
check('после Не принята — только Принята',
  JSON.stringify(nextStatuses([
    { status: 'Не принята', at: '', operator: '' },
  ])) === JSON.stringify(['Принята']));
check('после Работы завершены — ничего',
  nextStatuses([{ status: 'Работы завершены', at: '', operator: '' }]).length === 0);
check('после Принята открыт ход работ',
  nextStatuses([{ status: 'Принята', at: '', operator: '' }]).includes('Прибытие'));

// --- оценка: узнал против угадал ---
const sc = getScenario('t3-c1')!;
const guessCard = emptyCard('3');
guessCard.typeIds = ['101'];
guessCard.address.settlement = 'Королёв';
const gGuess = grade(guessCard, sc, 30, []);          // ничего не спрашивал
const gAsked = grade(guessCard, sc, 30, ['city']);    // спросил про город

const guessedFields = gGuess.findings.filter((f) => f.kind === 'guessed').map((f) => f.field);
check('без вопроса поле помечено «угадал»', guessedFields.includes('address.settlement'));
const askedGuessed = gAsked.findings
  .filter((f) => f.kind === 'guessed').map((f) => f.field);
check('после вопроса пометки нет', !askedGuessed.includes('address.settlement'));
check('спросивший получает балл выше', gAsked.score > gGuess.score,
  `${gGuess.score} → ${gAsked.score}`);
check('превышение норматива фиксируется', grade(guessCard, sc, 120, []).overNorm);
check('пропущенная служба даёт последствие',
  gGuess.findings.some((f) => f.kind === 'missing' && f.field.startsWith('service.')
    && (f.consequence ?? '').includes('не выехала')));

console.log(failed === 0 ? '\nВсе проверки домена пройдены' : `\nПровалено: ${failed}`);
process.exit(failed === 0 ? 0 : 1);
