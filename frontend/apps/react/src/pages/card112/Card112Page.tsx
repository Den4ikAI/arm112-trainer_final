import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTraining } from '@/entities/incident/model';
import { useCatalog } from '@/entities/catalog/model';
import { CallBar } from '@/widgets/call-bar/CallBar';
import { CardHeader } from '@/widgets/card-header/CardHeader';
import { AddressBlock } from '@/widgets/address-block/AddressBlock';
import { IncidentPanel } from '@/widgets/incident-panel/IncidentPanel';
import { SurveyCardView } from '@/widgets/survey-card/SurveyCardView';
import { ServicesBar } from '@/widgets/services-bar/ServicesBar';
import { TrainerHud } from '@/widgets/trainer-hud/TrainerHud';
import { Chip } from '@/shared/ui/Chip';
import { useTimer } from '@/shared/lib/useTimer';
import clsx from '@/shared/lib/clsx';
import s from './Card112Page.module.scss';

/** Карточка происшествия в режиме оператора 112 — основной учебный экран. */
export const Card112Page = () => {
  const { number = '' } = useParams();
  const nav = useNavigate();
  const {
    card, scenario, classify, revealed, transcript, grade, error, startedAt,
    edit, editAddress, toggleType, setAnswer, setManualServices, ask, save, flush, resume,
  } = useTraining();
  const applicantStatuses = useCatalog((st) => st.applicantStatuses);

  const elapsed = useTimer(!grade, startedAt);

  // После обновления страницы, повторного входа или в новой вкладке
  // вызов продолжается с того же места: сервер помнит карточку,
  // разговор и заданные вопросы. Раньше здесь сразу было «Занятие не
  // запущено», а повторный выбор билета заводил новую карточку
  const [resuming, setResuming] = useState(!card);
  useEffect(() => {
    if (card) return;
    let alive = true;
    void resume(number).finally(() => { if (alive) setResuming(false); });
    return () => { alive = false; };
    // Только при входе на страницу: дальше карточку ведёт хранилище
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Список оповещения считает сервер: здесь только то, что он прислал
  const auto = classify?.services.map((x) => x.serviceId) ?? [];

  useEffect(() => { if (grade) nav('/debrief'); }, [grade, nav]);

  // Уход со страницы не должен терять последние правки
  useEffect(() => () => { void flush(); }, [flush]);

  // Горячие клавиши боевой системы: Alt+T, Alt+A, Alt+O, Alt+S.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.altKey) return;
      const go = (sel: string) =>
        (document.querySelector(sel) as HTMLElement | null)?.focus();
      if (e.code === 'KeyT') { e.preventDefault(); go('#type-search'); }
      if (e.code === 'KeyA') { e.preventDefault(); go('[aria-label="Единая адресная строка"]'); }
      if (e.code === 'KeyO') { e.preventDefault(); go('[aria-label="Описание со слов заявителя"]'); }
      if (e.code === 'KeyS') { e.preventDefault(); void save(); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [save]);

  if (!card && resuming) {
    return (
      <div className={s.page}>
        <p className={s.empty} data-testid="resuming">Восстанавливаю вызов…</p>
      </div>
    );
  }

  if (!card) {
    return (
      <div className={s.page}>
        <p className={s.empty} data-testid="no-session">
          {number
            ? `Карточки ${number} нет среди ваших незаконченных вызовов: она уже сохранена или начата другим пользователем. Возьмите вызов в журнале.`
            : 'Занятие не запущено. Возьмите вызов в журнале.'}
        </p>
        <button onClick={() => nav('/journal')}>В журнал</button>
      </div>
    );
  }

  return (
    <div className={s.page}>
      <div className={s.top}>
        <CallBar
          aon={card.phoneAon} provided={card.phoneProvided} onSite={card.phoneOnSite}
          onChange={(f, v) => edit({ [f]: v })}
        />
        <CardHeader
          number={number || card.number}
          savedAt={new Date(card.createdAt).toLocaleString('ru-RU')}
          operator={card.operator} workstation={card.workstation ?? ''}
          mode="112" elapsedSec={elapsed} normSec={scenario?.normSec ?? 60}
        />
      </div>

      {error && <div className={s.error} role="status">{error}</div>}

      <div className={clsx(s.main, scenario && s.withHud)}>
        <div className={s.left}>
          <div className={s.applicant}>
            <input
              className={s.name}
              placeholder="Фамилия и имя заявителя"
              aria-label="ФИО заявителя"
              value={card.applicantName}
              onChange={(e) => edit({ applicantName: e.target.value })}
            />
            <select
              className={s.status}
              aria-label="Статус заявителя"
              value={card.applicantStatus}
              onChange={(e) => edit({ applicantStatus: e.target.value })}
            >
              <option value="">выберите статус</option>
              {applicantStatuses.map((st) => <option key={st} value={st}>{st}</option>)}
            </select>
          </div>

          <AddressBlock value={card.address} onChange={editAddress} />

          <div className={s.description}>
            <div className={s.descLabel}>Описание со слов заявителя</div>
            <textarea
              className={s.descInput}
              placeholder="введите"
              aria-label="Описание со слов заявителя"
              maxLength={1999}
              value={card.description}
              onChange={(e) => edit({ description: e.target.value })}
            />
            <div className={s.counter}>{card.description.length} / 1999</div>
          </div>
        </div>

        <div className={s.right}>
          <div className={s.flags}>
            <Chip selected={card.victims != null}
                  onClick={() => edit({ victims: card.victims == null ? 1 : null })}>
              Пострадавшие
            </Chip>
            <Chip stub="Отметка «нет на месте / отказ от скорой»">
              Нет на месте/<br />Отказ от скорой
            </Chip>
            <Chip stub="Отметка «нет доступа / заблокированные»">
              Нет доступа/<br />Заблокированные
            </Chip>
            <div className={s.spacer} />
            <Chip tone="danger" selected={card.noContact}
                  onClick={() => edit({ noContact: !card.noContact })}>нет контакта</Chip>
            <Chip tone="danger" selected={card.callDropped}
                  onClick={() => edit({ callDropped: !card.callDropped })}>срыв звонка</Chip>
          </div>

          <IncidentPanel selected={card.typeIds} onToggle={toggleType} />

          {card.typeIds.map((t) => (
            <SurveyCardView
              key={t} typeId={t} answers={card.answers}
              onAnswer={setAnswer} onClose={() => toggleType(t)}
            />
          ))}

          {/* Классификатор не смог определить позицию — говорим чего не хватает */}
          {classify?.reason && (
            <div className={s.hint} data-testid="classify-reason">{classify.reason}</div>
          )}
        </div>

        {scenario && (
          <TrainerHud
            scenario={scenario} revealed={revealed} transcript={transcript}
            elapsedSec={elapsed} onAsk={(id) => void ask(id)}
          />
        )}
      </div>

      <ServicesBar
        mode="112" serviceIds={card.serviceIds} autoIds={auto}
        onChange={setManualServices} onSave={() => void save()}
      />
    </div>
  );
};
