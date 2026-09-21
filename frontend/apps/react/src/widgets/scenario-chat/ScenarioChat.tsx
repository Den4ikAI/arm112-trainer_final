import { useEffect, useRef, useState } from 'react';
import { ApiError, scenarios, type ChatMessage, type ScenarioFull } from '@/shared/api';
import clsx from '@/shared/lib/clsx';

/**
 * Человеческие названия полей вызова.
 *
 * Преподаватель читал «изменено: legend, normSec» и не мог понять,
 * что именно поправила модель.
 */
const ПОЛЯ: Record<string, string> = {
  legend: 'первая фраза',
  applicantName: 'ФИО заявителя',
  applicantStatus: 'статус заявителя',
  normSec: 'норматив',
  hidden: 'вопросы заявителю',
  answers: 'признаки',
  outcome: 'тип происшествия',
};

const подпись = (ключ: string) => ПОЛЯ[ключ] ?? ключ;
import s from './ScenarioChat.module.scss';

/**
 * Окно переписки с моделью о вызове.
 *
 * Требование ТЗ: «в случае некорректного формирования вопроса или
 * ответа у Преподавателя имеется контекстное поле, куда он может
 * ввести комментарий, который должен быть отработан системой».
 *
 * Именно переписка, а не одно поле: правка редко получается с первого
 * раза, и на «нет, короче» модель должна понимать, о чём речь.
 *
 * Правки применяются сразу, но сценарий рядом обновляется на глазах —
 * преподаватель видит результат и может поправить следующей репликой
 * или откатить руками.
 */
export const ScenarioChat = ({
  scenario,
  onChanged,
}: {
  scenario: ScenarioFull;
  onChanged: (next: ScenarioFull) => void;
}) => {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const tail = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void scenarios.chat(scenario.id).then(setMessages).catch(() => setError('Переписка недоступна'));
  }, [scenario.id]);

  // Держим последнюю реплику на виду
  useEffect(() => {
    tail.current?.scrollIntoView({ block: 'end' });
  }, [messages.length, busy]);

  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    const comment = draft.trim();
    if (!comment || busy) return;

    setBusy(true);
    setError(null);
    // Свою реплику показываем сразу: ответа ждать секунды, и пустое
    // поле без неё выглядит так, будто ничего не отправилось
    setMessages((m) => [...m, {
      id: -Date.now(), role: 'teacher', content: comment,
      patch: null, at: new Date().toISOString(),
    }]);
    setDraft('');

    try {
      const r = await scenarios.say(scenario.id, comment);
      onChanged(r.scenario);
      setMessages(await scenarios.chat(scenario.id));
    } catch (err) {
      setError(err instanceof ApiError ? (err.detail ?? err.message) : 'Модель не ответила');
      setMessages(await scenarios.chat(scenario.id).catch(() => []));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className={s.wrap} aria-label="Правка вызова">
      <header className={s.head}>
        <h2 className={s.title}>Правка в диалоге</h2>
        <p className={s.hint}>
          Напишите, что не так. Модель поправит текст вызова; тип
          происшествия и список служб пересчитает классификатор.
        </p>
      </header>

      <div className={s.log} data-testid="chat-log">
        {messages.length === 0 && (
          <p className={s.empty}>
            Замечаний пока нет. Например: «заявитель говорит слишком складно,
            он ведь в панике» или «убери вопрос про газ, его тут быть не может».
          </p>
        )}

        {messages.map((m) => (
          <div key={m.id} className={clsx(s.msg, s[m.role])} data-testid={`msg-${m.role}`}>
            <span className={s.who}>
              {m.role === 'teacher' ? 'вы' : m.role === 'model' ? 'модель' : 'система'}
            </span>
            <span className={s.text}>{m.content}</span>
            {m.patch && Object.keys(m.patch).length > 0 && (
              <span className={s.changed}>
                изменено: {Object.keys(m.patch).map(подпись).join(', ')}
              </span>
            )}
          </div>
        ))}

        {busy && <div className={clsx(s.msg, s.model, s.pending)}>модель думает…</div>}
        <div ref={tail} />
      </div>

      {error && <div className={s.error} role="alert">{error}</div>}

      <form className={s.form} onSubmit={(e) => void send(e)}>
        <textarea
          className={s.input}
          value={draft}
          rows={2}
          placeholder="Что поправить?"
          aria-label="Замечание к вызову"
          data-testid="chat-input"
          disabled={busy}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            // Enter отправляет, Shift+Enter переносит строку —
            // как в любом мессенджере
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              void send(e as unknown as React.FormEvent);
            }
          }}
        />
        <button className={s.send} type="submit" disabled={busy || !draft.trim()}
                data-testid="chat-send">
          {busy ? '…' : 'Отправить'}
        </button>
      </form>
    </section>
  );
};
