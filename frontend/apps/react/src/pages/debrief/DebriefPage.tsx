import { useNavigate } from 'react-router-dom';
import { getService, type Finding } from '@arm112/domain';
import { useTraining } from '@/entities/incident/model';
import { fmt } from '@/shared/lib/useTimer';
import clsx from '@/shared/lib/clsx';
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
  const { result, scenario, transcript, reset } = useTraining();
  const nav = useNavigate();

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
          <div className={s.sub}>Билет {scenario.ticket} · вызов {scenario.callNo}</div>
        </div>
        <div className={s.scoreBox} data-testid="score">
          <div className={s.score}>{result.score}</div>
          <div className={s.scoreCap}>баллов</div>
        </div>
      </header>

      <div className={s.metrics}>
        <Metric label="Время заполнения" value={fmt(result.elapsedSec)}
                danger={result.overNorm} note={`норматив ${fmt(result.normSec)}`} />
        <Metric label="Выяснено у заявителя"
                value={`${scenario.hidden.length - guessed.length} / ${scenario.hidden.length}`} />
        <Metric label="Службы оповещены"
                value={`${result.services.actual.length} / ${result.services.expected.length}`}
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
                      ? getService(f.field.slice(8))?.title ?? f.label
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
