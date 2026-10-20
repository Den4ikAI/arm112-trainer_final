import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useSession } from '@/entities/session/model';
import { useStand } from '@/entities/stand/model';
import { ApiError, stand, type DemoAccount } from '@/shared/api';
import s from './LoginPage.module.scss';

/** Кто это и что у него смотреть — подпись учётной записи стенда. */
const роль = (a: DemoAccount): [string, string] => {
  if (a.role === 'teacher') return ['Преподаватель', 'учебный материал и карточки от ИИ, занятия, аналитика'];
  if (a.role === 'admin') return ['Администратор', 'учётные записи, классификатор, параметры, журнал аудита'];
  if (a.duty === 'dds') return [`Диспетчер ДДС · ${a.service ?? ''}`, 'лента службы, статусы, бригада, тренировка с ИИ'];
  return ['Оператор 112', 'приём вызова и заполнение карточки 112'];
};

/**
 * Включение системы 112 прямо с экрана входа.
 *
 * Заказчик (рекомендации финалистам, п. 5): функционал оператора 112
 * скрыть или включать только через администратора. Поэтому переключатель
 * спрашивает логин и пароль администратора, а право проверяет сервер:
 * чужой учётной записью параметр не поменять.
 */
const Op112Switch = () => {
  const op112 = useStand((st) => st.op112);
  const reload = useStand((st) => st.load);
  const [open, setOpen] = useState(false);
  const [login, setLogin] = useState('admin');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const apply = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await stand.setOp112(login, password, !op112);
      await reload();
      setOpen(false);
      setPassword('');
    } catch (err) {
      const st = err instanceof ApiError ? err.status : 0;
      setError(st === 401 ? 'Неверный логин или пароль'
        : st === 403 ? 'Включать систему 112 может только администратор'
          : 'Не удалось изменить, попробуйте ещё раз');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={s.sys} data-testid="op112-switch">
      <div className={s.sysRow}>
        <span>Система 112: <b className={op112 ? s.sysOn : s.sysOff} data-testid="op112-state">
          {op112 ? 'включена' : 'выключена'}</b></span>
        {!open && (
          <button type="button" className={s.sysBtn} data-testid="op112-toggle"
                  onClick={() => { setOpen(true); setError(null); }}>
            {op112 ? 'Выключить' : 'Включить'}
          </button>
        )}
      </div>
      <p className={s.demoWhat}>
        Приём вызовов оператором 112 и учебный материал для него. Тренажёр
        по умолчанию — для диспетчеров ДДС; включает и выключает администратор.
      </p>
      {open && (
        <form className={s.sysForm} onSubmit={(e) => void apply(e)} data-testid="op112-form">
          <input value={login} onChange={(e) => setLogin(e.target.value)} aria-label="Логин администратора"
                 placeholder="логин администратора" autoComplete="off" name="op112-login" />
          <input type="password" value={password} onChange={(e) => setPassword(e.target.value)}
                 aria-label="Пароль администратора" placeholder="пароль" autoComplete="off"
                 name="op112-password" autoFocus />
          <button type="submit" className={s.sysBtn} disabled={busy || !login || !password}
                  data-testid="op112-apply">
            {busy ? '…' : op112 ? 'Выключить 112' : 'Включить 112'}
          </button>
          <button type="button" className={s.sysCancel} onClick={() => setOpen(false)}>отмена</button>
          {error && <div className={s.sysError} role="alert" data-testid="op112-error">{error}</div>}
        </form>
      )}
    </div>
  );
};

/**
 * Учебные учётные записи стенда — вход одним щелчком.
 *
 * Эксперты 18.10: «пароли и логины вывести на переднюю панель, чтобы мы
 * не искали их внутри технических решений». Список даёт сервер и только
 * на демонстрационном стенде; в рабочей установке его нет.
 */
const DemoAccess = ({ onPick, busy }: {
  onPick: (login: string, password: string) => void; busy: boolean;
}) => {
  const demo = useStand((st) => st.demo);
  if (!demo || demo.accounts.length === 0) {
    return <section className={s.demo} aria-label="Система 112"><Op112Switch /></section>;
  }
  return (
    <section className={s.demo} aria-label="Учебные учётные записи" data-testid="demo-access">
      <h2 className={s.demoTitle}>Учебный стенд — войти одним щелчком</h2>
      <p className={s.demoHint}>
        Пароль у всех учётных записей: <code className={s.demoCode}>{demo.password}</code>
      </p>
      <ul className={s.demoList}>
        {demo.accounts.map((a) => {
          const [title, what] = роль(a);
          return (
            <li key={a.login}>
              <button type="button" className={s.demoBtn} disabled={busy}
                      data-testid={`demo-${a.login}`}
                      onClick={() => onPick(a.login, demo.password)}>
                <span className={s.demoRole}>{title}</span>
                <code className={s.demoLogin}>{a.login}</code>
                <span className={s.demoWhat}>{what}</span>
              </button>
            </li>
          );
        })}
      </ul>
      <Op112Switch />
    </section>
  );
};

/** Экран входа. Композиция повторяет боевой: иллюстрация города и форма справа. */
export const LoginPage = () => {
  const [login, setLogin] = useState('');
  const [password, setPassword] = useState('');
  const [workstation, setWorkstation] = useState('4');
  const [busy, setBusy] = useState(false);
  const signIn = useSession((st) => st.signIn);
  const error = useSession((st) => st.error);
  const nav = useNavigate();

  const enter = async (who: string, secret: string) => {
    setBusy(true);
    const ok = await signIn(who, secret, workstation);
    setBusy(false);
    if (ok) nav('/journal');
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    await enter(login, password);
  };

  // Щелчок по учётной записи стенда заполняет форму и сразу входит:
  // видно, какой логин и пароль ушли, и не нужно жать «Войти»
  const pick = (who: string, secret: string) => {
    setLogin(who);
    setPassword(secret);
    void enter(who, secret);
  };

  return (
    <div className={s.page}>
      <div className={s.art} aria-hidden>
        <div className={s.sky} />
        <div className={s.city}>
          {Array.from({ length: 14 }, (_, i) => (
            <span key={i} className={s.tower} style={{ ['--i' as string]: i }} />
          ))}
        </div>
      </div>

      <DemoAccess onPick={pick} busy={busy} />

      <form className={s.form} onSubmit={(e) => void submit(e)}>
        <h1 className={s.title}>
          <span className={s.logo}>112</span>
          <span className={s.subtitle}>ВХОД В СИСТЕМУ</span>
        </h1>

        <label className={s.field}>
          <span>логин:</span>
          <input value={login} onChange={(e) => setLogin(e.target.value)}
                 name="login" autoComplete="username" />
        </label>

        <label className={s.field}>
          <span>пароль:</span>
          <input type="password" value={password} name="password"
                 autoComplete="current-password"
                 onChange={(e) => setPassword(e.target.value)} />
        </label>

        <label className={s.field}>
          <span>номер АРМ:</span>
          <input value={workstation} onChange={(e) => setWorkstation(e.target.value)}
                 name="workstation" inputMode="numeric" />
        </label>

        {error && <div className={s.error} role="alert" data-testid="login-error">{error}</div>}

        <button className={s.submit} type="submit" disabled={busy}
                data-testid="login-submit">{busy ? '…' : 'ВОЙТИ'}</button>

        <div className={s.support}>
          <div className={s.supportTitle}>Техподдержка</div>
          <div>+7 (495) 197-89-81 (многоканальный)</div>
          <a className={s.mail} href="mailto:hd-112@mos.ru">hd-112@mos.ru</a>
        </div>
      </form>
    </div>
  );
};
