import { useState } from 'react';
import type { ScenarioPublic } from '@/shared/api';
import clsx from '@/shared/lib/clsx';
import { fmt } from '@/shared/lib/useTimer';
import s from './TrainerHud.module.scss';

interface Props {
  scenario: ScenarioPublic;
  revealed: string[];
  transcript: Array<{ who: string; text: string; atSec: number }>;
  elapsedSec: number;
  onAsk: (factId: string) => void;
}

/**
 * Панель обучающегося — единственный элемент, которого НЕТ в боевой системе.
 *
 * Здесь реализован ключевой приём тренажёра: заявитель не выкладывает всё сразу.
 * Часть фактов он сообщает только в ответ на конкретный вопрос — ровно так,
 * как это заложено в бумажных билетах, где настоящий адрес напечатан в скобках,
 * а заявитель называет лишь ориентир. Поле, заполненное без вопроса,
 * при разборе будет помечено как «угадал», а не «выяснил».
 */
export const TrainerHud = ({ scenario, revealed, transcript, elapsedSec, onAsk }: Props) => {
  const [collapsed, setCollapsed] = useState(false);
  const over = elapsedSec > scenario.normSec;
  const asked = revealed.length;
  const total = scenario.questions.length;

  return (
    <aside className={clsx(s.hud, collapsed && s.collapsed)} aria-label="Панель обучающегося">
      <header className={s.head}>
        <div>
          <div className={s.badge}>Билет {scenario.ticket} · вызов {scenario.callNo}</div>
          <div className={s.sub}>{scenario.applicantName}, {scenario.applicantPhone}</div>
        </div>
        <button className={s.toggle} onClick={() => setCollapsed((v) => !v)}
                aria-expanded={!collapsed}>
          {collapsed ? '▸' : '▾'}
        </button>
      </header>

      {!collapsed && (
        <div className={s.body}>
          <div className={clsx(s.norm, over && s.normOver)}>
            <span>Норматив {fmt(scenario.normSec)}</span>
            <strong data-testid="hud-elapsed">{fmt(elapsedSec)}</strong>
          </div>

          <div className={s.sectionTitle}>Разговор</div>
          <ol className={s.transcript} data-testid="transcript">
            {transcript.map((line, i) => (
              <li key={i} className={clsx(s.line, line.who === 'оператор' && s.mine)}>
                <span className={s.who}>{line.who}</span>
                <span className={s.text}>{line.text}</span>
              </li>
            ))}
          </ol>

          <div className={s.sectionTitle}>
            Задать вопрос <span className={s.counter}>{asked} / {total}</span>
          </div>
          <div className={s.questions}>
            {scenario.questions.map((q) => (
              <button
                key={q.factId}
                className={clsx(s.question, revealed.includes(q.factId) && s.asked)}
                disabled={revealed.includes(q.factId)}
                data-testid={`ask-${q.key}`}
                onClick={() => onAsk(q.factId)}
              >
                {q.question}
              </button>
            ))}
          </div>
        </div>
      )}
    </aside>
  );
};
