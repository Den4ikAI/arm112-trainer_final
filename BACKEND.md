# Бэкенд тренажёра АРМ-112 — спецификация MVP

Документ описывает серверную часть учебного ПО для подготовки операторов ДДС
(задача №9, Департамент ГО и ЧС Москвы). Опирается на готовый фронтенд
(`frontend/`), серверный домен (`backend/src/domain`) и материалы задачи.

Всё, что касается ИИ, спроектировано как интерфейс с заглушкой: MVP работает
полностью без моделей, модели подключаются позже заменой реализации.

---

## 1. Что делает бэкенд и чего не делает

**Делает:**

* хранит пользователей, группы, занятия, карточки, статусы, оценки;
* держит **единственный источник правды по эталону** — классификатор
  и движок оценки; фронтенд их только отображает;
* версионирует классификатор и считает дельту между версиями;
* раздаёт учебные сценарии и принимает результаты;
* ведёт аудит и формирует отчётность;
* координирует класс через WebSocket (карточка от оператора к диспетчеру ДДС).

**Не делает в MVP:**

* не обрабатывает реальные вызовы и не подключается к настоящей системе-112;
* не содержит SIP-стека (голос — отдельная задача, см. §12);
* не тренирует модели, только вызывает их через адаптеры.

---

## 2. Системные требования и что из них следует

Из ТЗ, раздел «Требования к производительности»:

| Требование | Значение | Следствие для бэкенда |
|---|---|---|
| Время отклика интерфейса | ≤ 2 с при 100 пользователях | любой синхронный эндпоинт ≤ 300 мс p95; тяжёлое — в фон |
| Пропускная способность | ≥ 20 одновременных сессий | пул БД ≥ 32, без блокирующих операций в хендлерах |
| Задержка голоса (VoIP) | ≤ 150 мс | голос не проходит через бэкенд, только сигнализация |
| Сбой сети до 30 с | восстановление сессий без потери данных | идемпотентные записи, автосохранение, resume по токену |
| Запись в БД | ≥ 100 операций/с | батчинг статусов и телеметрии, отдельный пул на запись |
| Аналитические отчёты | ≤ 30 с | генерация в фоне, выдача по job-id |
| Журналы безопасности | хранение ≥ 6 мес | отдельная таблица аудита с партиционированием по месяцам |
| Резервное копирование | ≥ 1 раз в сутки | cron + `pg_dump`, ретенция 30 дней |

Сервер (из ТЗ): **Intel Core i7 / Xeon от 6 ядер, 32 ГБ ОЗУ, NVMe SSD 512 ГБ**,
без видеокарты. Клиентские АРМ: i5, 16 ГБ, SSD 256 ГБ.

СУБД: **PostgreSQL 12+**. ОС: Windows 10/11 или Ubuntu 20.04+.
Браузеры: Chrome, Firefox, Яндекс.Браузер.

**Главное следствие из железа:** на 6 ядрах без GPU нельзя держать
генеративную модель в горячем пути. Поэтому вся генерация — офлайн,
на этапе подготовки занятия, а во время вызова работает детерминированный
движок. Подробно в §10.

---

## 3. Архитектура

```
┌─────────────────────────────────────────────────────────┐
│ Браузер (АРМ обучающегося / преподавателя)              │
│   Angular или React, WebRTC для голоса                  │
└───────────────┬──────────────────────┬──────────────────┘
                │ REST + WS            │ WebRTC (медиа мимо бэкенда)
┌───────────────▼──────────────────────▼──────────────────┐
│ Бэкенд (один процесс)                                   │
│                                                         │
│  api/            REST-хендлеры, валидация, RBAC         │
│  ws/             канал класса, события занятия          │
│  domain/         классификатор, статусы, оценка         │
│  ingest/         импорт XLSX, дельта версий             │
│  jobs/           фоновые задачи, очередь                │
│  ml/             адаптеры моделей (в MVP — моки)        │
│  store/          PostgreSQL, миграции                   │
│  audit/          журнал действий                        │
└───────────────┬─────────────────────────────────────────┘
                │
        ┌───────▼────────┐        ┌──────────────────────┐
        │ PostgreSQL 12+ │        │ Файловое хранилище   │
        │                │        │ аудио, PDF, xlsx     │
        └────────────────┘        └──────────────────────┘
```

Один процесс, одна база. Горизонтальное масштабирование — добавлением
инстансов за балансировщиком, состояние сессии в БД, WebSocket-события
через `LISTEN/NOTIFY` PostgreSQL (в MVP — один инстанс, шина не нужна).

### Выбор стека

Спецификация языконезависима: схема БД, контракт API и алгоритмы
одинаковы. Практические замечания:

* **Rust (axum + tokio + sqlx)** — подходит, если в команде двое и больше
  пишут на нём. Даёт нативный ONNX через `ort` и `tokenizers`, то есть
  Python со стенда уходит полностью. `sqlx` брать без compile-time макросов:
  на хакатоне схема меняется ежечасно, живая база при сборке будет мешать.
* **Python (FastAPI + SQLAlchemy)** — быстрее в разработке, весь ML-стек
  родной, но на стенде появляется venv с моделями.

Что бы ни выбрали, §4–§9 не меняются.

---

## 4. Доменная модель

Источник истины — схема PostgreSQL ниже и миграции в `backend/migrations`.
Формы ответов API описаны в `frontend/apps/react/src/shared/api/types.ts`
и списаны с обработчиков `backend/src/api`.

*(До задачи №18 источником истины был фронтовой пакет
`frontend/packages/domain/src/types.ts`. Пакет удалён: держать доменные
правила в двух местах — верный способ показать курсанту одно, а в отчёт
записать другое.)*

### 4.1 Пользователи и службы

**ДДС — это организация, а не человек.** Пользователь принадлежит службе,
и от этого зависит, какие карточки он видит. Роль и служба — независимые оси.

```sql
CREATE TABLE services (
  id            text PRIMARY KEY,           -- 's101', 'dep-jkh', 'moslift'
  title         text NOT NULL,              -- 'Служба 101', 'Деп. ЖКХ'
  is_primary    boolean NOT NULL DEFAULT false,
  external_code text,                       -- код в ЕКП, если есть
  created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TYPE user_role AS ENUM ('student', 'teacher', 'admin');

CREATE TABLE users (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  login         text UNIQUE NOT NULL,
  password_hash text NOT NULL,              -- argon2id
  fio           text NOT NULL,
  role          user_role NOT NULL,
  service_id    text REFERENCES services(id),  -- NULL для преподавателя/админа
  workstation   text,                       -- номер АРМ, как в боевой системе
  is_active     boolean NOT NULL DEFAULT true,
  created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX ON users (service_id) WHERE is_active;
```

Требование ТЗ: «чтобы в их ленту попадали только профильные события».
Реализуется фильтром `cards → card_services → users.service_id`.

### 4.2 Классификатор и версии

Ключевая сущность продукта. Классификатор живой: между двумя версиями
из материалов задачи появились 8 служб и позиция «БПЛА».

```sql
CREATE TABLE classifier_versions (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  name         text NOT NULL,               -- 'v046_24 корректировка МВД'
  source_file  text,                        -- имя загруженного xlsx
  imported_at  timestamptz NOT NULL DEFAULT now(),
  imported_by  uuid REFERENCES users(id),
  is_active    boolean NOT NULL DEFAULT false,
  positions_count int NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX ON classifier_versions (is_active) WHERE is_active;

CREATE TABLE classifier_positions (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  version_id   uuid NOT NULL REFERENCES classifier_versions(id) ON DELETE CASCADE,
  number       text NOT NULL,               -- '1010101', '24120200'
  group_no     int NOT NULL,                -- 1..24
  group_title  text NOT NULL,               -- 'Пожары и задымления'
  sign1        text,                        -- 112-Признак.1
  sign2        text,
  sign3        text,
  extra_signs  text,
  outcome_type text,                        -- итоговый тип: 'пожар: мусор'
  ekp_type     text,                        -- ТИП происшествия ЕКП 35
  UNIQUE (version_id, number)
);
CREATE INDEX ON classifier_positions (version_id, group_no);

-- матрица «позиция × служба» с условием оповещения
CREATE TABLE classifier_position_services (
  position_id  uuid NOT NULL REFERENCES classifier_positions(id) ON DELETE CASCADE,
  service_id   text NOT NULL REFERENCES services(id),
  condition    text,     -- 'признак не выбран', 'угроза людям', 'пострадавшие'
  value        text,     -- 'карточка-112', 'пожар: мусор', 'нет реагирования'
  PRIMARY KEY (position_id, service_id, condition)
);
```

### 4.3 Опросные карты

```sql
CREATE TABLE incident_types (
  id         text PRIMARY KEY,              -- '101', 'dtp', 'bpla'
  title      text NOT NULL,
  frequent   boolean NOT NULL DEFAULT false, -- показывать плашкой
  group_no   int
);

CREATE TYPE answer_kind AS ENUM ('single', 'multi', 'text', 'number');

CREATE TABLE survey_questions (
  id         text NOT NULL,                 -- 'where', 'fire-sign'
  type_id    text NOT NULL REFERENCES incident_types(id) ON DELETE CASCADE,
  label      text NOT NULL,                 -- 'Признак пожара (дом)'
  kind       answer_kind NOT NULL,
  sort_order int NOT NULL,
  PRIMARY KEY (type_id, id)
);

CREATE TABLE survey_options (
  type_id     text NOT NULL,
  question_id text NOT NULL,
  value       text NOT NULL,                -- 'Открытое пламя / Дым'
  sort_order  int NOT NULL,
  PRIMARY KEY (type_id, question_id, value),
  FOREIGN KEY (type_id, question_id) REFERENCES survey_questions(type_id, id) ON DELETE CASCADE
);
```

### 4.4 Сценарии

Соответствует типу `Scenario`. Обратите внимание: **`expected` не пишется
моделью**, он выводится из позиции классификатора. Модель заполняет только
текстовую обвязку.

```sql
CREATE TYPE scenario_status AS ENUM ('draft', 'approved', 'archived');

CREATE TABLE scenarios (
  id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  ticket           int,                     -- номер билета-источника
  call_no          int,
  legend           text NOT NULL,           -- первая фраза заявителя
  applicant_name   text NOT NULL,
  applicant_phone  text NOT NULL,
  applicant_status text NOT NULL,           -- очевидец/пострадавший/...
  norm_sec         int NOT NULL DEFAULT 60,
  expected         jsonb NOT NULL,          -- {typeIds, answers, address, services}
  version_id       uuid REFERENCES classifier_versions(id),
  status           scenario_status NOT NULL DEFAULT 'draft',
  created_by       uuid REFERENCES users(id),
  approved_by      uuid REFERENCES users(id),
  created_at       timestamptz NOT NULL DEFAULT now()
);

-- скрытые факты: заявитель сообщает их только в ответ на вопрос
CREATE TABLE scenario_hidden_facts (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  scenario_id uuid NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
  key         text NOT NULL,                -- 'city', 'victims', 'gas'
  question    text NOT NULL,
  answer      text NOT NULL,
  unlocks     text[] NOT NULL,              -- ['address.settlement','address.region']
  sort_order  int NOT NULL
);
```

### 4.5 Группы и задания

```sql
CREATE TABLE training_groups (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  title      text NOT NULL,
  teacher_id uuid NOT NULL REFERENCES users(id),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE group_members (
  group_id uuid NOT NULL REFERENCES training_groups(id) ON DELETE CASCADE,
  user_id  uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, user_id)
);

CREATE TABLE assignments (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  group_id    uuid REFERENCES training_groups(id),
  user_id     uuid REFERENCES users(id),    -- либо группе, либо лично
  scenario_id uuid NOT NULL REFERENCES scenarios(id),
  norm_sec    int,                          -- переопределяет сценарный
  due_at      timestamptz,
  created_by  uuid NOT NULL REFERENCES users(id),
  created_at  timestamptz NOT NULL DEFAULT now(),
  CHECK (group_id IS NOT NULL OR user_id IS NOT NULL)
);
```

### 4.6 Занятия, карточки, статусы

```sql
CREATE TYPE session_mode AS ENUM ('card112', 'dds');

CREATE TABLE sessions (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id       uuid NOT NULL REFERENCES users(id),
  scenario_id   uuid REFERENCES scenarios(id),
  assignment_id uuid REFERENCES assignments(id),
  mode          session_mode NOT NULL,
  started_at    timestamptz NOT NULL DEFAULT now(),
  finished_at   timestamptz,
  norm_sec      int NOT NULL
);
CREATE INDEX ON sessions (user_id, started_at DESC);

CREATE TYPE card_status AS ENUM (
  'Зарегистрирована','Отработана','Проверена',
  'Не оповещено','Отказ','Не завершено','Завершена'
);

CREATE TABLE cards (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  number        text UNIQUE NOT NULL,       -- '36814851'
  session_id    uuid REFERENCES sessions(id) ON DELETE CASCADE,
  created_by    uuid REFERENCES users(id),
  workstation   text,
  phone_aon        text NOT NULL DEFAULT '',
  phone_provided   text NOT NULL DEFAULT '',
  phone_on_site    text NOT NULL DEFAULT '',
  applicant_name   text NOT NULL DEFAULT '',
  applicant_status text NOT NULL DEFAULT '',
  address       jsonb NOT NULL,             -- AddressFields, 15 полей
  description   text NOT NULL DEFAULT '',
  victims       int,
  no_contact    boolean NOT NULL DEFAULT false,
  call_dropped  boolean NOT NULL DEFAULT false,
  type_ids      text[] NOT NULL DEFAULT '{}',
  answers       jsonb NOT NULL DEFAULT '{}',-- questionId → string|string[]
  outcome_type  text,                       -- рассчитан классификатором
  status        card_status NOT NULL DEFAULT 'Зарегистрирована',
  created_at    timestamptz NOT NULL DEFAULT now(),
  saved_at      timestamptz
);
CREATE INDEX ON cards (created_at DESC);
CREATE INDEX ON cards USING gin (answers);

CREATE TYPE service_source AS ENUM ('auto', 'manual', 'vis');

CREATE TABLE card_services (
  card_id    uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  service_id text NOT NULL REFERENCES services(id),
  source     service_source NOT NULL DEFAULT 'auto',
  added_at   timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (card_id, service_id)
);
CREATE INDEX ON card_services (service_id);   -- фильтр ленты по своей ДДС

CREATE TYPE response_status AS ENUM (
  'Добавлена','Получена службой','Принята','Не принята',
  'Начало реагирования','Прибытие','Проведение работ',
  'Работы завершены','Отказ от выполнения работ'
);

CREATE TABLE response_entries (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  card_id    uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  service_id text NOT NULL REFERENCES services(id),
  status     response_status NOT NULL,
  at         timestamptz NOT NULL DEFAULT now(),
  operator   text NOT NULL,                 -- 'оп. 0' или ФИО
  user_id    uuid REFERENCES users(id),
  order_no   text,
  comment    text
);
CREATE INDEX ON response_entries (card_id, service_id, at);
```

### 4.7 Разговор и оценка

```sql
CREATE TABLE transcript_lines (
  id         bigserial PRIMARY KEY,
  session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  who        text NOT NULL,                 -- 'оператор' | 'заявитель'
  text       text NOT NULL,
  at_sec     int NOT NULL                   -- смещение от старта занятия
);

CREATE TABLE revealed_facts (
  session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  fact_id    uuid NOT NULL REFERENCES scenario_hidden_facts(id),
  at_sec     int NOT NULL,
  PRIMARY KEY (session_id, fact_id)
);

CREATE TABLE grades (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  session_id  uuid UNIQUE NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  score       int NOT NULL,
  elapsed_sec int NOT NULL,
  norm_sec    int NOT NULL,
  over_norm   boolean NOT NULL,
  findings    jsonb NOT NULL,               -- Finding[]
  services    jsonb NOT NULL,               -- {expected,actual,missed,extra}
  created_at  timestamptz NOT NULL DEFAULT now()
);
```

### 4.8 Аудит и фоновые задачи

```sql
CREATE TABLE audit_log (
  id        bigserial,
  at        timestamptz NOT NULL DEFAULT now(),
  user_id   uuid REFERENCES users(id),
  action    text NOT NULL,                  -- 'card.save', 'grade.override'
  entity    text,
  entity_id text,
  payload   jsonb,
  ip        inet,
  PRIMARY KEY (id, at)
) PARTITION BY RANGE (at);
-- партиции по месяцам, ретенция не менее 6 месяцев (требование ТЗ)

CREATE TYPE job_status AS ENUM ('queued','running','done','failed');

CREATE TABLE ml_jobs (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  kind        text NOT NULL,     -- 'scenario.generate', 'insights', 'report'
  status      job_status NOT NULL DEFAULT 'queued',
  input       jsonb NOT NULL,
  output      jsonb,
  error       text,
  created_by  uuid REFERENCES users(id),
  created_at  timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz
);
```

---

## 5. Движок классификатора

Реализовано в `backend/src/domain/classify.rs`. **После переноса
на бэкенд фронтендовая копия удаляется**, иначе эталон разъедется.
Типы для фронта генерировать из серверных структур (`ts-rs` для Rust,
OpenAPI-кодоген для Python).

### 5.1 Алгоритм

```
classify(typeIds, answers) -> { outcome, services[], firedRules[] }

1. Отобрать правила, у которых typeId входит в typeIds.
2. Правило срабатывает, если ВСЕ его условия выполнены.
   Условие: для questionId хотя бы одно из выбранных значений
   входит в список допустимых.
3. services = объединение services всех сработавших правил.
4. outcome = outcome самого специфичного правила
   (специфичность = число условий в `when`).
```

Требование из инструкции, раздел 6: автоподобранную службу
**удалить нельзя**, добавить вручную можно. Поэтому `card_services.source`
различает `auto` и `manual`, и DELETE по `auto` запрещён на уровне API.

### 5.2 Импорт XLSX

Структура файла (проверено на обеих версиях из материалов):

* строка 1 — названия служб начиная с колонки 12 (59 именованных);
* строки 2–3 — уточнения условий («признак не выбран», «угроза людям»);
* строки 4+ — данные; строка без значения в колонке 0 при заполненной
  колонке 5 является заголовком группы;
* колонки: `0=Г, 1=п1, 2=п2, 3=п3, 4=Номер, 5=Группа, 6=Признак.1,
  7=Признак.2, 8=Признак.3, 9=Доп.признаки, 10=Итоговый тип, 11=ЕКП35, 12+=службы`.

Версия v046_24: **1283 позиции, 104 колонки, 23 группы**.

Библиотеки: `calamine` (Rust) или `openpyxl` (Python).

### 5.3 Дельта версий — ключевая функция продукта

```
diff(versionA, versionB) -> {
  addedServices[], removedServices[],
  addedPositions[], removedPositions[],
  changedPositions[{ number, field, before, after }],
  affectedScenarios[],         -- сценарии, чей expected больше не верен
  affectedUsers[]              -- кто аттестован по устаревшим правилам
}
```

Проверено на реальных файлах: между `v046_11` и `v046_24` появились
8 служб (в том числе ЦУКБ.БПЛА Минобороны, Минобороны РХБЗ, Ситиэнерго)
и позиция **24120200 «БПЛА»** с итоговым типом «БВС Регион».

Отсюда бизнес-логика: при активации новой версии система находит
курсантов, чья аттестация не покрывает новые или изменённые позиции,
и ставит им переаттестацию по дельте.

---

## 6. Машина статусов ДДС

Порт `response.ts`. Правила из памятки для ДДС:

```
nextStatuses(entries) -> ResponseStatus[]

нет содержательных статусов      → ['Принята', 'Не принята']
последний 'Не принята'           → ['Принята']
последний 'Принята'              → ['Начало реагирования','Прибытие',
                                     'Проведение работ','Работы завершены',
                                     'Отказ от выполнения работ']
последний в цепочке хода работ   → только статусы правее по цепочке
терминальный статус              → [] (карточка закрыта для редактирования)

requiresComment(status) = status ∈ {'Не принята', 'Отказ от выполнения работ'}
isTerminal(status)      = status ∈ {'Работы завершены', 'Отказ от выполнения работ'}
```

Константы: `ACK_NORM_SEC = 30`, `COMPLETION_NORM_HOURS = 48`.

**Серверная валидация обязательна.** Фронт ограничивает выбор, но статус
без комментария там, где он обязателен, должен отклоняться и на сервере
с кодом `422` и телом `{ "error": "comment_required" }`.

### Автоматические переходы карточки

**Реализовано** в `backend/src/domain/card_status.rs` и
`backend/src/jobs/watchdog.rs` — не набором переходов, а пересчётом
статуса из состояния служб. Порядок приоритетов выведен нами, в
материалах заказчика его нет: записан в OPEN-QUESTIONS.md, вопрос 10.

Фоновая задача раз в 5 секунд:

* служба не проставила первичный статус за 30 с → карточка `Не оповещено`;
* служба проставила `Не принята` или `Отказ от выполнения работ`
  → карточка `Отказ`;
* прошло 48 ч без `Работы завершены` → карточка `Не завершено`;
* все службы завершили → `Завершена`.

Каждый переход пишется в `audit_log` — это требование раздела «Аудит»
инструкции.

---

## 7. Движок оценки

Реализовано в `backend/src/domain/grade.rs`. Сравнивает **три слоя**: скрытую истину сценария,
стенограмму разговора и содержимое карточки.

```
grade(card, scenario, elapsedSec, revealedFactIds) -> Grade
```

Типы находок:

| kind | Когда |
|---|---|
| `ok` | совпало с эталоном и подтверждено разговором |
| `guessed` | **совпало, но заявитель этого не сообщал** |
| `wrong` | значение не совпало |
| `missing` | поле не заполнено / служба не оповещена |
| `extra` | лишняя служба |

Логика `guessed`:

```
provenFields  = ∪ unlocks(факты, которые курсант выяснил)
askableFields = ∪ unlocks(все скрытые факты сценария)

поле верное И поле ∈ askableFields И поле ∉ provenFields  →  guessed
```

Это единственная метрика, которую нельзя получить без сопоставления
трёх слоёв. На ней строится позиционирование продукта.

Каждая находка несёт **последствие**, а не балл:
«служба не оповещена — фактически не выехала», «силы уедут не на тот адрес».

Веса штрафа: `guessed=1, extra=1, missing=2, wrong=3`, превышение
норматива `+2`. Итог: `score = max(0, round(100 * (1 - penalty / (n*3))))`.

Подписи полей берутся из опросных карт, а не из идентификаторов —
курсант должен видеть ту же формулировку, что была на экране.

---

## 8. REST API

Префикс `/api`. Формат — JSON (требование ТЗ). Аутентификация —
`Authorization: Bearer <jwt>`. Все изменяющие запросы принимают
заголовок `Idempotency-Key` (нужен для восстановления после обрыва сети).

### 8.1 Аутентификация

```http
POST /api/auth/login
{ "login": "umc_operdds1", "password": "...", "workstation": "4" }
→ 200 { "token": "...", "user": { "id","fio","role","serviceId","workstation" } }
→ 401 { "error": "invalid_credentials" }

POST /api/auth/logout        → 204
GET  /api/auth/me            → 200 User
```

### 8.2 Справочники

```http
GET /api/catalog/incident-types   → IncidentType[]   (id,title,frequent,group)
GET /api/catalog/services         → Service[]
GET /api/catalog/survey-cards/:typeId → SurveyCard
GET /api/catalog/applicant-statuses   → string[]
```

Кэшируются на клиенте, инвалидация по `ETag` от активной версии классификатора.

### 8.3 Классификатор

```http
POST /api/classifier/versions            (multipart, xlsx)   [admin]
→ 202 { "jobId": "..." }

GET  /api/classifier/versions            → ClassifierVersion[]
GET  /api/classifier/versions/:a/diff/:b → ClassifierDiff
POST /api/classifier/versions/:id/activate [admin]
→ 200 { "affectedScenarios": 12, "affectedUsers": 34 }

POST /api/classifier/evaluate            ← ГОРЯЧИЙ ПУТЬ
{ "typeIds": ["101"], "answers": { "where": ["Дом"], "people-threat": ["Да"] } }
→ 200 { "outcome": "пожар: квартира",
        "services": ["s101","dep-jkh","s103","cemp"],
        "firedRules": [ ... ] }
```

`evaluate` вызывается фронтом после каждого нажатия плашки признака.
Требование — p95 ≤ 50 мс. Реализация: правила в памяти, без обращения к БД.

### 8.4 Сценарии

```http
GET  /api/scenarios?status=approved&group=1        → Scenario[]
GET  /api/scenarios/:id                            → Scenario

POST /api/scenarios/generate                       [teacher]
{ "groupNo": 13, "count": 5, "hints": "многоквартирные дома" }
→ 202 { "jobId": "..." }

POST /api/scenarios/:id/refine                     [teacher]
{ "comment": "слишком очевидно, пусть не называет этаж сразу" }
→ 202 { "jobId": "..." }

POST /api/scenarios/:id/approve                    [teacher] → 200 Scenario
PATCH /api/scenarios/:id                           [teacher]
DELETE /api/scenarios/:id                          [teacher]
```

`refine` — прямое требование ТЗ: «контекстное поле, куда преподаватель
может ввести комментарий, который должен быть отработан системой
для корректировки вопроса или ответа».

### 8.5 Занятие обучающегося

```http
POST /api/sessions
{ "scenarioId": "...", "mode": "card112" }
→ 201 { "session": {...}, "card": IncidentCard, "scenario": ScenarioPublic }
```

`ScenarioPublic` — сценарий **без** `expected` и без `answer` скрытых
фактов. Иначе курсант увидит эталон в DevTools. Отдаются только
`legend` и список `question` без ответов.

```http
POST /api/sessions/:id/ask
{ "factId": "city" }
→ 200 { "answer": "Нет, что вы, это Королёв...", "atSec": 47 }

PATCH /api/sessions/:id/card         (частичное обновление, автосохранение)
{ "address": { "settlement": "Королёв" }, "answers": { "gasified": ["Нет"] } }
→ 200 { "card": IncidentCard, "services": ["s101"] }

POST /api/sessions/:id/save
→ 200 Grade

GET  /api/sessions/:id/grade         → Grade
GET  /api/sessions/:id/transcript    → TranscriptLine[]
```

`PATCH` вызывается по дебаунсу 500 мс — это и есть буферизация
и восстановление после сбоя сети до 30 секунд.

### 8.6 Журнал и режим ДДС

```http
GET /api/cards?scope=service&status=&q=&page=&size=
```

`scope=service` — только карточки, где служба пользователя в списке
оповещения. Это и есть «в ленту попадают только профильные события».

```http
GET  /api/cards/:id                  → IncidentCard + responses

POST /api/cards/:id/responses
{ "serviceId": "dep-jkh", "status": "Принята",
  "orderNo": "23", "comment": "Направлена аварийная бригада" }
→ 201 ResponseEntry
→ 422 { "error": "comment_required" }
→ 422 { "error": "status_not_allowed", "allowed": ["Принята","Не принята"] }
```

### 8.7 Преподаватель

```http
GET  /api/groups                            → Group[]
POST /api/groups                            [teacher]
POST /api/groups/:id/members                [teacher]
POST /api/assignments                       [teacher]

GET  /api/monitor/live?groupId=             → активные занятия группы
GET  /api/reports/session/:id?format=pdf|csv|json
GET  /api/reports/group/:id?from=&to=       → 202 { jobId }
GET  /api/insights/group/:id                → 202 { jobId }
```

Отчёт по занятию содержит, как требует ТЗ: действия, замечания, время
заполнения карточки, отличие от норматива и результат проверки грамматики.

### 8.8 Администратор

```http
GET    /api/admin/health         → { db, jobs, ml, uptime, sessions }
GET    /api/admin/audit?from=&to=&userId=&action=
GET    /api/admin/users          [admin]
POST   /api/admin/users          [admin]
PATCH  /api/admin/users/:id      [admin]   (блокировка, смена роли/службы)
POST   /api/admin/backup         [admin]
GET    /api/admin/metrics        (Prometheus-формат)
```

Ограничения из ТЗ: администратор **не может** менять оценки и сценарии
во время активного занятия. Проверка: если у пользователя есть сессия
с `finished_at IS NULL`, изменяющие операции по его данным отклоняются
кодом `409`.

### 8.9 Коды ошибок

```
400 validation_error     — тело не прошло схему
401 unauthorized
403 forbidden            — роль или служба не позволяют
404 not_found
409 conflict             — активное занятие, версия изменилась
422 comment_required | status_not_allowed | auto_service_removal
429 rate_limited
500 internal
503 unavailable          — сервер голоса не подключён или не отвечает
```

---

## 9. WebSocket

Один канал `/ws`, аутентификация тем же JWT в query.

```
клиент → сервер
  { "type": "subscribe", "topics": ["session:<id>", "service:<id>", "group:<id>"] }

сервер → клиент
  { "type": "card.dispatched", "cardId", "number", "services": [...] }
  { "type": "response.recorded", "cardId", "serviceId", "status", "at" }
  { "type": "ack.timeout", "cardId", "serviceId" }        -- истекли 30 с
  { "type": "session.started", "userId", "scenarioId" }
  { "type": "session.finished", "sessionId", "score" }
  { "type": "controller.call", "cardId", "reason", "text" }  -- ИИ-контролёр
```

Это то, что делает класс живым: карточка, сохранённая курсантом-оператором,
немедленно прилетает курсанту-диспетчеру профильной ДДС, и у него
начинают тикать 30 секунд.

---

## 10. Адаптеры моделей и моки

**Правило:** ни один эндпоинт не зависит от модели напрямую. Всё через
интерфейсы. Переключение — переменной окружения `ML_PROVIDER=mock|onnx|http`.
В MVP по умолчанию `mock`, и система полностью работоспособна.

### 10.1 Интерфейсы

```ts
// Распознавание речи
interface Asr {
  transcribe(audio: Bytes, opts: { lang: 'ru' }): Promise<{
    text: string;
    segments: { text: string; startMs: number; endMs: number; conf: number }[];
  }>;
}

// Синтез речи
interface Tts {
  synthesize(text: string, voice: string): Promise<{ audioRef: string; durationMs: number }>;
}

// Классификация по закрытому словарю (NLI / логит-скоринг)
interface LabelScorer {
  score(text: string, candidates: string[], context?: string): Promise<
    { label: string; prob: number }[]
  >;
}

// Генеративная модель — только офлайн
interface Llm {
  generateScenario(input: {
    position: ClassifierPosition;
    examples: Scenario[];       // few-shot из билетов
    hints?: string;
  }): Promise<ScenarioDraft>;

  refineScenario(input: { scenario: Scenario; comment: string }): Promise<ScenarioDraft>;

  groupInsights(input: { stats: GroupStats }): Promise<{ text: string; recommendations: string[] }>;

  judgeComment(input: { comment: string; rubric: string[] }): Promise<
    { criterion: string; satisfied: boolean; reason: string }[]
  >;
}

// Проверка грамматики
interface Grammar {
  check(text: string): Promise<{
    span: [number, number]; kind: string; message: string; suggestion?: string;
  }[]>;
}
```

### 10.2 Поведение моков

Моки должны быть **детерминированными** — тесты не должны плавать.

| Мок | Что делает |
|---|---|
| `MockAsr` | возвращает заранее заданный текст из сценария; задержка настраивается `MOCK_ASR_DELAY_MS` (по умолчанию 300) |
| `MockTts` | возвращает путь к тишине нужной длительности (≈ 60 мс на слово); файл генерируется один раз |
| `MockLabelScorer` | подстрочное совпадение кандидата с текстом → `prob = 0.9`, иначе `0.1 + hash(label) % 10 / 100`; сортировка по убыванию |
| `MockLlm.generateScenario` | берёт шаблон из набора билетов, подставляет признаки позиции классификатора, генерирует 3–5 скрытых фактов по полям, которых нет в легенде |
| `MockLlm.refineScenario` | возвращает исходный сценарий с полем `refinedBy: comment`, легенда не меняется |
| `MockLlm.groupInsights` | собирает текст из **реальной статистики** БД по шаблону: «N из M не заполнили поле X» — выглядит правдоподобно и не врёт |
| `MockLlm.judgeComment` | критерий «указана причина» — есть ли в комментарии слово из списка причин; «куда передано» — есть ли предлог «в» + существительное с заглавной |
| `MockGrammar` | регулярки: двойные пробелы, отсутствие заглавной в начале, «тся/ться», пробел перед запятой |

Мок `groupInsights` важен отдельно: он **не выдумывает цифры**, а
пересказывает настоящую агрегацию. Демо выглядит честно, и при замене
на реальную модель контракт не меняется.

### 10.3 Когда что вызывается

```
Подготовка занятия (офлайн, минуты)
  Llm.generateScenario   → черновик сценария
  Llm.refineScenario     → правка по комментарию преподавателя
  Tts.synthesize         → банк реплик заявителя заранее
  ↓
Во время вызова (реальное время)
  Asr.transcribe         → речь курсанта
  LabelScorer.score      → подсказки копилота (опционально)
  ГЕНЕРАТИВНОЙ МОДЕЛИ НЕТ
  ↓
После вызова (секунды)
  Grammar.check          → свободный текст карточки
  Llm.judgeComment       → комментарии к статусам ДДС
  Llm.groupInsights      → сводка преподавателю
```

Такое разделение — прямое следствие железа из ТЗ. Подробности
в `IDEAS.md`, раздел 0.

---

## 11. Безопасность

* **Пароли** — argon2id, соль на запись.
* **Токены** — JWT, TTL 8 часов (смена), refresh не нужен для MVP.
  Требование инструкции: автоматический выход через 24 часа.
* **RBAC** — проверка на уровне хендлера, не на уровне UI.
  Матрица прав: студент видит только свои сессии и карточки своей службы;
  преподаватель — свои группы; админ — всё, кроме изменения оценок
  во время активного занятия.
* **TLS** — обязателен внутри контура; self-signed сертификат допустим,
  в конфиге путь к ключу и сертификату.
* **Аудит** — каждое изменяющее действие пишется в `audit_log`;
  партиции по месяцам, ретенция не менее 6 месяцев.
* **Персональные данные** — ФИО курсантов; в отчётах наружу
  выгружать обезличенно (ФЗ-152).
* **Rate limit** — 100 запросов/мин на пользователя, кроме `PATCH card`.

---

## 12. Голос (вне MVP, но заложить)

Медиа **не должно идти через бэкенд** — иначе 150 мс из ТЗ не удержать.

Рекомендация: WebRTC между браузером и медиасервером, бэкенд только
сигнализация. Для MVP голоса нет — разговор текстовый через банк вопросов,
и это полностью рабочий учебный режим.

Задел в API: `POST /api/sessions/:id/ask` уже возвращает `answer`,
достаточно добавить `audioRef` — и тот же эндпоинт начнёт отдавать
предрендеренную реплику.

---

## 13. Фоновые задачи

| Задача | Период | Что делает |
|---|---|---|
| `status.watchdog` | 5 с | переводит карточки в «Не оповещено» по истечении 30 с |
| `completion.watchdog` | 5 мин | «Не завершено» после 48 ч |
| `jobs.worker` | постоянно | очередь `ml_jobs`: генерация, инсайты, отчёты |
| `backup` | 1 раз в сутки | `pg_dump`, ретенция 30 дней |
| `audit.partition` | 1 раз в сутки | создаёт партицию на следующий месяц |

Очередь на MVP — таблица `ml_jobs` с `FOR UPDATE SKIP LOCKED`.
Брокер не нужен.

---

## 14. Наполнение данными

Порядок сидирования:

1. **Службы** — из шапки XLSX классификатора: **59 именованных колонок**
   начиная с 12-й. Часть из них разворачивается в несколько подколонок
   по условиям оповещения («признак не выбран», «угроза людям»,
   «пострадавшие») — отсюда 104 колонки всего. Колонка 12 «Главная служба»
   служебная, в ней код основной службы, а не отдельная ДДС.
2. **Классификатор** — `task/Классификатор_происшествий_v_046_24...xlsx`,
   1283 позиции. Вторым импортом — `refs/..._v_046_11...xlsx`,
   чтобы сразу работала функция дельты.
3. **Типы происшествий и опросные карты** — из
   опросные карты, снятые со скриншотов (миграция `20260920210002_survey.sql`,
   боевой системы: 101, 104, Взрыв, ДТП, БПЛА).
4. **Сценарии** — из `extracted/bilety_96_scenarios.md`,
   32 билета × 3 вызова = 96 сценариев. Три уже разобраны вручную
   в `scenarios.ts` и годятся как эталон разметки.
5. **Пользователи** — минимум: `admin`, `teacher1`,
   `student1..student5` с разными `service_id`
   (Деп. ЖКХ, Мослифт, Служба 101 — чтобы работал мультиплеер).

---

## 15. Границы MVP

**Входит:**

* аутентификация, роли, службы;
* классификатор с импортом XLSX, версиями и дельтой;
* опросные карты и `evaluate`;
* занятие в режиме карточки 112 с банком вопросов и оценкой;
* режим ДДС со статусами и валидацией на сервере;
* журнал с фильтром по своей службе;
* WebSocket класса;
* отчёт по занятию (JSON + CSV);
* аудит;
* все ИИ-функции за моками.

**Не входит:**

* SIP и реальный голос;
* PDF-сертификаты;
* интеграция с внешними системами (ВИС);
* мобильная версия;
* горизонтальное масштабирование (архитектурно заложено, не реализовано).

---

## 16. Порядок реализации

| Шаг | Что | Зачем именно в этом порядке |
|---|---|---|
| 1 | Схема БД, миграции, сидирование служб | без данных остальное не проверить |
| 2 | Импорт XLSX + `classifier.evaluate` | ядро эталона, от него зависит всё |
| 3 | Аутентификация и RBAC | дальше всё требует пользователя |
| 4 | Справочники + `GET /catalog/*` | фронт заводится на реальных данных |
| 5 | Сессия, карточка, автосохранение | основной учебный сценарий |
| 6 | Движок оценки и разбор | ключевая ценность продукта |
| 7 | Режим ДДС и статусы | второй сценарий ТЗ |
| 8 | WebSocket и watchdog | 30-секундный норматив оживает |
| 9 | Моки ИИ + генерация сценариев | закрывает «ИИ» в названии задачи |
| 10 | Дельта версий и переаттестация | главный отличающий признак |
| 11 | Отчёты, аудит, админка | добивка требований ТЗ |

Шаги 1–6 дают работающее демо. Всё остальное наращивается поверх.

---

## 17. Контракт с фронтендом

**Сделано в задаче №18 (21.09.2026).** Пакет `frontend/packages/domain`
удалён целиком: там лежали своя копия классификатора, своя оценка и
свои сценарии, ни разу не сверенные с ЕКП. Держать вторую реализацию
тех же правил нельзя — они расходятся, и курсанту показывают одно,
а в отчёт попадает другое.

Теперь формы ответов описаны в `frontend/apps/react/src/shared/api/types.ts`,
единственный вход к серверу — `shared/api`, а состояние занятия
(`entities/incident/model`) держит только то, что нужно нарисовать.

Заглушка `OWN_SERVICE = 'dep-jkh'` в карточке ДДС тоже убрана: служба
приезжает из профиля пользователя (`/api/auth/me`).
