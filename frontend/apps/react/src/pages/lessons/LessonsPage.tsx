import { useCallback, useEffect, useState } from 'react';
import {
  ApiError, lessons, reports, scenarios as scenariosApi, SEGMENT_LABEL,
  type BlitzSegment, type GroupBrief, type LessonBrief, type LessonFull, type LessonKind,
  type Protocol, type ScenarioBrief,
} from '@/shared/api';
import { fmt } from '@/shared/lib/useTimer';
import { scenarioTitle } from '@/shared/lib/scenarioTitle';
import { useCatalog } from '@/entities/catalog/model';
import { useSession } from '@/entities/session/model';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import { GroupMembers } from '@/widgets/group-members/GroupMembers';
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
  // Итоговый протокол марафона и блица
  const [protocol, setProtocol] = useState<Protocol | null>(null);
  // Утверждённые вызовы — из них преподаватель собирает марафон и блиц
  const [approved, setApproved] = useState<ScenarioBrief[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);

  const types = useCatalog((st) => st.types);
  const loadCatalog = useCatalog((st) => st.load);
  const user = useSession((st) => st.user);

  // Черновик нового занятия
  const [form, setForm] = useState({
    groupId: '', title: '', mode: 'card112' as 'card112' | 'dds',
    typeIds: [] as string[],
    // Режим ДДС: карточки приходят потоком (ответ заказчика 23.09), темп
    // заказчик не задал. 60 секунд — наше допущение, см. PROVENANCE.md
    cardIntervalSec: 60,
    // Марафон — экзамен на 5–10 вызовов подряд; блиц — серия заданий
    // на один сегмент карточки
    kind: 'training' as LessonKind,
    segment: 'address' as BlitzSegment,
    callCount: 5,
    // Выбранные поимённо вызовы, в порядке выбора. Пусто — подберёт
    // система по категориям
    scenarioIds: [] as string[],
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
        const все = await scenariosApi.list().catch(() => [] as ScenarioBrief[]);
        setApproved(все.filter((sc) => sc.status === 'approved'));
        setForm((f) => ({ ...f, groupId: f.groupId || (гр[0]?.id ?? '') }));
      } catch (e) {
        setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Нет связи с сервером');
      }
    })();
  }, [loadCatalog]);

  // Протокол — у марафона и блица приёма вызова: там есть вызовы
  // и баллы по ним. Обновляем вместе с ходом занятия
  useEffect(() => {
    if (!current || current.kind === 'training' || current.mode !== 'card112') return;
    let alive = true;
    void lessons.protocol(current.id)
      .then((p) => { if (alive) setProtocol(p); })
      .catch(() => { if (alive) setProtocol(null); });
    return () => { alive = false; };
  }, [current]);

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
      const упражнение = form.kind !== 'training';
      const сегмент: BlitzSegment | undefined = form.kind !== 'blitz'
        ? undefined
        : form.mode === 'dds' ? 'statuses' : form.segment;
      const созданное = await lessons.create({
        groupId: form.groupId, title: form.title.trim(),
        mode: form.mode, typeIds: form.typeIds,
        ...(form.mode === 'dds' ? { cardIntervalSec: form.cardIntervalSec } : {}),
        kind: form.kind,
        ...(сегмент ? { segment: сегмент } : {}),
        ...(упражнение && form.mode === 'card112'
          ? (form.scenarioIds.length > 0
            ? { scenarioIds: form.scenarioIds }
            : { callCount: form.callCount })
          : {}),
      });
      setForm((f) => ({ ...f, title: '', scenarioIds: [] }));
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

  const выбрать_вызов = (id: string) =>
    setForm((f) => ({
      ...f,
      scenarioIds: f.scenarioIds.includes(id)
        ? f.scenarioIds.filter((v) => v !== id)
        : [...f.scenarioIds, id],
    }));

  const ВИД: Record<LessonKind, string> = {
    training: 'обычное занятие',
    marathon: 'марафон',
    blitz: 'блиц',
  };

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

            <label className={s.field}>
              <span>Вид</span>
              <select value={form.kind} aria-label="Вид занятия"
                      data-testid="lesson-kind"
                      onChange={(e) => setForm({ ...form, kind: e.target.value as LessonKind })}>
                <option value="training">Обычное занятие</option>
                {form.mode === 'card112' && (
                  <option value="marathon">Марафон — экзамен на 5–10 вызовов подряд</option>
                )}
                <option value="blitz">Блиц — серия заданий на один сегмент</option>
              </select>
            </label>

            {form.kind === 'blitz' && form.mode === 'card112' && (
              <label className={s.field}>
                <span>Сегмент карточки</span>
                <select value={form.segment} aria-label="Сегмент блица"
                        data-testid="lesson-segment"
                        onChange={(e) => setForm({ ...form, segment: e.target.value as BlitzSegment })}>
                  {(['address', 'type', 'services'] as const).map((sg) => (
                    <option key={sg} value={sg}>{SEGMENT_LABEL[sg]}</option>
                  ))}
                </select>
              </label>
            )}
            {form.kind === 'blitz' && form.mode === 'dds' && (
              <p className={s.hint}>Блиц ДДС оценивает статусы по докладам бригады.</p>
            )}

            {form.kind !== 'training' && form.mode === 'card112' && (
              <>
                <label className={s.field}>
                  <span>Вызовов{form.scenarioIds.length > 0 ? ' (выбраны ниже)' : ''}</span>
                  <input type="number" aria-label="Число вызовов" data-testid="lesson-count"
                         min={form.kind === 'marathon' ? 5 : 3} max={10}
                         disabled={form.scenarioIds.length > 0}
                         value={form.scenarioIds.length || form.callCount}
                         onChange={(e) => setForm({ ...form, callCount: Number(e.target.value) })} />
                </label>
                <fieldset className={s.types}>
                  <legend>Вызовы по порядку (необязательно)</legend>
                  {approved.length === 0 && <p className={s.empty}>Утверждённых вызовов нет</p>}
                  {approved.map((sc) => {
                    const номер = form.scenarioIds.indexOf(sc.id);
                    return (
                      <label key={sc.id} className={s.type}>
                        <input type="checkbox" checked={номер >= 0}
                               data-testid={`lesson-pick-${sc.id}`}
                               onChange={() => выбрать_вызов(sc.id)} />
                        <span>{номер >= 0 ? `${номер + 1}. ` : ''}{scenarioTitle(sc)}</span>
                      </label>
                    );
                  })}
                  <p className={s.hint}>
                    {form.scenarioIds.length === 0
                      ? 'Не выбрано — система подберёт по категориям'
                      : `Выбрано: ${form.scenarioIds.length}`}
                  </p>
                </fieldset>
              </>
            )}

            {form.mode === 'dds' && (
              <label className={s.field}>
                <span>Новая карточка каждой службе — раз в, секунд</span>
                <input type="number" min={10} max={900} step={10}
                       value={form.cardIntervalSec} aria-label="Интервал карточек"
                       data-testid="lesson-interval"
                       onChange={(e) => setForm({
                         ...form, cardIntervalSec: Number(e.target.value) })} />
              </label>
            )}

            <fieldset className={s.types}>
              {/* ТЗ: «выбор категории событий исходя из темы занятий
                  (возможен множественный выбор)». Ничего не выбрано —
                  берутся любые */}
              <legend>Категории происшествий</legend>
              {types.length === 0 && <p className={s.empty}>Справочник не загрузился</p>}
              {types.filter((t) => t.listed || form.typeIds.includes(t.id)).map((t) => (
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

          <GroupMembers groupId={form.groupId} mode={form.mode} />

          <div className={s.listBox}>
          <h2 className={s.subtitle}>Список</h2>
          {list.length === 0 && (
            <p className={s.empty}>
              Занятий пока нет. Соберите первое формой выше.
            </p>
          )}
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
                    {l.groupTitle} · {l.kind !== 'training' && `${ВИД[l.kind]} · `}{состояние(l)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
          </div>
        </section>

        <section className={s.right} aria-label="Ход занятия">
          {!current && (
            <div className={s.card}>
              <p className={s.empty}>
                Выберите занятие слева — здесь будет виден его ход:
                кто сколько вызовов взял и с каким баллом.
              </p>
            </div>
          )}

          {current && (
            <>
              <div className={s.card}>
                <h2 className={s.subtitle} data-testid="lesson-current">{current.title}</h2>
                <dl className={s.facts}>
                  <div><dt>Группа</dt><dd>{current.groupTitle}</dd></div>
                  <div><dt>Режим</dt><dd>
                    {current.mode === 'card112' ? 'приём вызова' : 'статусы ДДС'}
                  </dd></div>
                  <div><dt>Вид</dt><dd data-testid="lesson-kind-value">
                    {ВИД[current.kind]}
                    {current.segment && ` · ${SEGMENT_LABEL[current.segment]}`}
                  </dd></div>
                  <div><dt>Состояние</dt>
                       <dd data-testid="lesson-state">{состояние(current)}</dd></div>
                  <div><dt>Курсантов</dt><dd>{current.members}</dd></div>
                  <div><dt>{current.mode === 'dds' ? 'Карточек в наборе' : 'Роздано вызовов'}</dt>
                       <dd>{current.items}</dd></div>
                  {current.mode === 'dds' && (
                    <div><dt>Поступление</dt>
                         <dd>службе — по карточке раз в {current.cardIntervalSec} с</dd></div>
                  )}
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
                      {/* В режиме ДДС разбора нет: курсант берёт карточки
                          и доводит работу своей службы до конца */}
                      {current.mode === 'dds' ? (
                        <tr><th>Курсант</th><th>Взял в работу</th>
                            <th>Довёл до конца</th><th>Средний балл</th></tr>
                      ) : (
                        <tr><th>Курсант</th><th>Взял вызовов</th>
                            <th>Разобрано</th><th>Средний балл</th></tr>
                      )}
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

              {protocol && protocol.lessonId === current.id && current.kind !== 'training' && (
                <div className={s.card}>
                  <h2 className={s.subtitle}>
                    Протокол {current.kind === 'marathon' ? 'марафона' : 'блица'}
                    {protocol.segmentLabel && ` · ${protocol.segmentLabel}`}
                  </h2>
                  <p className={s.hint}>
                    {current.kind === 'marathon'
                      ? 'Курсант видит свои баллы только после последнего вызова. Грубые ошибки — неверное значение или пропущенная служба.'
                      : 'Балл — только по сегменту; время не оценивается.'}
                  </p>
                  <table className={s.table} data-testid="lesson-protocol">
                    <thead>
                      <tr><th>Курсант</th><th>Пройдено</th><th>Средний балл</th>
                          <th>Время</th><th>Грубых</th><th>По вызовам</th></tr>
                    </thead>
                    <tbody>
                      {protocol.rows.map((r) => (
                        <tr key={r.userId}>
                          <td>{r.fio}</td>
                          <td>{r.done} / {r.total}</td>
                          <td>{r.avgScore == null ? '—' : Math.round(r.avgScore)}</td>
                          <td>{r.done > 0 ? fmt(r.totalSec) : '—'}</td>
                          <td>{r.grossCount}</td>
                          <td>
                            {r.calls.map((c) => {
                              const подпись = (
                                <>
                                  {c.seq}: {c.finished ? (c.score ?? '—') : '·'}
                                  {c.expertScore != null && ` (эксп. ${c.expertScore})`}
                                </>
                              );
                              const подсказка = c.gross.length ? `Грубые: ${c.gross.join(', ')}` : c.legend;
                              // Сохранённая карточка открывается с разбором,
                              // анализом текста и экспертной оценкой
                              return c.finished && c.cardId
                                ? <a key={c.seq} className={s.callScore} title={подсказка}
                                     href={`#/card-dds/${c.cardId}`}>{подпись}</a>
                                : <span key={c.seq} className={s.callScore} title={подсказка}>{подпись}</span>;
                            })}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          )}
        </section>
      </div>
    </div>
  );
};
