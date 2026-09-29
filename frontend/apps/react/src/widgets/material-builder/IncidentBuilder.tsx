import { useMemo, useState } from 'react';
import { ApiError, ddsIncidents, type Incident, type IncidentInput } from '@/shared/api';
import { useCatalog } from '@/entities/catalog/model';
import { AddressBlock } from '@/widgets/address-block/AddressBlock';
import { TypeSurvey } from './TypeSurvey';
import s from './MaterialBuilder.module.scss';

interface Props {
  /** Правка существующего; нет — новое */
  initial?: Incident | null;
  onSaved: (incident: Incident) => void;
  onCancel: () => void;
}

/**
 * Учебное происшествие ДДС — готовая карточка 112, собранная
 * преподавателем. Диспетчер получит её так же, как боевую: с адресом,
 * описанием, типом и службами. Службы подбирает классификатор по типу и
 * признакам; сверх них преподаватель может добавить службы из окна
 * «Добавьте службы».
 */
export const IncidentBuilder = ({ initial, onSaved, onCancel }: Props) => {
  const statuses = useCatalog((st) => st.applicantStatuses);
  const services = useCatalog((st) => st.services);
  const serviceTitle = useCatalog((st) => st.serviceTitle);

  const [title, setTitle] = useState(initial?.title ?? '');
  const [applicantName, setApplicantName] = useState(initial?.applicantName ?? '');
  const [applicantStatus, setApplicantStatus] = useState(initial?.applicantStatus || 'очевидец');
  const [phoneAon, setPhoneAon] = useState(initial?.phoneAon ?? '');
  const [address, setAddress] = useState<Record<string, string>>(initial?.address ?? {});
  const [description, setDescription] = useState(initial?.description ?? '');
  const [victims, setVictims] = useState<string>(initial?.victims != null ? String(initial.victims) : '');
  const [typeIds, setTypeIds] = useState<string[]>(initial?.typeIds ?? []);
  const [answers, setAnswers] = useState<Record<string, string[]>>(initial?.answers ?? {});
  const [extra, setExtra] = useState<string[]>(initial?.manualServices ?? []);
  const [query, setQuery] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Добавить вручную можно только службы из окна боевой системы
  const found = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return services
      .filter((sv) => sv.listed && !extra.includes(sv.id))
      .filter((sv) => [sv.title, sv.sourceTitle ?? '', ...sv.aliases]
        .some((t) => t.toLowerCase().includes(q)))
      .slice(0, 12);
  }, [query, services, extra]);

  const save = async () => {
    setBusy(true);
    setError(null);
    const body: IncidentInput = {
      title: title.trim(),
      applicantName, applicantStatus, phoneAon,
      address, description,
      victims: victims.trim() === '' ? null : Number(victims),
      typeIds, answers,
      extraServices: extra,
    };
    try {
      const saved = initial
        ? await ddsIncidents.update(initial.id, body)
        : await ddsIncidents.create(body);
      onSaved(saved);
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не сохранилось');
    } finally {
      setBusy(false);
    }
  };

  return (
    // Не <form>: плашки опросной карты и кнопки адреса — обычные кнопки
    // без type, и внутри формы каждая отправляла бы её
    <div className={s.form} data-testid="incident-builder">
      <h2 className={s.title}>
        {initial ? `Правка происшествия № ${initial.number}` : 'Собрать происшествие ДДС вручную'}
      </h2>

      <label className={s.field}>
        <span>Тема</span>
        <input value={title} required onChange={(e) => setTitle(e.target.value)}
               placeholder="Задымление мусоропровода" aria-label="Тема происшествия"
               data-testid="incident-title" />
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
          <span>Телефон (АОН)</span>
          <input value={phoneAon} onChange={(e) => setPhoneAon(e.target.value)}
                 aria-label="Телефон" />
        </label>
        <label className={s.field}>
          <span>Пострадавших</span>
          <input type="number" min={0} value={victims} onChange={(e) => setVictims(e.target.value)}
                 placeholder="нет" aria-label="Пострадавших" />
        </label>
      </div>

      <h3 className={s.section}>Адрес</h3>
      <AddressBlock value={address} onChange={(f, v) => setAddress((a) => ({ ...a, [f]: v }))} />

      <label className={s.field}>
        <span>Описание со слов заявителя</span>
        <textarea value={description} onChange={(e) => setDescription(e.target.value)}
                  placeholder="Задымление мусоропровода, дым в подъезде, пострадавших нет"
                  aria-label="Описание" data-testid="incident-description" />
      </label>

      <h3 className={s.section}>Тип происшествия и признаки</h3>
      <p className={s.hint}>По ним классификатор подберёт службы при сохранении.</p>
      <TypeSurvey typeIds={typeIds} answers={answers} onTypes={setTypeIds} setAnswers={setAnswers} />

      <h3 className={s.section}>Службы сверх подобранных классификатором</h3>
      <div className={s.services}>
        {extra.map((id) => (
          <span key={id} className={s.svc}>
            {serviceTitle(id)}
            <button type="button" aria-label={`Убрать ${serviceTitle(id)}`}
                    onClick={() => setExtra((x) => x.filter((v) => v !== id))}>×</button>
          </span>
        ))}
        {extra.length === 0 && <span className={s.note}>нет</span>}
      </div>
      <input className={s.search} value={query} onChange={(e) => setQuery(e.target.value)}
             placeholder="Найти службу: «Мослифт», «скорая», «Щукино»…" aria-label="Поиск службы" />
      {found.length > 0 && (
        <div className={s.pick}>
          {found.map((sv) => (
            <button key={sv.id} type="button" className={s.pickItem}
                    onClick={() => { setExtra((x) => [...x, sv.id]); setQuery(''); }}>
              {sv.title}
            </button>
          ))}
        </div>
      )}

      {error && <p className={s.error} role="alert">{error}</p>}
      <div className={s.actions}>
        <button className={s.primary} type="button" onClick={() => void save()} disabled={busy || !title.trim()}
                data-testid="incident-save">
          {busy ? 'сохраняю…' : 'Сохранить черновик'}
        </button>
        <button className={s.plain} type="button" onClick={onCancel}>Отмена</button>
      </div>
    </div>
  );
};
