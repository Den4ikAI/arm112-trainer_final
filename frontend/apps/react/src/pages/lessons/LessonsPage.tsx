import { useCallback, useEffect, useState } from 'react';
import {
  ApiError, lessons, reports,
  type GroupBrief, type LessonBrief, type LessonFull,
} from '@/shared/api';
import { useCatalog } from '@/entities/catalog/model';
import { useSession } from '@/entities/session/model';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import clsx from '@/shared/lib/clsx';
import s from './LessonsPage.module.scss';

/**
 * Занятие, которым управляет преподаватель.
 *
 * Порядок из ТЗ §10, оба сценария «Проведение практического занятия»:
 * преподаватель выбирает категории происшествий под тему, запускает
 * обучение, следит за ходом и завершает в любой момент.
 *
 * Осторожно с терминами: в ТЗ «занятие» — это то, что ведёт
 * преподаватель. Один отработанный курсантом вызов у нас называется
 * `session` и в интерфейсе зовётся вызовом.
 */
export const LessonsPage = () => {
  const [list, setList] = useState<LessonBrief[]>([]);
  const [groups, setGroups] = useState<GroupBrief[]>([]);
  const [current, setCurrent] = useState<LessonFull | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);

  const types = useCatalog((st) => st.types);
  const loadCatalog = useCatalog((st) => st.load);
  const user = useSession((st) => st.user);

  // Черновик нового занятия
  const [form, setForm] = useState({
    groupId: '', title: '', mode: 'card112' as 'card112' | 'dds',
    typeIds: [] as string[],
  });

  const reload = useCallback(async (pick?: string) => {
    const items = await lessons.list();
    setList(items);
    const id = pick ?? current?.id;
    if (id && items.some((l) => l.id === id)) setCurrent(await lessons.one(id));
    else if (!items.length) setCurrent(null);
  }, [current?.id]);

  useEffect(() => {
    void loadCatalog().catch(() => {});
    void (async () => {
      try {
        const [гр, зан] = await Promise.all([reports.groups(), lessons.list()]);
        setGroups(гр);
        setList(зан);
        setForm((f) => ({ ...f, groupId: f.groupId || (гр[0]?.id ?? '') }));
      } catch (e) {
        setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Нет связи с сервером');
      }
    })();
  }, [loadCatalog]);

  const открыть = async (id: string) => {
    setError(null);
    try {
      setCurrent(await lessons.one(id));
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Занятие не открылось');
    }
  };

  const действие = async (что: () => Promise<unknown>, подпись: string) => {
    setWorking(true);
    setError(null);
    try {
      await что();
      await reload();
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : подпись);
    } finally {
      setWorking(false);
    }
  };

  const собрать = () =>
    действие(async () => {
      const созданное = await lessons.create({
        groupId: form.groupId, title: form.title.trim(),
        mode: form.mode, typeIds: form.typeIds,
      });
      setForm((f) => ({ ...f, title: '' }));
      await reload(созданное.id);
    }, 'Занятие не собралось');

  // Сервер курсанта сюда не пустит, но и рисовать ему форму незачем:
  // кнопка, которая заведомо ответит отказом, — это не интерфейс
  if (!user || user.role === 'student') {
    return (
      <div className={s.page}>
        <p className={s.empty}>Раздел доступен преподавателю и администратору.</p>
      </div>
    );
  }

  const идёт = (l: LessonBrief) => !!l.startedAt && !l.finishedAt;
  const состояние = (l: LessonBrief) =>
    l.finishedAt ? 'завершено' : l.startedAt ? 'идёт' : 'собрано';

  const переключить = (id: string) =>
    setForm((f) => ({
      ...f,
      typeIds: f.typeIds.includes(id)
        ? f.typeIds.filter((t) => t !== id)
        : [...f.typeIds, id],
    }));

  return (
    <div className={s.page}>
      <header className={s.head}>
        <h1 className={s.title}>Занятия</h1>
        <UserMenu />
      </header>

      {error && <div className={s.error} role="alert">{error}</div>}

      <div className={s.cols}>
        <section className={s.left} aria-label="Занятия">
          <form className={s.new} onSubmit={(e) => { e.preventDefault(); void собрать(); }}>
            <h2 className={s.subtitle}>Собрать занятие</h2>

            <label className={s.field}>
              <span>Группа</span>
              <select value={form.groupId} required aria-label="Группа"
                      data-testid="lesson-group"
                      onChange={(e) => setForm({ ...form, groupId: e.target.value })}>
                {groups.map((g) => (
                  <option key={g.id} value={g.id}>{g.title}</option>
                ))}
              </select>
            </label>

            <label className={s.field}>
              <span>Тема занятия</span>
              <input value={form.title} required aria-label="Тема занятия"
                     placeholder="Пожары в жилом секторе"
                     data-testid="lesson-title"
                     onChange={(e) => setForm({ ...form, title: e.target.value })} />
            </label>

            <label className={s.field}>
              <span>Режим</span>
              <select value={form.mode} aria-label="Режим занятия"
                      data-testid="lesson-mode"
                      onChange={(e) => setForm({
                        ...form, mode: e.target.value as 'card112' | 'dds' })}>
                <option value="card112">Приём вызова (карточка 112)</option>
                <option value="dds">Работа со статусами (ДДС)</option>
              </select>
            </label>

            <fieldset className={s.types}>
              {/* ТЗ: «выбор категории событий исходя из темы занятий
                  (возможен множественный выбор)». Ничего не выбрано —
                  берутся любые */}
              <legend>Категории происшествий</legend>
              {types.length === 0 && <p className={s.empty}>Справочник не загрузился</p>}
              {types.map((t) => (
                <label key={t.id} className={s.type}>
                  <input type="checkbox" checked={form.typeIds.includes(t.id)}
                         onChange={() => переключить(t.id)} />
                  <span>{t.title}</span>
                </label>
              ))}
              <p className={s.hint}>
                {form.typeIds.length === 0
                  ? 'Не выбрано ничего — подойдут вызовы любых категорий'
                  : `Выбрано категорий: ${form.typeIds.length}`}
              </p>
            </fieldset>

            <button className={s.primary} type="submit"
                    disabled={working || !form.title.trim() || !form.groupId}
                    data-testid="lesson-create">
              Собрать
            </button>
          </form>

          <h2 className={s.subtitle}>Список</h2>
          {list.length === 0 && <p className={s.empty}>Занятий пока нет</p>}
          <ul className={s.list}>
            {list.map((l) => (
              <li key={l.id}>
                <button
                  className={clsx(s.item, current?.id === l.id && s.active,
                                  идёт(l) && s.running)}
                  data-testid={`lesson-${l.id}`}
                  onClick={() => void открыть(l.id)}>
                  <span className={s.itemTitle}>{l.title}</span>
                  <span className={s.itemMeta}>
                    {l.groupTitle} · {состояние(l)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </section>

        <section className={s.right} aria-label="Ход занятия">
          {!current && <p className={s.empty}>Выберите занятие слева</p>}

          {current && (
            <>
              <div className={s.card}>
                <h2 className={s.subtitle} data-testid="lesson-current">{current.title}</h2>
                <dl className={s.facts}>
                  <div><dt>Группа</dt><dd>{current.groupTitle}</dd></div>
                  <div><dt>Режим</dt><dd>
                    {current.mode === 'card112' ? 'приём вызова' : 'статусы ДДС'}
                  </dd></div>
                  <div><dt>Состояние</dt>
                       <dd data-testid="lesson-state">{состояние(current)}</dd></div>
                  <div><dt>Курсантов</dt><dd>{current.members}</dd></div>
                  <div><dt>Роздано</dt><dd>{current.items}</dd></div>
                </dl>

                <div className={s.actions}>
                  {!current.startedAt && (
                    <button className={s.primary} disabled={working}
                            data-testid="lesson-start"
                            onClick={() => void действие(
                              () => lessons.start(current.id), 'Занятие не запустилось')}>
                      Начать обучение
                    </button>
                  )}
                  {идёт(current) && (
                    <button className={s.danger} disabled={working}
                            data-testid="lesson-finish"
                            onClick={() => void действие(
                              () => lessons.finish(current.id), 'Занятие не завершилось')}>
                      Завершить занятие
                    </button>
                  )}
                  {!current.startedAt && (
                    <button className={s.ghost} disabled={working}
                            data-testid="lesson-remove"
                            onClick={() => void действие(async () => {
                              await lessons.remove(current.id);
                              setCurrent(null);
                            }, 'Занятие не удалилось')}>
                      Удалить
                    </button>
                  )}
                </div>
              </div>

              <div className={s.card}>
                <h2 className={s.subtitle}>Ход занятия</h2>
                {current.progress.length === 0 && (
                  <p className={s.empty}>В группе нет курсантов</p>
                )}
                {current.progress.length > 0 && (
                  <table className={s.table} data-testid="lesson-progress">
                    <thead>
                      <tr><th>Курсант</th><th>Взято</th><th>Разобрано</th><th>Средний балл</th></tr>
                    </thead>
                    <tbody>
                      {current.progress.map((p) => (
                        <tr key={p.userId}>
                          <td>{p.fio}</td>
                          <td>{p.started}</td>
                          <td>{p.graded}</td>
                          <td>{p.avgScore === null ? '—' : Math.round(p.avgScore)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
            </>
          )}
        </section>
      </div>
    </div>
  );
};
