import { useCallback, useEffect, useState } from 'react';
import { useParams } from 'react-router-dom';
import { cards, channel, type CardDetail } from '@/shared/api';
import { useSession } from '@/entities/session/model';
import { useCatalog } from '@/entities/catalog/model';
import { BrigadePanel } from '@/widgets/brigade-panel/BrigadePanel';
import { CallBar } from '@/widgets/call-bar/CallBar';
import { CardHeader } from '@/widgets/card-header/CardHeader';
import { ServiceTab } from '@/widgets/status-history/ServiceTab';
import { SurveyCardView } from '@/widgets/survey-card/SurveyCardView';
import s from './CardDdsPage.module.scss';
import { stubProps } from '@/shared/ui/Stub';

/**
 * Карточка в режиме ДДС: поля только для чтения, работа идёт через статусы.
 * Норматив подтверждения — 30 секунд с момента направления карточки в службу.
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

  const reload = useCallback(async () => {
    try {
      setCard(await cards.one(id));
      setError(null);
    } catch {
      setError('Карточка недоступна');
    }
  }, [id]);

  useEffect(() => { void reload(); }, [reload]);

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

  const addr = ['country', 'region', 'settlement', 'district', 'street', 'house', 'building']
    .map((f) => card.address[f])
    .filter(Boolean)
    .join(', ');

  const own = user?.serviceId ?? '';

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

          {/* Бригада и доклады — только своей службы: чужую бригаду
              диспетчер не направляет и докладов её не слышит */}
          {own && card.services.some((sv) => sv.serviceId === own) && (
            <BrigadePanel cardId={card.id} serviceId={own} stamp={card} />
          )}
        </div>

        <div className={s.right}>
          <div className={s.flagsRow}>
            <span>Пострадавшие: {card.victims == null ? 'нет' : card.victims}</span>
            <span>Нет контакта: {card.noContact ? 'да' : 'нет'}</span>
            <span>Срыв звонка: {card.callDropped ? 'да' : 'нет'}</span>
            <button {...stubProps('Отметка ЧС')} className={s.chs}>ЧС ⚡</button>
            <button {...stubProps('Отметка ЧП')} className={s.chp}>ЧП ⚠</button>
          </div>

          {card.typeIds.map((t) => (
            <SurveyCardView key={t} typeId={t} answers={card.answers}
                            onAnswer={() => {}} onClose={() => {}} readOnly />
          ))}

          <div className={s.klass} data-testid="card-class">
            Класс.: <strong>{card.outcomeType ?? '—'}</strong>
          </div>
          <div className={s.klassVis}>[ВИС] Класс.:</div>
        </div>
      </div>

      <footer className={s.serviceBar} aria-label="Службы и статусы реагирования">
        <span className={s.caption}>Службы:</span>
        <div className={s.tabs}>
          {card.services.map((sv) => (
            <ServiceTab
              key={sv.serviceId}
              service={sv}
              own={sv.serviceId === own}
              onAdd={async (body) => {
                await cards.addResponse(card.id, { serviceId: sv.serviceId, ...body });
                await reload();
              }}
            />
          ))}
        </div>
        {own && (
          <div className={s.hint}>
            Ваша служба — {serviceTitle(own)}. Норматив подтверждения 30 секунд.
          </div>
        )}
      </footer>
    </div>
  );
};
