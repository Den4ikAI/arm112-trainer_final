import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTraining } from '@/entities/incident/model';
import { useCatalog } from '@/entities/catalog/model';
import type { Finding } from '@/shared/api';
import { fmt } from '@/shared/lib/useTimer';
import { scenarioTitle } from '@/shared/lib/scenarioTitle';
import clsx from '@/shared/lib/clsx';
import { MyForecast } from '@/widgets/forecast/MyForecast';
import { MyAdvice } from '@/widgets/advice/MyAdvice';
import s from './DebriefPage.module.scss';

const KIND_LABEL: Record<Finding['kind'], string> = {
  ok: 'верно',
  guessed: 'угадал',
  wrong: 'ошибка',
  missing: 'не заполнено',
  extra: 'лишнее',
};

/**
 * Разбор занятия. Главное отличие от обычного тренажёра — колонка «угадал»:
 * значение совпало с эталоном, но заявитель его не сообщал.
 */
export const DebriefPage = () => {
  const { grade: result, scenario, transcript, revealed, reset, sessionId, resume } = useTraining();
  const serviceTitle = useCatalog((st) => st.serviceTitle);
  const nav = useNavigate();

  // После обновления страницы разбор берётся с сервера: он посчитан
  // один раз при сохранении и там и лежит
  const [resuming, setResuming] = useState(!result);
  useEffect(() => {
    if (result) return;
    let alive = true;
    void resume().finally(() => { if (alive) setResuming(false); });
    return () => { alive = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if ((!result || !scenario) && resuming) {
    return (
      <div className={s.page}>
        <p className={s.empty}>Загружаю разбор…</p>
      </div>
    );
  }

  if (!result || !scenario) {
    return (
      <div className={s.page}>
        <p className={s.empty}>Нет данных разбора. Запустите учебный вызов из журнала.</p>
        <button className={s.again} onClick={() => nav('/journal')}>В журнал</button>
      </div>
    );
  }

  const guessed = result.findings.filter((f) => f.kind === 'guessed');
  const problems = result.findings.filter((f) => f.kind !== 'ok' && f.kind !== 'guessed');
  const ok = result.findings.filter((f) => f.kind === 'ok');

  return (
    <div className={s.page}>
      <header className={s.head}>
        <div>
          <h1 className={s.title}>Разбор вызова</h1>
          <div className={s.sub}>{scenarioTitle(scenario)}</div>
        </div>
        <div className={s.scoreBox} data-testid="score">
          <div className={s.score}>{result.score}</div>
          <div className={s.scoreCap}>баллов</div>
        </div>
      </header>

      {sessionId && <MyForecast sessionId={sessionId} actual={result.score} />}
      {sessionId && <MyAdvice sessionId={sessionId} />}

      <div className={s.metrics}>
        <Metric label="Время заполнения" value={fmt(result.elapsedSec)}
                danger={result.overNorm} note={`норматив ${fmt(result.normSec)}`} />
        {/* Единица — вопрос заявителю, а не поле карточки. Раньше из
            числа вопросов вычиталось число угаданных полей, а один
            вопрос раскрывает несколько полей — на стенде вышло «−1 / 5» */}
        <Metric label="Задано вопросов заявителю"
                value={`${revealed.filter((id) => scenario.questions.some((q) => q.factId === id)).length} / ${scenario.questions.length}`}
                note={guessed.length > 0 ? `угадано полей: ${guessed.length}` : undefined} />
        {/* Направлено из требуемых и отдельно лишние. Было «оповещены
            6 / 5»: лишняя служба смешивалась с нужными, а «оповещены»
            выдавало выбор службы за подтверждённый приём — его
            ставит диспетчер ДДС, а не курсант 112 */}
        <Metric label="Требуемые службы направлены"
                value={`${result.services.expected.length - result.services.missed.length} / ${result.services.expected.length}`}
                note={result.services.extra.length > 0
                  ? `лишних: ${result.services.extra.length}` : 'лишних нет'}
                danger={result.services.missed.length > 0} />
      </div>

      {guessed.length > 0 && (
        <section className={s.section}>
          <h2 className={s.sectionTitle}>Угадал, а не выяснил</h2>
          <p className={s.sectionNote}>
            Значения верные, но заявитель их не сообщал. В боевых условиях
            это предположение, а не факт.
          </p>
          <ul className={s.list}>
            {guessed.map((f, i) => (
              <li key={i} className={clsx(s.item, s.guessed)} data-testid="finding-guessed">
                <span className={s.badge}>{KIND_LABEL[f.kind]}</span>
                <span className={s.field}>{f.label}</span>
                <span className={s.value}>{f.actual}</span>
                <span className={s.consequence}>{f.consequence}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className={s.section}>
        <h2 className={s.sectionTitle}>Замечания</h2>
        {problems.length === 0
          ? <p className={s.sectionNote}>Замечаний нет.</p>
          : (
            <ul className={s.list}>
              {problems.map((f, i) => (
                <li key={i} className={clsx(s.item, s[f.kind])}>
                  <span className={s.badge}>{KIND_LABEL[f.kind]}</span>
                  <span className={s.field}>
                    {f.field.startsWith('service.')
                      ? serviceTitle(f.field.slice(8))
                      : f.label}
                  </span>
                  <span className={s.value}>
                    {f.actual && <s>{f.actual}</s>} {f.expected && <b>{f.expected}</b>}
                  </span>
                  <span className={s.consequence}>{f.consequence}</span>
                </li>
              ))}
            </ul>
          )}
      </section>

      <section className={s.section}>
        <h2 className={s.sectionTitle}>Заполнено верно ({ok.length})</h2>
        <ul className={s.listCompact}>
          {ok.map((f, i) => <li key={i}>{f.label}: <b>{f.actual}</b></li>)}
        </ul>
      </section>

      <section className={s.section}>
        <h2 className={s.sectionTitle}>Стенограмма</h2>
        <ol className={s.transcript}>
          {transcript.map((l, i) => (
            <li key={i}><span className={s.who}>{l.who}</span> {l.text}</li>
          ))}
        </ol>
      </section>

      <button className={s.again} onClick={() => { reset(); nav('/journal'); }}>
        Вернуться в журнал
      </button>
    </div>
  );
};

const Metric = ({ label, value, note, danger }: {
  label: string; value: string; note?: string; danger?: boolean;
}) => (
  <div className={clsx(s.metric, danger && s.metricDanger)}>
    <div className={s.metricLabel}>{label}</div>
    <div className={s.metricValue}>{value}</div>
    {note && <div className={s.metricNote}>{note}</div>}
  </div>
);
