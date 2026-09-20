import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useSession } from '@/entities/session/model';
import s from './LoginPage.module.scss';

/** Экран входа. Композиция повторяет боевой: иллюстрация города и форма справа. */
export const LoginPage = () => {
  const [login, setLogin] = useState('student1');
  const [password, setPassword] = useState('');
  const [workstation, setWorkstation] = useState('4');
  const [busy, setBusy] = useState(false);
  const signIn = useSession((st) => st.signIn);
  const error = useSession((st) => st.error);
  const nav = useNavigate();

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    const ok = await signIn(login, password, workstation);
    setBusy(false);
    if (ok) nav('/journal');
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
