/**
 * Справочники, снятые со скриншотов боевой ПОВ-112.
 * Опросные карты 101 / 104 / Взрыв воспроизведены дословно по скриншотам
 * (refs/СКРИНШОТ КАРТОЧКИ 112ГСИ.docx, изображения 2, 4, 7, 9).
 */
import type { IncidentType, Service, SurveyCard, ApplicantStatus } from './types';

export const APPLICANT_STATUSES: ApplicantStatus[] = [
  'очевидец', 'пострадавший', 'родственник', 'знакомый', 'ребенок', 'участник',
];

/** Плашки частых типов на пустой карточке — порядок как на скриншоте. */
export const INCIDENT_TYPES: IncidentType[] = [
  { id: 'dtp', title: 'ДТП', frequent: true, group: 2 },
  { id: 'wrong-number', title: 'Ошибочно набран номер', frequent: true, group: 23 },
  { id: '104', title: '104', frequent: true, group: 13 },
  { id: 'person-danger', title: 'Человек в опасности', frequent: true, group: 17 },
  { id: 'cancel', title: 'Отмена вызова', frequent: true, group: 23 },
  { id: 'test-call', title: 'Тестовый вызов', frequent: true, group: 23 },
  { id: 'shift-handover', title: 'Передача дежурства', frequent: true, group: 23 },
  { id: 'consult', title: 'Консультация', frequent: true, group: 23 },
  { id: 'foreign', title: 'Вызов на иностранном языке', frequent: true, group: 23 },
  { id: 'ref-101', title: 'Справка-101', frequent: true, group: 23 },
  { id: '101', title: '101', group: 1 },
  { id: '102', title: '102', group: 15 },
  { id: '103', title: '103', group: 22 },
  { id: 'city-utilities', title: 'Аварии и происшествия в городском хозяйстве', group: 14 },
  { id: 'hydro', title: 'Аварии на гидротехнических сооружениях', group: 9 },
  { id: 'hazard-obj', title: 'Аварии на опасных и производственных объектах', group: 10 },
  { id: 'gratitude', title: 'Благодарность службам', group: 23 },
  { id: 'bpla', title: 'БПЛА', group: 24 },
  { id: 'explosion', title: 'Взрыв', group: 3 },
  { id: 'collapse', title: 'Обрушения', group: 5 },
  { id: 'gas-smell', title: 'Запах газа', group: 13 },
  { id: 'child-danger', title: 'Ребенок в опасности', group: 18 },
  { id: 'road-problem', title: 'Проблемы на дороге', group: 16 },
  { id: 'animals', title: 'Происшествия с участием животных', group: 21 },
];

export const SERVICES: Service[] = [
  { id: 's101', title: 'Служба 101', primary: true },
  { id: 's102', title: 'Служба 102' },
  { id: 's103', title: 'Служба 103' },
  { id: 's104', title: 'Служба 104' },
  { id: 'fsb', title: 'ФСБ' },
  { id: 'cemp', title: 'ЦЭМП' },
  { id: 'codd', title: 'ЦОДД' },
  { id: 'dep-jkh', title: 'Деп. ЖКХ' },
  { id: 'mosbez', title: 'Мос.Без.' },
  { id: 'moslift', title: 'Мослифт' },
  { id: 'oati', title: 'ОАТИ' },
  { id: 'mosvodokanal', title: 'Мосводоканал' },
  { id: 'moek', title: 'МОЭК' },
  { id: 'mosgaz', title: 'МОСГАЗ' },
  { id: 'voronovskoe', title: 'Поселение Вороновское' },
  { id: 'tihvinskoe', title: 'Поселение Тихвинское' },
  { id: 'cukb-bpla', title: 'ЦУКБ.БПЛА Минобороны' },
];

/** Опросные карты. Формулировки — дословно с экрана. */
export const SURVEY_CARDS: SurveyCard[] = [
  {
    typeId: '101',
    title: 'Происшествие 101',
    questions: [
      { id: 'where', label: 'Где', kind: 'single',
        options: ['Улица', 'Транспорт', 'Дом', 'Здание / объект', 'Опасный объект'] },
      { id: 'fire-sign', label: 'Признак пожара (дом)', kind: 'single',
        options: ['Открытое пламя / Дым', 'Запах гари', 'Сработала пожарная сигнализация'] },
      { id: 'access', label: 'Доступ', kind: 'multi', options: ['Нет доступа'] },
      { id: 'house-kind', label: 'Дом (пламя, дым)', kind: 'single',
        options: ['Дом многоквартирный', 'Дом частный', 'Дача',
                  'Сарай / бытовка / хоз. постройка', 'Выселенное здание'] },
      { id: 'storeys', label: 'Этажность здания', kind: 'number' },
      { id: 'people-threat', label: 'Угроза людям', kind: 'single', options: ['Да', 'Нет'] },
      { id: 'inhouse', label: 'Внутридомовые объекты (пламя, дым)', kind: 'multi',
        options: ['Квартира', 'Балкон', 'Газовая колонка', 'Газовая плита', 'Лифт',
                  'Мусоропровод', 'Подъезд', 'Счетчик электричества',
                  'Электрическая проводка', 'Электрощит', 'Лестничная клетка',
                  'Подвал', 'Прочие внутридомовые объекты', 'Крыша'] },
      { id: 'road-block', label: 'Есть ли перекрытие движения', kind: 'single', options: ['Да', 'Нет'] },
      { id: 'gasified', label: 'Проведена ли газификация', kind: 'single',
        options: ['Да', 'Нет', 'Нет данных'] },
      { id: 'note', label: 'Описание', kind: 'text' },
    ],
  },
  {
    typeId: '104',
    title: 'Происшествие 104',
    questions: [
      { id: 'gas-sign', label: 'Признаки происшествия', kind: 'single',
        options: ['Запах газа вне помещения (на улице)',
                  'Запах газа в помещении (в квартире, в доме)',
                  'Нарушение в работе газового оборудования',
                  'Повреждение газопровода', 'Повышенное давление газа'] },
      { id: 'people-threat', label: 'Угроза людям', kind: 'single', options: ['Да', 'Нет'] },
    ],
  },
  {
    typeId: 'explosion',
    title: 'П: Взрыв',
    questions: [
      { id: 'where-blast', label: 'Где взрыв', kind: 'single',
        options: ['Здание / Объект', 'Транспорт',
                  'Звуки похожие на взрыв, что взорвалось сообщить не может'] },
      { id: 'has-fire', label: 'Есть возгорание', kind: 'single', options: ['Да', 'Нет'] },
      { id: 'collapse-threat', label: 'Есть угроза обрушения', kind: 'single', options: ['Да', 'Нет'] },
      { id: 'damage', label: 'Какие видят разрушения', kind: 'text' },
    ],
  },
  {
    typeId: 'dtp',
    title: 'Происшествие ДТП',
    questions: [
      { id: 'dtp-where', label: 'Где', kind: 'single',
        options: ['Проезжая часть', 'Тротуар', 'Двор', 'Тоннель', 'Мост / эстакада'] },
      { id: 'participants', label: 'Участники', kind: 'multi',
        options: ['Легковой автомобиль', 'Грузовой автомобиль', 'Автобус',
                  'Мотоцикл', 'Пешеход', 'Велосипедист', 'Электросамокат'] },
      { id: 'victims-dtp', label: 'Пострадавшие', kind: 'single',
        options: ['Да', 'Нет', 'Нет данных'] },
      { id: 'blocked', label: 'Люди заблокированы в транспорте', kind: 'single', options: ['Да', 'Нет'] },
      { id: 'road-block', label: 'Есть ли перекрытие движения', kind: 'single', options: ['Да', 'Нет'] },
      { id: 'fuel-spill', label: 'Разлитие топлива', kind: 'single', options: ['Да', 'Нет'] },
    ],
  },
  {
    typeId: 'bpla',
    title: 'П: БПЛА',
    questions: [
      { id: 'bpla-state', label: 'Состояние', kind: 'single',
        options: ['Летит', 'Готовят к запуску', 'Упал / столкнулся'] },
      { id: 'bpla-fire', label: 'Есть взрыв или возгорание', kind: 'single', options: ['Да', 'Нет'] },
      { id: 'bpla-area', label: 'Территория', kind: 'single',
        options: ['Москва', 'Регион'] },
    ],
  },
];

export const getSurveyCard = (typeId: string): SurveyCard | undefined =>
  SURVEY_CARDS.find((c) => c.typeId === typeId);

export const getIncidentType = (id: string): IncidentType | undefined =>
  INCIDENT_TYPES.find((t) => t.id === id);

export const getService = (id: string): Service | undefined =>
  SERVICES.find((s) => s.id === id);
