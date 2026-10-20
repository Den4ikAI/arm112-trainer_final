import { useState } from 'react';
import { ApiError, ddsIncidents, type Incident } from '@/shared/api';
import { useCatalog } from '@/entities/catalog/model';
import clsx from '@/shared/lib/clsx';
import { SurveySummary } from '@/widgets/survey-card/SurveyCardView';
import s from './MaterialBuilder.module.scss';

interface Props {
  incident: Incident;
  onChanged: (incident: Incident | null) => void;
  onEdit: () => void;
}

/** Как собрано — чтобы было видно, где поработал ИИ. */
const ИСТОЧНИК: Record<string, string> = {
  ai: 'собрано ИИ: признаки и службы — классификатор, текст — модель',
  prompt: 'собрано моделью по описанию преподавателя',
  manual: 'собрано вручную',
};

const ADDRESS_ORDER = ['country', 'region', 'settlement', 'district', 'area', 'street', 'house',
  'building', 'structure', 'flat', 'entrance', 'floor', 'code', 'descriptive'];

/** Учебное происшествие ДДС глазами преподавателя: что получит диспетчер. */
export const IncidentView = ({ incident: inc, onChanged, onEdit }: Props) => {
  const typeTitle = useCatalog((st) => st.typeTitle);
  const serviceTitle = useCatalog((st) => st.serviceTitle);
  const [error, setError] = useState<string | null>(null);

  const act = async (fn: () => Promise<Incident | null>) => {
    setError(null);
    try {
      onChanged(await fn());
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не получилось');
    }
  };

  const address = ADDRESS_ORDER.map((k) => inc.address[k]).filter(Boolean).join(', ');

  return (
    <section className={s.view} data-testid="incident-view">
      <div className={s.actions}>
        <span className={clsx(s.badge, inc.status === 'approved' && s.badgeOk)}>
          {inc.status === 'approved' ? 'утверждено' : 'черновик'}
        </span>
        <strong>№ {inc.number} · {inc.title ?? 'без темы'}</strong>
        {inc.status !== 'approved' && (
          <button className={s.primary} data-testid="incident-approve"
                  onClick={() => void act(() => ddsIncidents.approve(inc.id))}>утвердить</button>
        )}
        {inc.lessons === 0 && (
          <>
            <button className={s.plain} onClick={onEdit} data-testid="incident-edit">править</button>
            <button className={s.danger}
                    onClick={() => void act(async () => { await ddsIncidents.remove(inc.id); return null; })}>
              удалить
            </button>
          </>
        )}
        {inc.lessons > 0 && <span className={s.note}>выдано в занятиях: {inc.lessons} — правка закрыта</span>}
      </div>
      {inc.source && (
        <p className={s.note} data-testid="incident-source">{ИСТОЧНИК[inc.source]}</p>
      )}
      {error && <p className={s.error} role="alert">{error}</p>}
      <dl className={s.dl}>
        <dt>Заявитель</dt><dd>{[inc.applicantName, inc.applicantStatus].filter(Boolean).join(', ') || '—'}</dd>
        <dt>Адрес</dt><dd>{address || '—'}</dd>
        <dt>Описание</dt><dd data-testid="incident-view-description">{inc.description || '—'}</dd>
        <dt>Пострадавшие</dt><dd>{inc.victims ?? 'нет'}</dd>
        <dt>Тип</dt><dd>{inc.typeIds.map(typeTitle).join(', ') || '—'}</dd>
        <dt>Класс</dt><dd>{inc.outcomeType ?? '—'}</dd>
        <dt>Службы</dt>
        <dd data-testid="incident-view-services">
          {inc.services.map((id) => serviceTitle(id) + (inc.manualServices.includes(id) ? ' (вручную)' : ''))
            .join(', ') || '—'}
        </dd>
      </dl>
      {/* Признаки — той же сводкой, что увидит диспетчер в карточке ДДС */}
      {inc.typeIds.map((t) => <SurveySummary key={t} typeId={t} answers={inc.answers} />)}
      {inc.classifyReason && <p className={s.warn}>{inc.classifyReason}</p>}
    </section>
  );
};
