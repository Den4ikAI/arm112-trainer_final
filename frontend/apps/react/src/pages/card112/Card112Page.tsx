import { useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { APPLICANT_STATUSES, classify, type AddressFields } from '@arm112/domain';
import { useTraining } from '@/entities/incident/model';
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
  const { number = '36814851' } = useParams();
  const nav = useNavigate();
  const {
    card, scenario, revealed, transcript, saved,
    patch, patchAddress, toggleType, setAnswer, setServices, ask, save,
  } = useTraining();

  const elapsed = useTimer(!saved);
  const auto = classify(card).services;

  useEffect(() => { if (saved) nav('/debrief'); }, [saved, nav]);

  // Горячие клавиши боевой системы: Alt+T, Alt+A, Alt+S.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.altKey) return;
      const go = (sel: string) =>
        (document.querySelector(sel) as HTMLElement | null)?.focus();
      if (e.code === 'KeyT') { e.preventDefault(); go('#type-search'); }
      if (e.code === 'KeyA') { e.preventDefault(); go('[aria-label="Единая адресная строка"]'); }
      if (e.code === 'KeyO') { e.preventDefault(); go('[aria-label="Описание со слов заявителя"]'); }
      if (e.code === 'KeyS') { e.preventDefault(); save(); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [save]);

  return (
    <div className={s.page}>
      <div className={s.top}>
        <CallBar
          aon={card.phoneAon} provided={card.phoneProvided} onSite={card.phoneOnSite}
          onChange={(f, v) => patch({ [f]: v })}
        />
        <CardHeader
          number={number}
          savedAt={new Date(card.createdAt).toLocaleString('ru-RU')}
          operator={card.operator} workstation={card.workstation}
          mode="112" elapsedSec={elapsed} normSec={scenario?.normSec ?? 60}
        />
      </div>

      <div className={clsx(s.main, scenario && s.withHud)}>
        <div className={s.left}>
          <div className={s.applicant}>
            <input
              className={s.name}
              placeholder="Фамилия и имя заявителя"
              aria-label="ФИО заявителя"
              value={card.applicantName}
              onChange={(e) => patch({ applicantName: e.target.value })}
            />
            <select
              className={s.status}
              aria-label="Статус заявителя"
              value={card.applicantStatus}
              onChange={(e) => patch({ applicantStatus: e.target.value as never })}
            >
              <option value="">выберите статус</option>
              {APPLICANT_STATUSES.map((st) => <option key={st} value={st}>{st}</option>)}
            </select>
          </div>

          <AddressBlock
            value={card.address}
            onChange={(f, v) => patchAddress(f as keyof AddressFields, v)}
          />

          <div className={s.description}>
            <div className={s.descLabel}>Описание со слов заявителя</div>
            <textarea
              className={s.descInput}
              placeholder="введите"
              aria-label="Описание со слов заявителя"
              maxLength={1999}
              value={card.description}
              onChange={(e) => patch({ description: e.target.value })}
            />
            <div className={s.counter}>{card.description.length} / 1999</div>
          </div>
        </div>

        <div className={s.right}>
          <div className={s.flags}>
            <Chip selected={card.victims != null}
                  onClick={() => patch({ victims: card.victims == null ? 1 : null })}>
              Пострадавшие
            </Chip>
            <Chip>Нет на месте/<br />Отказ от скорой</Chip>
            <Chip>Нет доступа/<br />Заблокированные</Chip>
            <div className={s.spacer} />
            <Chip tone="danger" selected={card.noContact}
                  onClick={() => patch({ noContact: !card.noContact })}>нет контакта</Chip>
            <Chip tone="danger" selected={card.callDropped}
                  onClick={() => patch({ callDropped: !card.callDropped })}>срыв звонка</Chip>
          </div>

          <IncidentPanel selected={card.typeIds} onToggle={toggleType} />

          {card.typeIds.map((t) => (
            <SurveyCardView
              key={t} typeId={t} answers={card.answers}
              onAnswer={setAnswer} onClose={() => toggleType(t)}
            />
          ))}
        </div>

        {scenario && (
          <TrainerHud
            scenario={scenario} revealed={revealed} transcript={transcript}
            elapsedSec={elapsed} onAsk={ask}
          />
        )}
      </div>

      <ServicesBar
        mode="112" serviceIds={card.serviceIds} autoIds={auto}
        onChange={setServices} onSave={save}
      />

    </div>
  );
};
