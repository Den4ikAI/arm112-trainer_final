import { useCallback } from 'react';
import { IncidentPanel } from '@/widgets/incident-panel/IncidentPanel';
import { SurveyCardView } from '@/widgets/survey-card/SurveyCardView';

type Answers = Record<string, string[]>;

interface Props {
  typeIds: string[];
  answers: Answers;
  onTypes: (ids: string[]) => void;
  /** Как у setState: опросная карта может снять несколько ответов подряд */
  setAnswers: (fn: (prev: Answers) => Answers) => void;
}

/**
 * Тип происшествия и опросная карта в конструкторе учебного материала —
 * те же виджеты, что у оператора 112. Преподаватель собирает эталон так
 * же, как курсант заполняет карточку: по признакам классификатора, а
 * службы и итоговый тип потом считает сервер.
 */
export const TypeSurvey = ({ typeIds, answers, onTypes, setAnswers }: Props) => {
  const onAnswer = useCallback(
    (q: string, v: string[] | string) =>
      setAnswers((prev) => ({ ...prev, [q]: Array.isArray(v) ? v : v ? [v] : [] })),
    [setAnswers],
  );
  return (
    <div data-testid="builder-type">
      <IncidentPanel
        selected={typeIds}
        onToggle={(id) => onTypes(typeIds.includes(id)
          ? typeIds.filter((t) => t !== id)
          : [...typeIds, id])}
      />
      {typeIds.map((t) => (
        <SurveyCardView
          key={t}
          typeId={t}
          answers={answers}
          onAnswer={onAnswer}
          onClose={() => onTypes(typeIds.filter((x) => x !== t))}
        />
      ))}
    </div>
  );
};
