import { useState } from 'react';
import { ApiError, scenarios, type ScenarioFull } from '@/shared/api';
import { useCatalog } from '@/entities/catalog/model';
import { AddressBlock } from '@/widgets/address-block/AddressBlock';
import { TypeSurvey } from './TypeSurvey';
import s from './MaterialBuilder.module.scss';

interface Props {
  onCreated: (scenario: ScenarioFull) => void;
  onCancel: () => void;
}

type Fact = { question: string; answer: string };

/**
 * Учебный вызов 112, собранный вручную.
 *
 * Эталон — это то, что оператор должен записать: тип и признаки опросной
 * карты, адрес. Службы и итоговый тип посчитает классификатор при
 * сохранении, как у вызовов из билетов. Вопросы — то, что заявитель
 * скажет только в ответ: их курсанту и надо выяснить.
 */
export const ScenarioBuilder = ({ onCreated, onCancel }: Props) => {
  const statuses = useCatalog((st) => st.applicantStatuses);
  const [title, setTitle] = useState('');
  const [legend, setLegend] = useState('');
  const [applicantName, setApplicantName] = useState('');
  const [applicantPhone, setApplicantPhone] = useState('');
  const [applicantStatus, setApplicantStatus] = useState('очевидец');
  const [normSec, setNormSec] = useState(60);
  const [typeIds, setTypeIds] = useState<string[]>([]);
  const [answers, setAnswers] = useState<Record<string, string[]>>({});
  const [address, setAddress] = useState<Record<string, string>>({});
  const [facts, setFacts] = useState<Fact[]>([{ question: '', answer: '' }]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const setFact = (i: number, patch: Partial<Fact>) =>
    setFacts((f) => f.map((x, j) => (j === i ? { ...x, ...patch } : x)));

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const created = await scenarios.createManual({
        title: title.trim(),
        legend: legend.trim(),
        applicantName, applicantPhone, applicantStatus,
        normSec,
        typeIds,
        answers,
        address,
        hidden: facts
          .filter((f) => f.question.trim() && f.answer.trim())
          .map((f, i) => ({ key: `f${i + 1}`, question: f.question.trim(), answer: f.answer.trim() })),
      });
      onCreated(created);
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не сохранилось');
    } finally {
      setBusy(false);
    }
  };

  return (
    // Не <form>: плашки опросной карты и кнопки адреса — обычные кнопки
    // без type, и внутри формы каждая отправляла бы её
    <div className={s.form} data-testid="scenario-builder">
      <h2 className={s.title}>Собрать вызов 112 вручную</h2>

      <div className={s.row}>
        <label className={s.field}>
          <span>Тема</span>
          <input value={title} required onChange={(e) => setTitle(e.target.value)}
                 placeholder="Пожар в частном доме" aria-label="Тема вызова"
                 data-testid="builder-title" />
        </label>
        <label className={s.field}>
          <span>Норматив заполнения, секунд</span>
          <input type="number" min={20} max={1800} value={normSec}
                 onChange={(e) => setNormSec(Number(e.target.value))} aria-label="Норматив" />
        </label>
      </div>

      <label className={s.field}>
        <span>Первая фраза заявителя</span>
        <textarea value={legend} required onChange={(e) => setLegend(e.target.value)}
                  placeholder="Алло! Горит крыша частного дома, приезжайте скорее!"
                  aria-label="Первая фраза заявителя" data-testid="builder-legend" />
      </label>

      <div className={s.row}>
        <label className={s.field}>
          <span>Заявитель (ФИО)</span>
          <input value={applicantName} onChange={(e) => setApplicantName(e.target.value)}
                 aria-label="ФИО заявителя" />
        </label>
        <label className={s.field}>
          <span>Статус заявителя</span>
          <select value={applicantStatus} onChange={(e) => setApplicantStatus(e.target.value)}
                  aria-label="Статус заявителя">
            {(statuses.length ? statuses : ['очевидец']).map((st) => <option key={st}>{st}</option>)}
          </select>
        </label>
        <label className={s.field}>
          <span>Телефон</span>
          <input value={applicantPhone} onChange={(e) => setApplicantPhone(e.target.value)}
                 placeholder="916-126-34-71" aria-label="Телефон заявителя" />
        </label>
      </div>

      <h3 className={s.section}>Эталон: тип происшествия и признаки</h3>
      <p className={s.hint}>Что оператор должен выбрать. Службы подберёт классификатор.</p>
      <TypeSurvey typeIds={typeIds} answers={answers} onTypes={setTypeIds} setAnswers={setAnswers} />

      <h3 className={s.section}>Эталон: адрес</h3>
      <AddressBlock value={address} onChange={(f, v) => setAddress((a) => ({ ...a, [f]: v }))} />

      <h3 className={s.section}>Что заявитель скажет только в ответ на вопрос</h3>
      <div className={s.facts}>
        {facts.map((f, i) => (
          <div key={i} className={s.fact}>
            <input value={f.question} placeholder="Вопрос оператора: «Пострадавшие есть?»"
                   aria-label={`Вопрос ${i + 1}`} data-testid={`builder-q-${i}`}
                   onChange={(e) => setFact(i, { question: e.target.value })} />
            <input value={f.answer} placeholder="Ответ заявителя: «Нет, все вышли»"
                   aria-label={`Ответ ${i + 1}`} data-testid={`builder-a-${i}`}
                   onChange={(e) => setFact(i, { answer: e.target.value })} />
            <button type="button" className={s.plain} aria-label="Убрать вопрос"
                    onClick={() => setFacts((x) => x.filter((_, j) => j !== i))}>×</button>
          </div>
        ))}
        <div>
          <button type="button" className={s.plain} data-testid="builder-add-fact"
                  onClick={() => setFacts((x) => [...x, { question: '', answer: '' }])}>
            + вопрос
          </button>
        </div>
      </div>

      {error && <p className={s.error} role="alert">{error}</p>}
      <div className={s.actions}>
        <button className={s.primary} type="button" onClick={() => void save()} data-testid="builder-save"
                disabled={busy || !title.trim() || !legend.trim()}>
          {busy ? 'сохраняю…' : 'Сохранить черновик'}
        </button>
        <button className={s.plain} type="button" onClick={onCancel}>Отмена</button>
        <span className={s.note}>Утвердить можно после сохранения — в карточке вызова.</span>
      </div>
    </div>
  );
};
