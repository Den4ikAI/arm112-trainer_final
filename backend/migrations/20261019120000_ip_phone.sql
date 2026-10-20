-- Финал: IP-телефон диспетчера ДДС (обратная связь экспертов 18.10).
--
-- В ТЗ — «система эмуляции вызовов с использованием IP-телефонии»,
-- «программная эмуляция IP-телефона» на рабочем месте и запись учебных
-- вызовов в WAV/MP3. Звонки старшему бригады и начальнику в тренажёре
-- уже были, но кнопками в разных местах карточки. Теперь это один
-- телефон: входящие с АОН, набор по справочнику, журнал вызовов и
-- запись разговора, которую слушает преподаватель.

-- Вызов: кто, кому, по какой карточке и чем кончился. Сам разговор
-- (что узнал диспетчер, что доложил) по-прежнему пишут бригада и
-- доклад начальнику — журнал телефона их не заменяет.
CREATE TABLE phone_calls (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    card_id     uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    service_id  text NOT NULL,
    -- brigade — старший бригады, chief — вышестоящий начальник
    peer        text NOT NULL CHECK (peer IN ('brigade', 'chief')),
    direction   text NOT NULL CHECK (direction IN ('in', 'out')),
    -- Внутренний номер собеседника: АОН входящего или набранный
    number      text NOT NULL,
    outcome     text NOT NULL DEFAULT 'ringing'
                CHECK (outcome IN ('ringing', 'talking', 'ended', 'missed', 'declined')),
    started_at  timestamptz NOT NULL DEFAULT now(),
    answered_at timestamptz,
    ended_at    timestamptz
);
CREATE INDEX phone_calls_card ON phone_calls (card_id, started_at);
CREATE INDEX phone_calls_user ON phone_calls (user_id, started_at);

-- Реплики вызова. Голос диспетчера — как записал микрофон (WAV 16 кГц
-- моно); реплики собеседника звук не хранят — запись разговора
-- озвучивает их заново тем же голосом (сервер голоса их кэширует).
CREATE TABLE phone_call_lines (
    id      bigserial PRIMARY KEY,
    call_id uuid NOT NULL REFERENCES phone_calls(id) ON DELETE CASCADE,
    who     text NOT NULL CHECK (who IN ('dispatcher', 'peer')),
    text    text NOT NULL,
    audio   bytea,
    at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX phone_call_lines_call ON phone_call_lines (call_id, id);

-- Параметры телефонии — у администратора в «Параметрах», как велит ТЗ
-- («настраивать параметры работы виртуальной IP-телефонии»)
INSERT INTO settings (key, value, title, hint) VALUES
    ('ui.phone', '1', 'IP-телефон диспетчера ДДС',
     '1 — звонки бригаде и начальнику идут через IP-телефон на карточке, 0 — кнопками в карточке, как раньше'),
    ('phone.record', '1', 'Запись разговоров IP-телефона',
     '1 — голос диспетчера сохраняется, преподаватель слушает разговор целиком; 0 — только текст реплик'),
    ('phone.ring_sec', '20', 'Входящий вызов звонит, с',
     'Сколько секунд звонит входящий вызов старшего бригады, прежде чем станет пропущенным'),
    ('phone.answer_sec', '2', 'Абонент снимает трубку через, с',
     'Сколько секунд длятся гудки исходящего вызова до ответа собеседника')
ON CONFLICT (key) DO NOTHING;
