import { useCallback, useEffect, useState } from 'react';
import { admin, type GroupMember } from '@/shared/api';
import { DutyEditor } from '@/widgets/duty-editor/DutyEditor';
import s from './GroupMembers.module.scss';
import { useStand } from '@/entities/stand/model';

/**
 * Курсанты группы и их рабочие места.
 *
 * Занятие ведётся на одном рабочем месте: вызовы 112 достаются
 * операторам 112, карточки ДДС — диспетчерам. Преподавателю нужно
 * видеть, кто где, и направить курсанта на другое место прямо здесь,
 * не уходя в администрирование.
 */
export const GroupMembers = ({ groupId, mode }: {
  groupId: string;
  mode: 'card112' | 'dds';
}) => {
  const [members, setMembers] = useState<GroupMember[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!groupId) return;
    try {
      setMembers(await admin.groupMembers(groupId));
      setError(null);
    } catch {
      setError('Не удалось получить список группы');
    }
  }, [groupId]);

  useEffect(() => { void load(); }, [load]);

  // Приём вызовов 112 свёрнут — об операторах молчим, пока их нет
  const op112 = useStand((st) => st.op112);
  const operators = members.filter((m) => m.duty === 'op112').length;
  const dispatchers = members.filter((m) => m.duty === 'dds').length;
  const suitable = mode === 'card112' ? operators : dispatchers;

  return (
    <section className={s.box} aria-label="Курсанты группы" data-testid="group-members">
      <h2 className={s.subtitle}>Курсанты группы</h2>
      <p className={s.summary}>
        {op112 || operators > 0
          ? <>Операторов 112: {operators}, диспетчеров ДДС: {dispatchers}.{' '}</>
          : <>Диспетчеров ДДС: {dispatchers}.{' '}</>}
        {suitable === 0
          ? <b>В этом режиме заниматься некому — назначьте рабочее место ниже.</b>
          : mode === 'card112'
            ? 'Вызовы 112 получат только операторы.'
            : 'Карточки ДДС придут только диспетчерам — по их службам.'}
      </p>
      {error && <p className={s.error} role="alert">{error}</p>}
      <ul className={s.list}>
        {members.map((m) => (
          <li key={m.id} className={s.row}>
            <span className={s.fio}>{m.fio}</span>
            <DutyEditor key={`${m.duty}-${m.serviceId}`} userId={m.id} login={m.login}
                        duty={m.duty} serviceId={m.serviceId} onChanged={() => void load()} />
          </li>
        ))}
      </ul>
    </section>
  );
};
