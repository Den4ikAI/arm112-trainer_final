import { useEffect, useRef, useState } from 'react';
import { chief, type ChiefReport as Report } from '@/shared/api';
import { ChiefReviewList } from '@/entities/chief/ChiefReviewList';
import { Recorder, speak, transcribe, voiceUnavailable } from '@/shared/lib/voiceBridge';
import { micBlockedReason } from '@/shared/lib/voice';
import s from './ChiefReport.module.scss';

const POLL_MS = 5_000;

/**
 * Доклад вышестоящему начальнику по карточке своей службы.
 *
 * Заказчик (чат задачи, 27.09): голосовое общение «ДДС с вышестоящим
 * начальником и ДДС с руководителем бригады». Диспетчер докладывает,
 * начальник переспрашивает о недоложенном и говорит «Принял». Говорить
 * можно голосом или набирать текстом: без сервера голоса звонок идёт
 * текстом целиком.
 */
/** `closed` — служба закончила работу («Работы завершены» или «Отказ»):
 *  начатый разговор договорить можно, новый звонок сервер отклоняет. */
export const ChiefReport = ({ cardId, closed = false }: { cardId: string; closed?: boolean }) => {
  const [report, setReport] = useState<Report | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const recorder = useRef<Recorder | null>(null);
  const spoken = useRef(0);

  useEffect(() => {
    let alive = true;
    void chief.get(cardId)
      .then((r) => { if (alive) { setReport(r); spoken.current = r?.lines.length ?? 0; } })
      .catch(() => {})
      .finally(() => { if (alive) setLoaded(true); });
    return () => { alive = false; };
  }, [cardId]);

  // Пока модель уточняет разбор, перечитываем его: на процессоре это
  // до минуты
  const pending = !!report?.review?.pending;
  useEffect(() => {
    if (!pending) return;
    const t = setInterval(() => {
      void chief.get(cardId).then((r) => { if (r) setReport(r); }).catch(() => {});
    }, POLL_MS);
    return () => clearInterval(t);
  }, [pending, cardId]);

  // Новые реплики начальника — голосом, если он есть. Уже сказанное при
  // открытии страницы не повторяем
  useEffect(() => {
    const lines = report?.lines ?? [];
    const fresh = lines.slice(spoken.current).filter((l) => l.who === 'chief');
    spoken.current = lines.length;
    for (const l of fresh) void speak(l.text, 'chief');
  }, [report]);

  const run = async (action: () => Promise<Report>) => {
    setBusy(true);
    setError(null);
    try {
      setReport(await action());
    } catch {
      setError('Связь с начальником не удалась, повторите');
    } finally {
      setBusy(false);
    }
  };

  const send = async (said: string) => {
    const t = said.trim();
    if (!t) return;
    setText('');
    await run(() => chief.say(cardId, t));
  };

  const toggleMic = async () => {
    if (!recording) {
      const r = new Recorder();
      if (!(await r.start())) {
        setError('Микрофон недоступен — доложите текстом');
        return;
      }
      recorder.current = r;
      setRecording(true);
      return;
    }
    setRecording(false);
    const wav = await recorder.current?.stop();
    recorder.current = null;
    if (!wav) return;
    setBusy(true);
    const heard = await transcribe(wav);
    setBusy(false);
    if (heard) await send(heard);
    else setError('Речь не распознана — доложите текстом');
  };

  if (!loaded) return null;

  const open = !!report && !report.finishedAt;
  const waiting = busy && !recording;

  return (
    <section className={s.panel} aria-label="Доклад начальнику" data-testid="chief-report">
      <div className={s.head}>
        Доклад начальнику
        {open && <span className={s.live}> · на связи</span>}
      </div>

      {!open && (
        <div className={s.row}>
          <button className={s.btn} disabled={busy || closed} data-testid="chief-start"
                  title={closed ? 'Служба закончила работу по карточке' : undefined}
                  onClick={() => void run(() => chief.start(cardId))}>
            {report ? 'Позвонить ещё раз' : 'Позвонить начальнику'}
          </button>
          {!report && (
            <span className={s.hint}>
              Доложите, что случилось, где, есть ли пострадавшие, кого направили и что с бригадой.
            </span>
          )}
        </div>
      )}

      {report && (
        <ol className={s.lines} data-testid="chief-lines">
          {report.lines.map((l, i) => (
            <li key={i} className={l.who === 'chief' ? s.chief : s.me}>
              <span className={s.who}>{l.who === 'chief' ? 'Начальник' : 'Вы'}:</span> {l.text}
            </li>
          ))}
          {waiting && open && <li className={s.chief}><span className={s.who}>Начальник</span> слушает…</li>}
        </ol>
      )}

      {open && (
        <form className={s.row} onSubmit={(e) => { e.preventDefault(); void send(text); }}>
          <textarea className={s.input} rows={2} value={text} name="chief-say"
                    placeholder="Докладывайте…" disabled={busy || recording}
                    onChange={(e) => setText(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(text); }
                    }} />
          <button type="submit" className={s.btn} disabled={busy || recording || !text.trim()}
                  data-testid="chief-send">Сказать</button>
          {!voiceUnavailable() && (
            <button type="button" className={recording ? s.micOn : s.btn}
                    disabled={waiting || !!micBlockedReason()}
                    title={micBlockedReason() ?? undefined}
                    onClick={() => void toggleMic()} aria-pressed={recording}>
              {recording ? '■ Закончить фразу' : '🎤 Голосом'}
            </button>
          )}
          <button type="button" className={s.btn} disabled={busy || recording}
                  data-testid="chief-finish"
                  onClick={() => void run(() => chief.finish(cardId))}>
            Доклад окончен
          </button>
        </form>
      )}

      {error && <p className={s.error}>{error}</p>}

      {report?.review && <ChiefReviewList review={report.review} />}
    </section>
  );
};
