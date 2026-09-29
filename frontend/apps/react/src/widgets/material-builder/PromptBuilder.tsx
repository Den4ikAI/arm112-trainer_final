import { useState } from 'react';
import { ApiError } from '@/shared/api';
import s from './MaterialBuilder.module.scss';

interface Props {
  /** «вызов 112» / «происшествие ДДС» — для подписей */
  what: string;
  placeholder: string;
  onSubmit: (title: string, prompt: string) => Promise<void>;
  onCancel: () => void;
}

/**
 * Создание по описанию: преподаватель пишет, что произошло, модель
 * собирает черновик. Дальше — правка и утверждение, как у любого
 * черновика: модель предлагает, решает преподаватель.
 */
export const PromptBuilder = ({ what, placeholder, onSubmit, onCancel }: Props) => {
  const [title, setTitle] = useState('');
  const [prompt, setPrompt] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSubmit(title.trim(), prompt.trim());
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не получилось');
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className={s.form} data-testid="prompt-builder"
          onSubmit={(e) => { e.preventDefault(); void go(); }}>
      <h2 className={s.title}>Создать {what} по описанию (ИИ)</h2>
      <p className={s.hint}>
        Модель подберёт тип и признаки из классификатора и напишет черновик.
        Службы и эталон посчитает классификатор. Черновик потом можно
        поправить перепиской или вручную и утвердить.
      </p>
      <label className={s.field}>
        <span>Тема</span>
        <input value={title} onChange={(e) => setTitle(e.target.value)}
               placeholder="Пожар в жилом доме" aria-label="Тема" data-testid="prompt-title" />
      </label>
      <label className={s.field}>
        <span>Что произошло</span>
        <textarea value={prompt} required onChange={(e) => setPrompt(e.target.value)}
                  placeholder={placeholder} aria-label="Описание" data-testid="prompt-text" />
      </label>
      {error && <p className={s.error} role="alert">{error}</p>}
      <div className={s.actions}>
        <button className={s.primary} type="submit" disabled={busy || !prompt.trim()}
                data-testid="prompt-submit">
          {busy ? 'модель готовит…' : 'Создать черновик'}
        </button>
        <button className={s.plain} type="button" onClick={onCancel}>Отмена</button>
        {busy && (
          <span className={s.note}>
            На видеокарте — секунды, на процессоре — до двух минут.
          </span>
        )}
      </div>
    </form>
  );
};
