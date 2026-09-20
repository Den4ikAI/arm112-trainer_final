import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { getService, type ResponseEntry, type ServiceResponse } from '@arm112/domain';
import { CallBar } from '@/widgets/call-bar/CallBar';
import { CardHeader } from '@/widgets/card-header/CardHeader';
import { ServiceTab } from '@/widgets/status-history/ServiceTab';
import s from './CardDdsPage.module.scss';

/**
 * Карточка в режиме ДДС: поля только для чтения, работа идёт через статусы.
 * Норматив подтверждения — 30 секунд с момента направления карточки в службу.
 */
const SERVICES_ON_CARD = ['s101', 's104', 's102', 'dep-jkh', 'cemp', 'codd', 'mosbez', 'moslift'];
const OWN_SERVICE = 'dep-jkh';

const baseAt = (minutesAgo: number) =>
  new Date(Date.now() - minutesAgo * 60_000).toISOString();

export const CardDdsPage = () => {
  const { number = '36814845' } = useParams();

  const [responses, setResponses] = useState<ServiceResponse[]>(
    SERVICES_ON_CARD.map((id) => ({
      serviceId: id,
      entries: [{ status: 'Добавлена', at: baseAt(12), operator: 'оп. 0' } as ResponseEntry],
    })),
  );

  const add = (serviceId: string, entry: ResponseEntry) =>
    setResponses((prev) => prev.map((r) =>
      r.serviceId === serviceId ? { ...r, entries: [...r.entries, entry] } : r));

  return (
    <div className={s.page}>
      <div className={s.top}>
        <CallBar aon="" provided="" onSite="" readOnly />
        <CardHeader number={number} savedAt={new Date().toLocaleString('ru-RU')}
                    operator="УМЦ O n" workstation="4" mode="dds" />
      </div>

      <div className={s.main}>
        <div className={s.left}>
          <div className={s.applicantRow}>ФИО заявителя</div>

          <div className={s.addr}>
            <div className={s.addrMain}>Россия, Москва, (ТАО, Вороновское)</div>
            <div className={s.addrSub}>Троицкий административный округ</div>
          </div>

          <div className={s.feed}>
            <div className={s.feedRow}>
              <div className={s.feedMeta}>
                {new Date().toLocaleDateString('ru-RU')} 11:13:19 &nbsp; 0 УМЦ O.n.
              </div>
              <div className={s.feedText}>Пожар в квартире</div>
            </div>
          </div>
        </div>

        <div className={s.right}>
          <div className={s.flagsRow}>
            <span>Пострадавшие: нет</span>
            <span>Отказ от скорой: нет</span>
            <span>Заблокированные: нет</span>
            <button className={s.chs}>ЧС ⚡</button>
            <button className={s.chp}>ЧП ⚠</button>
            <button className={s.pencil} aria-label="Редактировать">✎</button>
          </div>

          <div className={s.typeHead}>Происшествие 101</div>
          <div className={s.signs}>
            Дом . Открытое пламя / Дым (дом), Запах гари (дом) . Дом многоквартирный .
            квартира . Есть угроза людям . Есть газификация .
          </div>
          <div className={s.klass} data-testid="card-class">Класс.: <strong>пожар: квартира ;</strong></div>
          <div className={s.klassVis}>[ВИС] Класс.:</div>
        </div>
      </div>

      <footer className={s.serviceBar} aria-label="Службы и статусы реагирования">
        <span className={s.caption}>Службы:</span>
        <div className={s.tabs}>
          {SERVICES_ON_CARD.map((id) => (
            <ServiceTab
              key={id} serviceId={id}
              response={responses.find((r) => r.serviceId === id)}
              own={id === OWN_SERVICE}
              onAdd={(entry) => add(id, entry)}
            />
          ))}
        </div>
        <div className={s.hint}>
          Ваша служба — {getService(OWN_SERVICE)?.title}. Норматив подтверждения 30 секунд.
        </div>
      </footer>
    </div>
  );
};
