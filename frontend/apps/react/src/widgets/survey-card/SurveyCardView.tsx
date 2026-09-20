import { useEffect } from 'react';
import { useCatalog } from '@/entities/catalog/model';
import type { SurveyQuestion } from '@/shared/api';
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
export const SurveyCardView = ({ typeId, answers, onAnswer, onClose, readOnly }: Props) => {
  const card = useCatalog((st) => st.cards[typeId]);
  const loadCard = useCatalog((st) => st.loadCard);

  // Карт 1283, поэтому тянем ту, которую курсант действительно открыл
  useEffect(() => { void loadCard(typeId); }, [typeId, loadCard]);

  if (!card) return null;

  return (
    <section className={s.wrap} aria-label={card.title}>
      <header className={s.head}>
        <h3 className={s.title}>{card.title}</h3>
        {!readOnly && (
          <button className={s.close} onClick={onClose} aria-label="Убрать опросную карту">×</button>
        )}
      </header>

      <div className={s.body}>
        {card.questions.map((q) => (
          <Question key={q.id} q={q} value={answers[q.id]} readOnly={readOnly}
                    onAnswer={(v) => onAnswer(q.id, v)} />
        ))}
      </div>
    </section>
  );
};

const Question = ({ q, value, onAnswer, readOnly }: {
  q: SurveyQuestion;
  value: string[] | string | undefined;
  onAnswer: (v: string[] | string) => void;
  readOnly?: boolean;
}) => {
  const selected = Array.isArray(value) ? value : value ? [String(value)] : [];

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
