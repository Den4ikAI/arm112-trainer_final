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
}) => (
  <div className={s.body}>
    {card.questions.map((q) => {
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
