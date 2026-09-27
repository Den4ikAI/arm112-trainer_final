import type { ChiefReviewResult } from '@/shared/api';
import s from './ChiefReviewList.module.scss';

const STATUS_LABEL = {
  reported: 'доложено',
  missing: 'не доложено',
  wrong: 'доложено неверно',
} as const;

/**
 * Разбор доклада начальнику: по каждому сведению — что в карточке и что
 * сказал диспетчер. Показывается и курсанту после «Принял», и в разборе
 * попытки у преподавателя.
 */
export const ChiefReviewList = ({ review }: { review: ChiefReviewResult }) => (
  <div className={s.wrap} data-testid="chief-review">
    <p className={s.summary}>
      Доложено {review.reported} из {review.total}
      <span className={s.muted}>
        {' · '}{review.mode === 'model' ? 'сверяла нейросеть, цитаты проверены' : 'сверка по ключевым словам'}
        {review.modelMs != null && ` · ${Math.round(review.modelMs / 1000)} с`}
        {review.dropped > 0 && ` · отброшено отметок модели без цитаты: ${review.dropped}`}
      </span>
    </p>
    {review.note && <p className={s.muted}>{review.note}</p>}
    <ul className={s.list}>
      {review.items.map((i) => (
        <li key={i.key} className={s.item} data-testid={`chief-item-${i.key}`}>
          <span className={s[i.status]}>{STATUS_LABEL[i.status]}</span>
          <div>
            <div><strong>{i.title}.</strong> В карточке: {i.expected}</div>
            {i.quote && <div className={s.quote}>сказано: «{i.quote}»</div>}
          </div>
        </li>
      ))}
    </ul>
  </div>
);
