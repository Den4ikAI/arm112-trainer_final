import { useCallback, useEffect, useState } from 'react';
import { useParams } from 'react-router-dom';
import { cards, channel, type CardDetail } from '@/shared/api';
import { useSession } from '@/entities/session/model';
import { useCatalog } from '@/entities/catalog/model';
import { AttemptReview } from '@/widgets/attempt-review/AttemptReview';
import { BrigadePanel } from '@/widgets/brigade-panel/BrigadePanel';
import { CallBar } from '@/widgets/call-bar/CallBar';
import { CardHeader } from '@/widgets/card-header/CardHeader';
import { ChiefReport } from '@/widgets/chief-report/ChiefReport';
import { ServiceTab } from '@/widgets/status-history/ServiceTab';
import { SurveySummary } from '@/widgets/survey-card/SurveyCardView';
import s from './CardDdsPage.module.scss';
import { stubProps } from '@/shared/ui/Stub';
import { orderServices } from '@/shared/lib/serviceOrder';

/**
 * Карточка в режиме ДДС: поля только для чтения, работа идёт через статусы.
 * Нормативы — ответ заказчика в чате задачи 27.09: 30 секунд от поступления
 * карточки в список до её открытия, 3 минуты — до первой записи (статуса
 * и текста). Открытие сервер отмечает записью «Получена службой».
 *
 * Карандаш есть только у своей службы: диспетчер отвечает за неё и ни за
 * чью больше. Сервер это тоже проверяет — кнопку мало спрятать.
 */
export const CardDdsPage = () => {
  const { id = '' } = useParams();
  const [card, setCard] = useState<CardDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const user = useSession((st) => st.user);
  const serviceTitle = useCatalog((st) => st.serviceTitle);
  const serviceShort = useCatalog((st) => st.serviceShort);

  const reload = useCallback(async () => {
    try {
      setCard(await cards.one(id));
      setError(null);
    } catch {
      setError('Карточка недоступна');
    }
  }, [id]);

  useEffect(() => { void reload(); }, [reload]);

  // Открытие карточки — это и есть выполнение норматива 30 секунд.
  // Отмечает только курсант своей службы; сервер сам игнорирует
  // повтор и открытие преподавателем
  const own = user?.serviceId ?? '';
  const addressed = !!card?.services.some((sv) => sv.serviceId === own);
  // Своя служба закончила работу — карточка только для просмотра:
  // звонки старшему и новый звонок начальнику закрыты (сервер — 409)
  const closed = !!card?.services.find((sv) => sv.serviceId === own)?.entries
    .some((e) => e.status === 'Работы завершены' || e.status === 'Отказ от выполнения работ');
  useEffect(() => {
    if (user?.role !== 'student' || !addressed) return;
    void cards.opened(id).then(reload).catch(() => {});
  }, [id, user?.role, addressed, reload]);

  // Одну карточку ведут несколько ДДС сразу: статус, проставленный
  // соседней службой, должен появиться без обновления страницы
  useEffect(() => {
    const topics = user?.serviceId ? [`service:${user.serviceId}`] : [];
    if (topics.length) channel.subscribe(topics);

    const off = channel.on((e) => {
      if (e.cardId === id || e.type === 'lagged') void reload();
    });

    return () => {
      off();
      if (topics.length) channel.unsubscribe(topics);
    };
  }, [id, reload, user?.serviceId]);

  if (error) return <div className={s.page}><p className={s.empty}>{error}</p></div>;
  if (!card) return <div className={s.page}><p className={s.empty}>Загрузка…</p></div>;

  // Номерные службы впереди и первая из них подчёркнута — как на кадре
  const bar = orderServices(card.services.map((sv) => sv.serviceId), serviceShort);

  const addr = ['country', 'region', 'settlement', 'district', 'street', 'house', 'building']
    .map((f) => card.address[f])
    .filter(Boolean)
    .join(', ');


  return (
    <div className={s.page}>
      <div className={s.top}>
        <CallBar aon={card.phoneAon} provided={card.phoneProvided}
                 onSite={card.phoneOnSite} readOnly />
        <CardHeader number={card.number}
                    savedAt={new Date(card.savedAt ?? card.createdAt).toLocaleString('ru-RU')}
                    operator={card.operator} workstation={card.workstation ?? ''} mode="dds" />
      </div>

      <div className={s.main}>
        <div className={s.left}>
          <div className={s.applicantRow}>
            {card.applicantName || 'ФИО заявителя'}
            {card.applicantStatus && ` (${card.applicantStatus})`}
          </div>

          <div className={s.addr}>
            <div className={s.addrMain}>{addr || 'Адрес не заполнен'}</div>
            {card.address.descriptive && (
              <div className={s.addrSub}>{card.address.descriptive}</div>
            )}
          </div>

          <div className={s.feed}>
            <div className={s.feedRow}>
              <div className={s.feedMeta}>
                {new Date(card.savedAt ?? card.createdAt).toLocaleString('ru-RU')}
                &nbsp; {card.operator}
              </div>
              <div className={s.feedText}>{card.description}</div>
            </div>
          </div>

          {own && (
            <div className={s.hint}>
              Ваша служба — {serviceTitle(own)}. Нормативы: открыть карточку — 30 с
              с поступления, первая запись (статус и текст) — 3 мин.
            </div>
          )}

          {/* Бригада и доклады — только своей службы: чужую бригаду
              диспетчер не направляет и докладов её не слышит */}
          {own && card.services.some((sv) => sv.serviceId === own) && (
            <BrigadePanel cardId={card.id} serviceId={own} stamp={card} closed={closed} />
          )}

          {/* Доклад начальнику ведёт диспетчер своей службы; преподаватель
              видит его в разборе попытки ниже */}
          {user?.role === 'student' && addressed && <ChiefReport cardId={card.id} closed={closed} />}

          {/* Оператор 112 открывает здесь свою сохранённую карточку: смотреть
              и читать разбор. Работа ДДС по ней — у диспетчеров служб */}
          {user?.role === 'student' && user.duty === 'op112' && (
            <p className={s.ownNote} data-testid="own-card-note">
              Ваша карточка 112 — только просмотр. Работа по ней идёт у диспетчеров
              служб; ваш разбор — ниже.
            </p>
          )}

          {/* Разбор попыток по карточке: преподаватель видит всех и ставит
              экспертную оценку, курсант — только свою попытку */}
          <AttemptReview cardId={card.id} />

        </div>

        <div className={s.right}>
          <div className={s.flagsRow}>
            <span>Пострадавшие: {card.victims == null ? 'нет' : card.victims}</span>
            <span>Нет контакта: {card.noContact ? 'да' : 'нет'}</span>
            <span>Срыв звонка: {card.callDropped ? 'да' : 'нет'}</span>
            <button {...stubProps('Отметка ЧС')} className={s.chs}>ЧС ⚡</button>
            <button {...stubProps('Отметка ЧП')} className={s.chp}>ЧП ⚠</button>
          </div>

          {/* Как в боевой карточке ДДС: не опросная карта с вариантами, а
              сводка того, что выбрал оператор 112 */}
          {card.typeIds.map((t) => (
            <SurveySummary key={t} typeId={t} answers={card.answers} />
          ))}

          <div className={s.klass} data-testid="card-class">
            Класс.: <strong>{card.outcomeType ?? '—'}</strong>
          </div>
          <div className={s.klassVis}>[ВИС] Класс.:</div>
        </div>
      </div>

      <footer className={s.serviceBar} aria-label="Службы и статусы реагирования">
        <span className={s.caption}>Службы:</span>
        {/* Не влезающие в ряд вкладки встают рядом выше, как на кадре
            боевой системы, — а не уезжают в прокрутку */}
        <div className={s.tabs}>
          {bar.ordered.map((id) => card.services.find((sv) => sv.serviceId === id)!).map((sv) => (
            <ServiceTab
              key={sv.serviceId}
              service={sv}
              own={sv.serviceId === own}
              main={sv.serviceId === bar.main}
              addedAt={card.savedAt}
              onAdd={async (body) => {
                await cards.addResponse(card.id, { serviceId: sv.serviceId, ...body });
                await reload();
              }}
            />
          ))}
        </div>
        <div className={s.barTools}>
          <button {...stubProps('Свернуть службы')} className={s.barBtn}>
            <svg viewBox="0 0 14 16" width="14" height="16" aria-hidden="true">
              <path d="M2 2l5 4 5-4M2 14l5-4 5 4" fill="none" stroke="currentColor" strokeWidth="1.6" />
            </svg>
          </button>
        </div>
        <div className={s.barRight}>
          <button {...stubProps('Сообщение')} className={s.barBtn}>
            <svg viewBox="0 0 18 16" width="18" height="16" aria-hidden="true">
              <path d="M1 1h16v11H6l-4 3v-3H1z" fill="currentColor" />
              <path d="M9 3.5v5M9 10v1" stroke="#49555d" strokeWidth="1.8" />
            </svg>
          </button>
          <button {...stubProps('Закрыть карточку')} className={s.barBtn}>
            <svg viewBox="0 0 14 14" width="14" height="14" aria-hidden="true">
              <path d="M2 2l10 10M12 2L2 12" stroke="currentColor" strokeWidth="1.8" />
            </svg>
          </button>
        </div>
      </footer>
    </div>
  );
};
