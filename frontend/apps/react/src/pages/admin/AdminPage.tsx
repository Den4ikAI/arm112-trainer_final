import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  admin, ApiError,
  type AdminUser, type AuditPage, type ClassifierVersion, type Health,
} from '@/shared/api';
import { useSession } from '@/entities/session/model';
import { useCatalog } from '@/entities/catalog/model';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import clsx from '@/shared/lib/clsx';
import s from './AdminPage.module.scss';

type Tab = 'audit' | 'users' | 'classifier' | 'health';

/**
 * Экран администратора.
 *
 * Журнал действий здесь не украшение: раздел «Аудит» инструкции
 * требует, чтобы было видно, кто, что и когда сделал. Разбирают по
 * нему обычно не всё подряд, а «что делал этот человек в этот день»
 * или «кто трогал эту карточку» — отсюда и набор фильтров.
 *
 * Преподаватель видит журнал, людей и версии классификатора;
 * заводить и отключать учётные записи может только администратор.
 */
export const AdminPage = () => {
  const [tab, setTab] = useState<Tab>('audit');
  const user = useSession((st) => st.user);
  const nav = useNavigate();

  // ТЗ отдаёт администратору управление учётными записями, системные
  // журналы и конфигурацию. Преподавателю сюда не нужно — у него
  // отчёты и аналитика по группе
  if (!user || user.role !== 'admin') {
    return (
      <div className={s.page}>
        <p className={s.empty}>Раздел доступен администратору системы.</p>
        <button className={s.back} onClick={() => nav('/journal')}>В журнал</button>
      </div>
    );
  }

  const tabs: Array<[Tab, string]> = [
    ['audit', 'Журнал действий'],
    ['users', 'Пользователи'],
    ['classifier', 'Классификатор'],
    ['health', 'Состояние'],
  ];

  return (
    <div className={s.page}>
      <header className={s.head}>
        <h1 className={s.title}>Администрирование</h1>
        <div className={s.tools}>
          <UserMenu compact />
          <button className={s.back} onClick={() => nav('/journal')}>← В журнал</button>
        </div>
      </header>

      <nav className={s.tabs} role="tablist">
        {tabs.map(([id, label]) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            className={clsx(s.tab, tab === id && s.tabOn)}
            data-testid={`admin-tab-${id}`}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </nav>

      {tab === 'audit' && <AuditTab />}
      {tab === 'users' && <UsersTab />}
      {tab === 'classifier' && <ClassifierTab />}
      {tab === 'health' && <HealthTab />}
    </div>
  );
};

// ─── Журнал действий ──────────────────────────────────────────────

const AuditTab = () => {
  const [data, setData] = useState<AuditPage | null>(null);
  const [action, setAction] = useState('');
  const [entityId, setEntityId] = useState('');
  const [page, setPage] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    admin.audit({ action: action || undefined, entityId: entityId || undefined, page, size: 50 })
      .then((d) => { if (alive) { setData(d); setError(null); } })
      .catch(() => { if (alive) setError('Не удалось получить журнал'); });
    return () => { alive = false; };
  }, [action, entityId, page]);

  if (error) return <p className={s.empty}>{error}</p>;
  if (!data) return <p className={s.empty}>Загрузка…</p>;

  const pages = Math.ceil(data.total / data.size);

  return (
    <section className={s.panel}>
      <div className={s.filters}>
        <label className={s.filter}>
          <span>Действие</span>
          <select value={action} aria-label="Фильтр по действию"
                  onChange={(e) => { setAction(e.target.value); setPage(0); }}>
            <option value="">все</option>
            {data.actions.map((a) => <option key={a} value={a}>{a}</option>)}
          </select>
        </label>

        <label className={s.filter}>
          <span>Номер карточки или занятия</span>
          <input value={entityId} placeholder="идентификатор"
                 aria-label="Фильтр по объекту"
                 onChange={(e) => { setEntityId(e.target.value.trim()); setPage(0); }} />
        </label>

        <div className={s.total} data-testid="audit-total">Записей: {data.total}</div>
      </div>

      <div className={s.scroll}>
        <table className={s.table}>
          <thead>
            <tr>
              <th>Когда</th><th>Кто</th><th>Действие</th>
              <th>Объект</th><th>Адрес</th><th>Подробности</th>
            </tr>
          </thead>
          <tbody>
            {data.items.length === 0 && (
              <tr><td className={s.empty} colSpan={6}>Записей нет</td></tr>
            )}
            {data.items.map((e) => (
              <tr key={e.id} data-testid="audit-row">
                <td className={s.when}>{new Date(e.at).toLocaleString('ru-RU')}</td>
                {/* Действия сторожа нормативов делает система, человека за ними нет */}
                <td>{e.fio ?? <span className={s.system}>система</span>}</td>
                <td className={s.action}>{e.action}</td>
                <td className={s.entity}>
                  {e.entity && <>{e.entity} {e.entityId?.slice(0, 8)}</>}
                </td>
                <td className={s.ip}>{e.ip ?? '—'}</td>
                <td className={s.payload}>
                  {e.payload ? JSON.stringify(e.payload, null, 0) : ''}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {pages > 1 && (
        <div className={s.pager}>
          <button disabled={page === 0} onClick={() => setPage((p) => p - 1)}>←</button>
          <span>{page + 1} из {pages}</span>
          <button disabled={page + 1 >= pages} onClick={() => setPage((p) => p + 1)}>→</button>
        </div>
      )}
    </section>
  );
};

// ─── Пользователи ─────────────────────────────────────────────────

const UsersTab = () => {
  const [users, setUsers] = useState<AdminUser[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  const reload = useCallback(async () => {
    try {
      setUsers(await admin.users());
      setError(null);
    } catch {
      setError('Не удалось получить список');
    }
  }, []);

  useEffect(() => { void reload(); }, [reload]);

  const [notice, setNotice] = useState<string | null>(null);

  // С подтверждением: удаление записи с историей её обезличивает,
  // и вернуть человека можно только из резервной копии
  const remove = async (u: AdminUser) => {
    const ok = window.confirm(
      `Удалить учётную запись ${u.login} (${u.fio})?\n\n` +
      'Если у неё есть история обучения, данные сначала сохранятся в резервную ' +
      'копию, а запись будет обезличена: оценки и журнал действий останутся.',
    );
    if (!ok) return;
    try {
      const r = await admin.deleteUser(u.id);
      setNotice(r.mode === 'deleted'
        ? `Запись ${u.login} удалена: истории у неё не было.`
        : `Запись ${u.login} обезличена, данные сохранены в резервную копию.`);
      await reload();
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не удалось удалить');
    }
  };

  const toggle = async (u: AdminUser) => {
    try {
      await admin.setActive(u.id, !u.isActive);
      await reload();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : 'Не удалось изменить');
    }
  };

  return (
    <section className={s.panel}>
      {error && <div className={s.error} role="alert">{error}</div>}
      {notice && <div className={s.note} role="status" data-testid="user-notice">{notice}</div>}

      <div className={s.toolbar}>
        <button className={s.primary} data-testid="add-user"
                onClick={() => setAdding((v) => !v)}>
          {adding ? 'Отмена' : 'Завести пользователя'}
        </button>
      </div>

      {adding && <NewUserForm onDone={() => { setAdding(false); void reload(); }} />}

      <div className={s.scroll}>
        <table className={s.table}>
          <thead>
            <tr>
              <th>Логин</th><th>ФИО</th><th>Роль</th>
              <th>Служба</th><th>АРМ</th><th>Занятий</th><th>Состояние</th>
            </tr>
          </thead>
          <tbody>
            {users.map((u) => (
              <tr key={u.id} data-testid={`user-${u.login}`}
                  className={clsx(!u.isActive && s.inactive)}>
                <td>{u.login}</td>
                <td>{u.fio}</td>
                <td>{ROLE_TITLE[u.role]}</td>
                <td>{u.serviceTitle ?? '—'}</td>
                <td>{u.workstation ?? '—'}</td>
                <td className={s.num}>{u.sessions}</td>
                <td>
                  <button className={s.link} onClick={() => void toggle(u)}>
                    {u.isActive ? 'отключить' : 'включить'}
                  </button>{' '}
                  <button className={s.link} data-testid={`delete-${u.login}`}
                          onClick={() => void remove(u)}>
                    удалить
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* ТЗ запрещает удалять критически важные данные без резервной
          копии: история обучения сохраняется, человек исчезает */}
      <p className={s.note}>
        Запись без истории удаляется сразу. Запись с историей обучения
        сначала копируется в резервную копию, затем обезличивается: оценки,
        карточки и журнал действий остаются читаемыми.
      </p>
    </section>
  );
};

const ROLE_TITLE: Record<string, string> = {
  student: 'Обучающийся',
  teacher: 'Преподаватель',
  admin: 'Администратор',
};

const NewUserForm = ({ onDone }: { onDone: () => void }) => {
  const services = useCatalog((st) => st.services);
  const [form, setForm] = useState({
    login: '', password: '', fio: '', role: 'student',
    serviceId: '', workstation: '',
  });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const set = (k: keyof typeof form) => (v: string) =>
    setForm((f) => ({ ...f, [k]: v }));

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await admin.createUser({
        login: form.login,
        password: form.password,
        fio: form.fio,
        role: form.role,
        // Служба только у курсанта: у преподавателя её быть не должно,
        // сервер это тоже проверяет
        serviceId: form.role === 'student' && form.serviceId ? form.serviceId : null,
        workstation: form.workstation || null,
      });
      onDone();
    } catch (err) {
      setError(err instanceof ApiError ? (err.detail ?? err.message) : 'Не удалось завести');
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className={s.form} onSubmit={(e) => void submit(e)}>
      <label className={s.field}>
        <span>Логин</span>
        <input value={form.login} required aria-label="Логин"
               onChange={(e) => set('login')(e.target.value)} />
      </label>
      <label className={s.field}>
        <span>Пароль</span>
        <input type="password" value={form.password} required minLength={8}
               aria-label="Пароль" onChange={(e) => set('password')(e.target.value)} />
      </label>
      <label className={s.field}>
        <span>ФИО</span>
        <input value={form.fio} required aria-label="ФИО"
               onChange={(e) => set('fio')(e.target.value)} />
      </label>
      <label className={s.field}>
        <span>Роль</span>
        <select value={form.role} aria-label="Роль"
                onChange={(e) => set('role')(e.target.value)}>
          <option value="student">Обучающийся</option>
          <option value="teacher">Преподаватель</option>
          <option value="admin">Администратор</option>
        </select>
      </label>
      {form.role === 'student' && (
        <>
          <label className={s.field}>
            <span>Служба (ДДС)</span>
            {/* Обязательна: по службе отбирается лента курсанта,
                без неё главный экран у него пустой */}
            <select value={form.serviceId} aria-label="Служба" required
                    onChange={(e) => set('serviceId')(e.target.value)}>
              <option value="">выберите службу</option>
              {services.map((sv) => (
                <option key={sv.id} value={sv.id}>{sv.title}</option>
              ))}
            </select>
          </label>
          <label className={s.field}>
            <span>Номер АРМ</span>
            <input value={form.workstation} aria-label="Номер АРМ"
                   onChange={(e) => set('workstation')(e.target.value)} />
          </label>
        </>
      )}

      {error && <div className={s.error} role="alert">{error}</div>}

      <button className={s.primary} type="submit" disabled={busy}
              data-testid="save-user">
        {busy ? '…' : 'Завести'}
      </button>
    </form>
  );
};

// ─── Классификатор ────────────────────────────────────────────────

const ClassifierTab = () => {
  const [versions, setVersions] = useState<ClassifierVersion[]>([]);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setVersions(await admin.versions());
      setError(null);
    } catch {
      setError('Не удалось получить список версий');
    }
  }, []);

  useEffect(() => { void reload(); }, [reload]);

  const activate = async (v: ClassifierVersion) => {
    try {
      await admin.activateVersion(v.id);
      await reload();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : 'Не удалось переключить');
    }
  };

  return (
    <section className={s.panel}>
      {error && <div className={s.error} role="alert">{error}</div>}

      <div className={s.scroll}>
        <table className={s.table}>
          <thead>
            <tr><th>Версия</th><th>Файл</th><th>Позиций</th><th>Загружена</th><th /></tr>
          </thead>
          <tbody>
            {versions.map((v) => (
              <tr key={v.id} data-testid="version-row"
                  className={clsx(v.isActive && s.activeRow)}>
                <td>{v.name}</td>
                <td className={s.file}>{v.sourceFile}</td>
                <td className={s.num}>{v.positionsCount}</td>
                <td className={s.when}>{new Date(v.importedAt).toLocaleString('ru-RU')}</td>
                <td>
                  {v.isActive
                    ? <span className={s.badge} data-testid="version-active">активна</span>
                    : (
                        <button className={s.link} onClick={() => void activate(v)}>
                          сделать активной
                        </button>
                      )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className={s.note}>
        Активная версия используется и для подбора служб, и для эталона
        при разборе. Уже выставленные оценки не пересчитываются: они
        зафиксировали тот справочник, что действовал в момент сохранения.
      </p>
    </section>
  );
};

// ─── Состояние ────────────────────────────────────────────────────

const HealthTab = () => {
  const [health, setHealth] = useState<Health | null>(null);
  // До первого ответа — загрузка, а не «сервер не отвечает»:
  // иначе каждое открытие вкладки начиналось с ложной тревоги
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const load = () => void admin.health()
      .then((h) => { setHealth(h); setFailed(false); })
      .catch(() => { setHealth(null); setFailed(true); });
    load();
    const timer = setInterval(load, 5000);
    return () => clearInterval(timer);
  }, []);

  if (!health) {
    return <p className={s.empty}>{failed ? 'Сервер не отвечает' : 'Загрузка…'}</p>;
  }

  // Поле, которого нет в ответе, — «нет данных», а не пустое место:
  // пустота не отличает ноль задач от поломки
  const show = (v: number | string | undefined) =>
    v === undefined || v === null || v === '' ? 'нет данных' : String(v);

  const uptime = (sec: number | undefined) => {
    if (typeof sec !== 'number' || !Number.isFinite(sec)) return 'нет данных';
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    return h ? `${h} ч ${m} мин` : `${m} мин ${sec % 60} с`;
  };

  return (
    <section className={s.panel}>
      <dl className={s.stats} data-testid="health">
        <div><dt>База данных</dt><dd>{health.db ? 'отвечает' : 'недоступна'}</dd></div>
        <div><dt>Время работы</dt><dd>{uptime(health.uptimeSec)}</dd></div>
        <div><dt>Адаптеры моделей</dt><dd>{show(health.mlProvider)}</dd></div>
        <div><dt>Открытых каналов класса</dt><dd>{show(health.wsListeners)}</dd></div>
        <div><dt>Задач в очереди</dt><dd>{show(health.jobsQueued)}</dd></div>
        {/* Залипшая задача видна по тому, что это число не падает */}
        <div><dt>Задач в работе</dt><dd>{show(health.jobsRunning)}</dd></div>
        <div><dt>Версия</dt><dd>{show(health.version)}</dd></div>
      </dl>
    </section>
  );
};
