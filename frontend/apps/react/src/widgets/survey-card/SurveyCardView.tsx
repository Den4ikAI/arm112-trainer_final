import { useEffect } from 'react';
import { useCatalog } from '@/entities/catalog/model';
import type { SurveyCard, SurveyQuestion } from '@/shared/api';
import { Chip } from '@/shared/ui/Chip';
import s from './SurveyCardView.module.scss';

interface Props {
  typeId: string;
  answers: Record<string, string[] | string>;
  onAnswer: (questionId: string, value: string[] | string) => void;
  onClose: () => void;
  readOnly?: boolean;
}

/**
 * Опросная карта под выбранным типом происшествия.
 * Каждая комбинация признаков соответствует итоговому типу, а итоговый тип —
 * набору служб. Поэтому любое нажатие плашки пересчитывает список оповещения.
 */
/** Выбранные значения ответа, как бы он ни был записан. */
const выбрано = (v: string[] | string | undefined): string[] =>
  Array.isArray(v) ? v : v ? [String(v)] : [];

/**
 * Какие варианты вопроса сейчас показывать.
 *
 * У карт, выведенных из классификатора, второй признак зависит от
 * первого: у ДТП одни варианты, у «ДТП пострадавшие» — другие.
 * Вариант без родителей показывается всегда.
 */
const видимые = (q: SurveyQuestion, answers: Props['answers']): string[] => {
  if (!q.dependsOn) return q.options;
  const родитель = выбрано(answers[q.dependsOn]);
  return q.options.filter((opt) => {
    const нужны = q.optionParents[opt];
    return !нужны || нужны.some((p) => родитель.includes(p));
  });
};

/**
 * Какие вопросы карты сейчас показывать.
 *
 * Карта растёт по мере ответов, как на кадрах боевой системы: после
 * «Улица» появляется признак пожара на улице, после объекта —
 * «Правонарушение». Вопрос виден, если выполнено хоть одно его условие,
 * а условие выполнено, только если виден и сам вопрос, на который оно
 * ссылается: ответ на скрытый вопрос карту не раскрывает.
 */
const видимыеВопросы = (card: SurveyCard, answers: Props['answers']): Set<string> => {
  const поId = new Map(card.questions.map((q) => [q.id, q]));
  const итог = new Map<string, boolean>();
  const виден = (id: string, путь: Set<string>): boolean => {
    const готово = итог.get(id);
    if (готово !== undefined) return готово;
    const q = поId.get(id);
    // Ссылка на вопрос, которого нет в карте, или цикл в условиях —
    // ошибка данных; прятать из-за неё вопрос нельзя, но и раскрывать
    // по ней другие тоже
    if (!q || путь.has(id)) return false;
    путь.add(id);
    const ответ = q.showWhen.length === 0 || q.showWhen.some((w) =>
      виден(w.question, путь)
      && выбрано(answers[w.question]).some((v) => w.values.includes(v)));
    путь.delete(id);
    итог.set(id, ответ);
    return ответ;
  };
  return new Set(card.questions.filter((q) => виден(q.id, new Set())).map((q) => q.id));
};

export const SurveyCardView = ({ typeId, answers, onAnswer, onClose, readOnly }: Props) => {
  const card = useCatalog((st) => st.cards[typeId]);
  const loadCard = useCatalog((st) => st.loadCard);

  // Карт 1283, поэтому тянем ту, которую курсант действительно открыл
  useEffect(() => { void loadCard(typeId); }, [typeId, loadCard]);

  // Поменяли первый признак — второй, выбранный под прежний, уже
  // не подходит. Снимаем его, иначе классификатор получит
  // комбинацию, которой в нём нет, и курсант не поймёт почему
  useEffect(() => {
    if (!card || readOnly) return;
    for (const q of card.questions) {
      if (!q.dependsOn) continue;
      const было = выбрано(answers[q.id]);
      if (было.length === 0) continue;
      const можно = видимые(q, answers);
      const осталось = было.filter((v) => можно.includes(v));
      if (осталось.length !== было.length) onAnswer(q.id, осталось);
    }
  }, [card, answers, onAnswer, readOnly]);

  // Вопрос скрылся — его ответ тоже снимаем. Иначе «Мусор», выбранный
  // до переключения на «Транспорт», продолжал бы подбирать службы
  // уличного пожара, хотя на экране его уже нет
  useEffect(() => {
    if (!card || readOnly) return;
    const видны = видимыеВопросы(card, answers);
    for (const q of card.questions) {
      if (видны.has(q.id)) continue;
      const было = answers[q.id];
      if (Array.isArray(было) ? было.length > 0 : Boolean(было)) {
        onAnswer(q.id, Array.isArray(было) ? [] : '');
      }
    }
  }, [card, answers, onAnswer, readOnly]);

  // Ещё грузится
  if (card === undefined) return null;

  return (
    <section className={s.wrap} aria-label={card?.title ?? typeId}
             data-testid={`survey-${typeId}`}>
      <header className={s.head}>
        <h3 className={s.title}>{card?.title ?? typeId}</h3>
        {!readOnly && (
          <button className={s.close} onClick={onClose} aria-label="Убрать опросную карту">×</button>
        )}
      </header>

      {/* Карты нет — говорим об этом, а не рисуем пустоту: иначе
          курсант решает, что плашка не нажалась */}
      {card === null
        ? <p className={s.none}>Для этого типа опросной карты нет.</p>
        : <Body card={card} answers={answers} onAnswer={onAnswer} readOnly={readOnly} />}
    </section>
  );
};

const Body = ({ card, answers, onAnswer, readOnly }: {
  card: SurveyCard;
  answers: Props['answers'];
  onAnswer: Props['onAnswer'];
  readOnly?: boolean;
}) => {
  const видны = видимыеВопросы(card, answers);
  return (
    <div className={s.body}>
      {card.questions.map((q) => {
        if (!видны.has(q.id)) return null;
        const варианты = видимые(q, answers);
        // Зависимый вопрос без подходящих вариантов не показываем:
        // пока не выбран первый признак, второму не из чего выбирать
        if (q.dependsOn && варианты.length === 0) return null;
        return (
          <Question key={q.id} q={{ ...q, options: варианты }} value={answers[q.id]}
                    readOnly={readOnly} onAnswer={(v) => onAnswer(q.id, v)} />
        );
      })}
    </div>
  );
};

const Question = ({ q, value, onAnswer, readOnly }: {
  q: SurveyQuestion;
  value: string[] | string | undefined;
  onAnswer: (v: string[] | string) => void;
  readOnly?: boolean;
}) => {
  const selected = выбрано(value);

  const toggle = (opt: string) => {
    if (readOnly) return;
    if (q.kind === 'single') {
      onAnswer(selected.includes(opt) ? [] : [opt]);
    } else {
      onAnswer(selected.includes(opt)
        ? selected.filter((v) => v !== opt)
        : [...selected, opt]);
    }
  };

  return (
    <div className={s.row}>
      <div className={s.label} id={`q-${q.id}`}>{q.label}</div>
      <div className={s.control} role="group" aria-labelledby={`q-${q.id}`}>
        {q.options.length > 0
          ? q.options.map((opt) => (
              <Chip key={opt} selected={selected.includes(opt)} disabled={readOnly}
                    onClick={() => toggle(opt)}>
                {opt}
              </Chip>
            ))
          : (
            <input
              className={s.free}
              type={q.kind === 'number' ? 'number' : 'text'}
              value={typeof value === 'string' ? value : ''}
              readOnly={readOnly}
              aria-label={q.label}
              onChange={(e) => onAnswer(e.target.value)}
            />
          )}
      </div>
    </div>
  );
};

/** Вопрос «да / нет»: в сводке его ответ пишется вместе с вопросом. */
const даНет = (q: SurveyQuestion) =>
  q.options.length > 0 && q.options.every((o) => ['Да', 'Нет', 'Нет данных'].includes(o));

/**
 * Опросная карта, свёрнутая в одну строку, — как в карточке ДДС боевой
 * системы: под заголовком типа выбранные признаки через точку («Дом .
 * Открытое пламя / Дым . Дом частный . …»).
 *
 * Диспетчер ДДС карту не заполняет и вариантов ответа не видит: ему
 * нужно, что выбрал оператор 112, а не из чего он выбирал.
 */
export const SurveySummary = ({ typeId, answers }: {
  typeId: string;
  answers: Props['answers'];
}) => {
  const card = useCatalog((st) => st.cards[typeId]);
  const loadCard = useCatalog((st) => st.loadCard);
  useEffect(() => { void loadCard(typeId); }, [typeId, loadCard]);
  if (card === undefined) return null;

  const части: string[] = [];
  if (card) {
    const видны = видимыеВопросы(card, answers);
    for (const q of card.questions) {
      if (!видны.has(q.id)) continue;
      const значения = выбрано(answers[q.id]).map((v) => v.trim()).filter(Boolean);
      if (значения.length === 0) continue;
      // «Угроза людям: нет» понятнее голого «Нет»
      части.push(даНет(q) ? `${q.label}: ${значения.join(', ').toLowerCase()}` : значения.join(', '));
    }
  }

  return (
    <section className={s.summary} aria-label={card?.title ?? typeId}
             data-testid={`survey-summary-${typeId}`}>
      <h3 className={s.summaryHead}>{card?.title ?? typeId}</h3>
      <p className={s.summaryLine}>
        {части.length > 0 ? `${части.join(' . ')} .` : 'Признаки не выбраны'}
      </p>
    </section>
  );
};
