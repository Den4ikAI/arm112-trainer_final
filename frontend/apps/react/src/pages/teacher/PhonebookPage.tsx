import { useEffect, useState } from 'react';
import { useSession } from '@/entities/session/model';
import { useCatalog } from '@/entities/catalog/model';
import { ApiError, phonebook, type PhoneBook, type PhoneBookEntry, type PhonePeer } from '@/shared/api';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import s from './Teacher.module.scss';

/**
 * Телефонный справочник ДДС.
 *
 * Рекомендации финалистам, п. 8: перечень абонентов виртуального
 * IP-телефона и их условные номера формирует преподаватель — не более
 * десяти на одну ДДС. Роли: руководитель организации, начальник отдела
 * реагирования, начальник и специалист аварийной бригады. Заявитель —
 * отдельная строка без номера: включил её преподаватель — диспетчер
 * сможет набрать номер заявителя из карточки и поговорить с ним.
 *
 * Справочник один на службу, правится таблицей целиком и сохраняется
 * кнопкой. Пока преподаватель ничего не менял, служба живёт со
 * справочником по умолчанию.
 */
export const PhonebookPage = () => {
  const user = useSession((st) => st.user);
  const services = useCatalog((st) => st.services);
  const loadCatalog = useCatalog((st) => st.load);
  const [chosen, setChosen] = useState('');
  // Пока преподаватель не выбрал службу — первая из справочника
  const serviceId = chosen || services[0]?.id || '';
  const [book, setBook] = useState<PhoneBook | null>(null);
  const [rows, setRows] = useState<PhoneBookEntry[]>([]);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => { if (services.length === 0) void loadCatalog(); }, [services.length, loadCatalog]);
  useEffect(() => {
    if (!serviceId) return;
    let alive = true;
    void phonebook.view(serviceId)
      .then((b) => { if (alive) { setBook(b); setRows(b.entries); setNote(null); setError(null); } })
      .catch(() => { if (alive) setError('Справочник не загрузился'); });
    return () => { alive = false; };
  }, [serviceId]);

  if (!user || user.role === 'student') {
    return <div className={s.page}><p className={s.empty}>Раздел доступен преподавателю.</p></div>;
  }

  const max = book?.max ?? 10;
  const roleTitle = (role: PhonePeer) => book?.roles.find((r) => r.role === role)?.title ?? role;
  const update = (i: number, patch: Partial<PhoneBookEntry>) =>
    setRows((r) => r.map((e, k) => (k === i ? { ...e, ...patch } : e)));
  const add = () => {
    if (rows.length >= max) return;
    // Новая строка — специалист бригады: их в ДДС больше всего
    setRows((r) => [...r, { role: 'brigade_specialist', number: '', title: roleTitle('brigade_specialist') }]);
  };
  const remove = (i: number) => setRows((r) => r.filter((_, k) => k !== i));
  const hasApplicant = rows.some((e) => e.role === 'applicant');
  const addApplicant = () => {
    if (hasApplicant || rows.length >= max) return;
    setRows((r) => [...r, { role: 'applicant', number: '', title: 'Заявитель' }]);
  };

  const save = async () => {
    setBusy(true); setNote(null); setError(null);
    try {
      const b = await phonebook.save(serviceId, rows);
      setBook(b); setRows(b.entries);
      setNote('Сохранено. Диспетчеры службы увидят справочник при следующем открытии карточки.');
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не сохранилось');
    } finally {
      setBusy(false);
    }
  };
  const reset = async () => {
    setBusy(true); setNote(null); setError(null);
    try {
      const b = await phonebook.reset(serviceId);
      setBook(b); setRows(b.entries);
      setNote('Справочник по умолчанию.');
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не сбросилось');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={s.page}>
      <header className={s.head}>
        <div>
          <h1 className={s.title}>Телефоны ДДС</h1>
          <p className={s.sub}>
            Кому диспетчер может позвонить с виртуального IP-телефона: до {max} номеров на службу.
            Строка «Заявитель» разрешает звонок заявителю по номеру из карточки.
          </p>
        </div>
        <div className={s.tools}>
          <UserMenu />
        </div>
      </header>

      <section className={s.card} aria-label="Телефонный справочник службы">
        <div className={s.head} style={{ marginBottom: 8 }}>
          <label className={s.cardTitle}>
            Служба{' '}
            <select value={serviceId} data-testid="phonebook-service"
                    onChange={(e) => {
                      // Та же служба — справочник уже загружен; сброс без новой
                      // загрузки оставил бы «Загрузка…» навсегда
                      if (e.target.value === serviceId) return;
                      setChosen(e.target.value);
                      setBook(null);
                    }}>
              {services.map((sv) => <option key={sv.id} value={sv.id}>{sv.title}</option>)}
            </select>
          </label>
          <div className={s.tools}>
            {book?.isDefault && <span className={s.muted} data-testid="phonebook-default">справочник по умолчанию</span>}
            <button className={s.btn} disabled={busy || rows.length >= max} data-testid="phonebook-add"
                    onClick={add}>+ Абонент</button>
            <button className={s.btn} disabled={busy || hasApplicant || rows.length >= max}
                    data-testid="phonebook-add-applicant" onClick={addApplicant}>+ Заявитель</button>
            <button className={s.btn} disabled={busy || !book || book.isDefault} onClick={() => void reset()}>
              По умолчанию
            </button>
            <button className={s.primary} disabled={busy || !book} data-testid="phonebook-save"
                    onClick={() => void save()}>Сохранить</button>
          </div>
        </div>

        {!book && !error && <p className={s.empty}>Загрузка…</p>}
        {book && (
          <div className={s.scroll}>
            <table className={s.table} data-testid="phonebook-table">
              <thead>
                <tr><th>№</th><th>Роль</th><th>Номер</th><th>Подпись в телефоне (должность, ФИО)</th><th /></tr>
              </thead>
              <tbody>
                {rows.map((e, i) => (
                  <tr key={i} data-testid="phonebook-row">
                    <td>{i + 1}</td>
                    <td>
                      <select value={e.role} aria-label="Роль" data-testid="phonebook-role"
                              onChange={(ev) => update(i, { role: ev.target.value as PhonePeer,
                                number: ev.target.value === 'applicant' ? '' : e.number })}>
                        {book.roles.map((r) => <option key={r.role} value={r.role}>{r.title}</option>)}
                      </select>
                    </td>
                    <td>
                      {e.role === 'applicant'
                        ? <span className={s.muted}>из карточки</span>
                        : <input value={e.number} inputMode="numeric" size={6} maxLength={6}
                                 aria-label="Номер" data-testid="phonebook-number" placeholder="3001"
                                 onChange={(ev) => update(i, { number: ev.target.value.replace(/\D/g, '') })} />}
                    </td>
                    <td>
                      <input value={e.title} size={40} maxLength={80} aria-label="Подпись"
                             data-testid="phonebook-title"
                             onChange={(ev) => update(i, { title: ev.target.value })} />
                    </td>
                    <td>
                      <button className={s.btn} disabled={busy} title="Убрать из справочника"
                              data-testid="phonebook-remove" onClick={() => remove(i)}>×</button>
                    </td>
                  </tr>
                ))}
                {rows.length === 0 && (
                  <tr><td colSpan={5} className={s.empty}>Пусто — у диспетчера не будет ни одного абонента.</td></tr>
                )}
              </tbody>
            </table>
          </div>
        )}
        <p className={s.muted}>{rows.length} из {max}</p>
        {note && <p className={s.muted} role="status" data-testid="phonebook-note">{note}</p>}
        {error && <p className={s.down} role="alert">{error}</p>}
      </section>
    </div>
  );
};
