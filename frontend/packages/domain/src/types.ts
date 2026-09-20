/** Доменные типы АРМ-112. Общие для React- и Angular-реализаций. */

/** Статус заявителя — панель выбора из инструкции, раздел 5.3. */
export type ApplicantStatus =
  | 'очевидец' | 'пострадавший' | 'родственник'
  | 'знакомый' | 'ребенок' | 'участник';

/** Ответ в опросной карте: плашка-переключатель либо свободный текст. */
export type AnswerKind = 'single' | 'multi' | 'text' | 'number';

export interface SurveyQuestion {
  id: string;
  label: string;
  kind: AnswerKind;
  options?: string[];
}

/** Опросная карта — то, что раскрывается под выбранным типом происшествия. */
export interface SurveyCard {
  /** Совпадает с id типа происшествия. */
  typeId: string;
  title: string;
  questions: SurveyQuestion[];
}

export interface IncidentType {
  id: string;
  /** Название так, как оно показывается в строке «что случилось?». */
  title: string;
  /** Показывать плашкой среди часто используемых. */
  frequent?: boolean;
  /** Номер группы классификатора (1..23), для связи с эталоном. */
  group?: number;
}

export interface Service {
  id: string;
  title: string;
  /** Основная служба для типа — в боевой системе подчёркнута двойной линией. */
  primary?: boolean;
}

/** Статусы реагирования. Порядок важен — выбор только последовательный. */
export type ResponseStatus =
  | 'Добавлена'
  | 'Получена службой'
  | 'Принята'
  | 'Не принята'
  | 'Начало реагирования'
  | 'Прибытие'
  | 'Проведение работ'
  | 'Работы завершены'
  | 'Отказ от выполнения работ';

export interface ResponseEntry {
  status: ResponseStatus;
  /** Время проставления, ISO. */
  at: string;
  /** «оп. 0» у ВИС, ФИО/номер у АРМ-112. */
  operator: string;
  orderNo?: string;
  comment?: string;
}

export interface ServiceResponse {
  serviceId: string;
  entries: ResponseEntry[];
}

export interface AddressFields {
  raw: string;
  country: string;
  region: string;
  settlement: string;
  object: string;
  district: string;
  area: string;
  street: string;
  house: string;
  building: string;
  structure: string;
  flat: string;
  entrance: string;
  floor: string;
  code: string;
  descriptive: string;
}

export const emptyAddress = (): AddressFields => ({
  raw: '', country: '', region: '', settlement: '', object: '',
  district: '', area: '', street: '', house: '', building: '',
  structure: '', flat: '', entrance: '', floor: '', code: '', descriptive: '',
});

/** Карточка происшествия — модель, общая для режима 112 и режима ДДС. */
export interface IncidentCard {
  id: string;
  /** Номер вида 36814851. */
  number: string;
  createdAt: string;
  operator: string;
  workstation: string;
  phoneAon: string;
  phoneProvided: string;
  phoneOnSite: string;
  applicantName: string;
  applicantStatus: ApplicantStatus | '';
  address: AddressFields;
  description: string;
  victims: number | null;
  noContact: boolean;
  callDropped: boolean;
  typeIds: string[];
  /** questionId → выбранные варианты либо текст. */
  answers: Record<string, string[] | string>;
  serviceIds: string[];
  responses: ServiceResponse[];
  cardStatus: CardStatus;
}

export type CardStatus =
  | 'Зарегистрирована' | 'Отработана' | 'Проверена'
  | 'Не оповещено' | 'Отказ' | 'Не завершено' | 'Завершена';

export const emptyCard = (number: string): IncidentCard => ({
  id: number, number, createdAt: new Date().toISOString(),
  operator: 'УМЦ O n', workstation: '4',
  phoneAon: '', phoneProvided: '', phoneOnSite: '',
  applicantName: '', applicantStatus: '',
  address: emptyAddress(), description: '', victims: null,
  noContact: false, callDropped: false,
  typeIds: [], answers: {}, serviceIds: [], responses: [],
  cardStatus: 'Зарегистрирована',
});
