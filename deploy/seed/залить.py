# -*- coding: utf-8 -*-
"""Чистый учебный набор на стенде.

Очищает учебные данные (карточки, вызовы, занятия, попытки, группы,
учётные записи, кроме администратора) и заводит заново:
  * преподаватель teacher1;
  * 2 группы операторов 112 по 5 человек и 2 группы диспетчеров ДДС по 5;
  * 20 вызовов 112 и 20 учебных происшествий ДДС (по 2 на каждую из 10 тем),
    утверждённых, но никому не выданных;
  * по 2 экзамена и 2 блица для каждой роли — созданы, не запущены.

Тексты вызовов и происшествий — учебный-набор.json (их написала модель
тренажёра по описаниям из темы.json), признаки опросной карты —
эталоны.json; службы и итоговый тип считает классификатор.

Пароль всех учётных записей — как у администратора (arm112 на стендах).

Запуск: python3 залить.py <адрес API> <имя базы>
  например: python3 залить.py http://127.0.0.1:8080 arm112
"""
import json
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

ТУТ = Path(__file__).parent
БАЗА, ИМЯ_БАЗЫ = sys.argv[1].rstrip("/"), sys.argv[2]

ОЧИСТИТЬ = """
TRUNCATE advice, assignments, brigade_calls, brigade_reports, brigade_assignments,
         chief_report_lines, chief_reports, expert_grades, text_analyses, grades,
         predictions, revealed_facts, transcript_lines, sessions, response_entries,
         card_services, lesson_cards, lessons, cards, scenario_chat,
         scenario_hidden_facts, scenarios, ml_jobs, group_members, training_groups,
         user_backups, audit_log CASCADE;
DELETE FROM users WHERE login <> 'admin';
"""

# Операторы 112 — без службы; диспетчеры ДДС — со службой. Темы учебного
# набора подобраны под эти службы: пожары и мусоропровод — 101, газ — 104,
# ДТП и «плохо» — 103 и ЦОДД, лифт — Мослифт, затопление — Деп. ЖКХ
ГРУППЫ = [
    ("Операторы 112 — группа 1", "op112", None, [
        ("op01", "Андреев Кирилл Сергеевич"), ("op02", "Белова Ольга Игоревна"),
        ("op03", "Васильев Денис Павлович"), ("op04", "Григорьева Анна Юрьевна"),
        ("op05", "Дмитриев Артём Олегович")]),
    ("Операторы 112 — группа 2", "op112", None, [
        ("op06", "Егорова Мария Андреевна"), ("op07", "Жуков Илья Викторович"),
        ("op08", "Зайцева Екатерина Николаевна"), ("op09", "Ильин Максим Романович"),
        ("op10", "Калинина Софья Алексеевна")]),
    ("Диспетчеры ДДС — группа 1", "dds", None, [
        ("dds01", "Лебедев Роман Игоревич", "mchs"), ("dds02", "Морозова Дарья Сергеевна", "mosgaz"),
        ("dds03", "Николаев Павел Андреевич", "smp"), ("dds04", "Орлова Виктория Олеговна", "codd"),
        ("dds05", "Павлов Никита Евгеньевич", "moslift")]),
    ("Диспетчеры ДДС — группа 2", "dds", None, [
        ("dds06", "Романова Алина Дмитриевна", "mchs"), ("dds07", "Сергеев Глеб Викторович", "mosgaz"),
        ("dds08", "Тихонова Полина Ивановна", "smp"), ("dds09", "Фёдоров Егор Максимович", "codd"),
        ("dds10", "Яковлева Ксения Павловна", "dep-zhkh")]),
]


def sql(текст):
    r = subprocess.run(["docker", "exec", "-i", "arm112-db", "psql", "-U", "arm112", "-d", ИМЯ_БАЗЫ,
                        "-v", "ON_ERROR_STOP=1", "-Atq"], input=текст, capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit(f"SQL: {r.stderr}")
    return r.stdout.strip()


def лит(s):
    return "NULL" if s is None else "'" + str(s).replace("'", "''") + "'"


def api(метод, путь, тело=None, токен=None):
    req = urllib.request.Request(БАЗА + путь, method=метод,
                                 data=json.dumps(тело).encode() if тело is not None else None,
                                 headers={"Content-Type": "application/json",
                                          **({"Authorization": f"Bearer {токен}"} if токен else {})})
    try:
        with urllib.request.urlopen(req, timeout=300) as r:
            сырое = r.read()
            return json.loads(сырое) if сырое else None
    except urllib.error.HTTPError as e:
        raise SystemExit(f"{метод} {путь}: {e.code} {e.read().decode()[:400]}")


def адрес(описание):
    """Адрес эталона из описания темы: «Москва, улица …, дом N, корпус K».
    Разбираем сами: адрес — то, что сверяет разбор, и он должен быть точным."""
    import re
    хвост = описание[описание.find("Москва, ") + len("Москва, "):] if "Москва, " in описание else ""
    части = [ч.strip(" .") for ч in хвост.split(",")]
    итог = {"region": "Москва"}
    if части:
        улица = части[0]
        if улица.startswith("перекрёсток") or улица.startswith("МКАД") or "у " in улица[:2]:
            итог["descriptive"] = ", ".join(части).strip()
        else:
            итог["street"] = улица
    for ч in части[1:]:
        m = re.match(r"(?:остановка )?(?:у |напротив )?дом(?:а)? (\S+)", ч)
        if m and "house" not in итог:
            итог["house"] = m.group(1)
        m = re.match(r"корпус (\S+)", ч)
        if m:
            итог["building"] = m.group(1)
        if not re.match(r"(?:у |напротив )?дом|корпус|подъезд|квартира|остановка", ч) and "street" in итог and ч:
            итог.setdefault("descriptive", ч)
    # Этажность — не адрес: она в фактах вызова
    if "этаж" in итог.get("descriptive", ""):
        итог.pop("descriptive")
    # «Садовое кольцо, у дома 12 по улице Земляной Вал» — дом стоит на Земляном Валу
    if итог.get("street") == "Садовое кольцо":
        итог.update({"street": "улица Земляной Вал", "house": "12", "descriptive": "Садовое кольцо"})
    return итог


def люди():
    sql(ОЧИСТИТЬ)
    хеш = sql("SELECT password_hash FROM users WHERE login = 'admin'")
    преп = sql(f"""INSERT INTO users (login, password_hash, fio, role)
                   VALUES ('teacher1', {лит(хеш)}, 'Смирнова Ольга Петровна', 'teacher') RETURNING id""")
    группы = {}
    for название, duty, _, люди_ in ГРУППЫ:
        gid = sql(f"INSERT INTO training_groups (title, teacher_id) VALUES ({лит(название)}, '{преп}') RETURNING id")
        группы[название] = gid
        for i, человек in enumerate(люди_, 1):
            login, fio = человек[0], человек[1]
            служба = человек[2] if len(человек) > 2 else None
            uid = sql(f"""INSERT INTO users (login, password_hash, fio, role, service_id, duty, workstation)
                          VALUES ({лит(login)}, {лит(хеш)}, {лит(fio)}, 'student', {лит(служба)},
                                  {лит(duty)}, {лит(str(i))}) RETURNING id""")
            sql(f"INSERT INTO group_members (group_id, user_id) VALUES ('{gid}', '{uid}')")
    return группы


def материал(токен):
    набор = json.loads((ТУТ / "учебный-набор.json").read_text(encoding="utf-8"))
    эталоны = json.loads((ТУТ / "эталоны.json").read_text(encoding="utf-8"))
    вручную = эталоны.get("_ддс_вручную", {})
    темы = {т["id"]: т["название"] for т in json.loads((ТУТ / "темы.json").read_text(encoding="utf-8"))["темы"]}
    счёт = {}
    вызовы, происшествия = [], []
    for v in набор["вызовы"]:
        т = v["тема"]; счёт[("112", т)] = счёт.get(("112", т), 0) + 1; n = счёт[("112", т)]
        с, э = v["сценарий"], эталоны[f"{т}-{n}"]
        тело = {
            "title": f"{темы[т]} — вызов {n}", "legend": с["legend"],
            "applicantName": с.get("applicantName", ""), "applicantPhone": с.get("applicantPhone", ""),
            "applicantStatus": с.get("applicantStatus", "очевидец"), "normSec": с.get("normSec"),
            "typeIds": э["typeIds"], "answers": э["answers"], "address": адрес(v["описание"]),
            "hidden": [{"key": h["key"], "question": h["question"], "answer": h["answer"],
                        "unlocks": h.get("unlocks", [])} for h in с.get("hidden", [])],
        }
        новый = api("POST", "/api/scenarios/manual", тело, токен)
        api("POST", f"/api/scenarios/{новый['id']}/approve", None, токен)
        вызовы.append((т, новый["id"]))
    счёт = {}
    for v in набор["происшествия"]:
        т = v["тема"]; счёт[("ДДС", т)] = счёт.get(("ДДС", т), 0) + 1; n = счёт[("ДДС", т)]
        п = v["происшествие"]
        э = эталоны.get(f"ДДС:{т}-{n}") or эталоны[f"{т}-{n}"]
        тело = {
            "title": f"{темы[т]} — происшествие {n}",
            "applicantName": п.get("applicantName", ""), "applicantStatus": п.get("applicantStatus", "очевидец"),
            "phoneAon": п.get("phoneAon", ""), "address": п.get("address"),
            "description": п.get("description", v["описание"]), "victims": п.get("victims"),
            "typeIds": э["typeIds"], "answers": э["answers"],
            "extraServices": вручную.get(т, []),
        }
        новое = api("POST", "/api/dds-incidents", тело, токен)
        api("POST", f"/api/dds-incidents/{новое['id']}/approve", None, токен)
        происшествия.append((т, новое["id"]))
    return вызовы, происшествия


def занятия(токен, группы, вызовы, происшествия):
    г = list(группы.values())
    ид = lambda пары, темы_: [i for т, i in пары if т in темы_]

    def по_номеру(пары, темы_, n):
        итог = []
        for тема in темы_:
            из_темы = [i for т, i in пары if т == тема]
            итог.append(из_темы[n])
        return итог
    первые = lambda пары, темы_: по_номеру(пары, темы_, 0)
    вторые = lambda пары, темы_: по_номеру(пары, темы_, 1)
    пожары = ["пожар-дом", "пожар-улица", "мусоропровод", "газ", "человек-опасность"]
    дороги = ["дтп-пострадавшие", "дтп-без", "лифт", "затопление", "плохо"]
    план = [
        # Оператор 112: экзамены по 5 вызовов, блицы на отдельные части карточки
        {"groupId": г[0], "title": "Экзамен 112: пожары, газ, человек в опасности", "mode": "card112",
         "kind": "marathon", "scenarioIds": ид(вызовы, пожары)[:5]},
        {"groupId": г[1], "title": "Экзамен 112: ДТП, лифт, затопление, медицина", "mode": "card112",
         "kind": "marathon", "scenarioIds": ид(вызовы, дороги)[:5]},
        {"groupId": г[0], "title": "Тренировка по частям: адрес", "mode": "card112",
         "kind": "blitz", "segment": "address", "scenarioIds": ид(вызовы, пожары + дороги)[:5]},
        {"groupId": г[1], "title": "Тренировка по частям: службы", "mode": "card112",
         "kind": "blitz", "segment": "services", "scenarioIds": ид(вызовы, дороги + пожары)[:5]},
        # Диспетчер ДДС. Статусы происшествия хранятся на карточку и службу:
        # одно происшествие в двух занятиях делило бы статус между ними.
        # Поэтому каждому занятию — свои происшествия: экзаменам первые по
        # теме, блицам — вторые
        {"groupId": г[2], "title": "Экзамен ДДС: пожары, газ, ДТП, лифт, медицина", "mode": "dds", "kind": "marathon",
         "cardIds": первые(происшествия, ["пожар-дом", "газ", "дтп-пострадавшие", "лифт", "плохо"])},
        {"groupId": г[3], "title": "Экзамен ДДС: улица, мусоропровод, затопление, ДТП, человек", "mode": "dds", "kind": "marathon",
         "cardIds": первые(происшествия, ["пожар-улица", "мусоропровод", "затопление", "дтп-без", "человек-опасность"])},
        {"groupId": г[2], "title": "Тренировка по частям: статусы по докладам", "mode": "dds", "kind": "blitz",
         "segment": "statuses", "cardIds": вторые(происшествия, ["пожар-дом", "газ", "лифт"])},
        {"groupId": г[3], "title": "Тренировка по частям: статусы по докладам (2)", "mode": "dds", "kind": "blitz",
         "segment": "statuses", "cardIds": вторые(происшествия, ["мусоропровод", "затопление", "плохо"])},
    ]
    for тело in план:
        api("POST", "/api/lessons", тело, токен)
        print("  занятие:", тело["title"])


if __name__ == "__main__":
    группы = люди()
    print("люди: 1 преподаватель, 4 группы, 20 курсантов")
    токен = api("POST", "/api/auth/login", {"login": "teacher1", "password": "arm112"})["token"]
    вызовы, происшествия = материал(токен)
    print(f"материал: {len(вызовы)} вызовов 112, {len(происшествия)} происшествий ДДС — утверждены")
    занятия(токен, группы, вызовы, происшествия)
    print("готово")
