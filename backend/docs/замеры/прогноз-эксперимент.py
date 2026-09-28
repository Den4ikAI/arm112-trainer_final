# -*- coding: utf-8 -*-
"""Контролируемая проверка прогноза: синтетические курсанты через API.

ЭТО НЕ ОЦЕНКА НА РЕАЛЬНЫХ КУРСАНТАХ. Настоящих курсантов у тренажёра ещё
не было, а на стенде в истории — следы автотестов (у student1 больше
тысячи вызовов по несколько секунд). Поэтому работоспособность прогноза
проверяется на курсантах, поведение которых задано скриптом: у каждого
свой уровень подготовки и свой темп роста. Уровень определяет, сколько
вопросов заявителю он задаёт и сколько полей карточки заполняет верно.

Всё идёт через настоящий API тренажёра, как у живого курсанта: вызов
берётся из журнала, прогноз фиксируется сервером в момент взятия вызова
(до ответов), факт — после сохранения. Скрипт только считает то, что
сервер записал, тем же способом, что и /api/forecast/group.

Запуск (свой экземпляр тренажёра на своей базе, не стенд):
  python3 прогноз-эксперимент.py http://127.0.0.1:8185 arm112_audit
"""
import json
import random
import subprocess
import sys
import time
import urllib.error
import urllib.request

БАЗА_URL, БАЗА = sys.argv[1], sys.argv[2]
СИД = 20260929
КУРСАНТОВ = 10
ВЫЗОВОВ = 14
ПАУЗА_ДЛЯ_ОБУЧЕНИЯ = 70  # модель переобучается в фоне раз в минуту

random.seed(СИД)


def api(метод, путь, тело=None, токен=None):
    данные = json.dumps(тело).encode() if тело is not None else None
    req = urllib.request.Request(f"{БАЗА_URL}{путь}", data=данные, method=метод,
                                 headers={"Content-Type": "application/json"})
    if токен:
        req.add_header("Authorization", f"Bearer {токен}")
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            текст = r.read()
            return json.loads(текст) if текст else None
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"{метод} {путь}: {e.code} {e.read()[:300]!r}") from None


def sql(запрос):
    вывод = subprocess.run(["docker", "exec", "arm112-db", "psql", "-U", "arm112", "-d", БАЗА,
                            "-Atc", запрос], capture_output=True, text=True, check=True)
    return вывод.stdout.strip()


def вход(логин, пароль="arm112"):
    return api("POST", "/api/auth/login", {"login": логин, "password": пароль})["token"]


# Утверждённые вызовы и их эталон — из базы: скрипт играет курсанта,
# которому эталон известен ровно настолько, насколько он «подготовлен»
СЦЕНАРИИ = []
for строка in sql("select id, expected::text from scenarios where status='approved' "
                  "and ticket is not null order by ticket").splitlines():
    sid, эталон = строка.split("|", 1)
    СЦЕНАРИИ.append((sid, json.loads(эталон)))

админ = вход("admin")
метка = str(int(time.time()))
группа = sql(f"insert into training_groups (title, teacher_id) select 'Проверка прогноза {метка}', "
             f"id from users where login='teacher1' returning id").splitlines()[0]

курсанты = []
for i in range(КУРСАНТОВ):
    логин = f"прогноз-{метка}-{i}"
    старт = random.uniform(0.25, 0.9)      # исходная подготовка
    рост = random.uniform(-0.01, 0.04)     # за вызов: кто-то растёт, кто-то нет
    user = api("POST", "/api/admin/users", {"login": логин, "password": "длинныйПароль1",
               "fio": f"Проверка П.{i}", "role": "student", "duty": "op112"}, админ)
    sql(f"insert into group_members (group_id, user_id) values ('{группа}', '{user['id']}')")
    курсанты.append({"логин": логин, "токен": вход(логин, "длинныйПароль1"),
                     "уровень": старт, "рост": рост})


def пройти(к):
    sid, эталон = random.choice(СЦЕНАРИИ)
    p = max(0.05, min(0.98, к["уровень"] + random.gauss(0, 0.08)))
    т = к["токен"]
    начато = api("POST", "/api/sessions", {"scenarioId": sid, "mode": "card112"}, т)
    сессия = начато["session"]["id"]
    for q in начато["scenario"]["questions"]:
        if random.random() < p:
            api("POST", f"/api/sessions/{сессия}/ask", {"factId": q["factId"]}, т)
    ответы = {k: v for k, v in эталон.get("answers", {}).items() if random.random() < p}
    адрес = {k: v for k, v in эталон.get("address", {}).items() if random.random() < p}
    правка = {"applicantName": "Заявитель", "address": адрес,
              "description": "Со слов заявителя: происшествие по адресу." if random.random() < p else "",
              "typeIds": эталон.get("typeIds", []) if random.random() < 0.9 else [],
              "answers": ответы}
    api("PATCH", f"/api/sessions/{сессия}/card", правка, т)
    итог = api("POST", f"/api/sessions/{сессия}/save", None, т)
    к["уровень"] = max(0.05, min(0.98, к["уровень"] + к["рост"]))
    return (итог.get("grade") or {}).get("score")


# Первые вызовы: история набирается, прогнозов ещё нет (меньше 3 разборов)
# и модели не на чем учиться. Пауза — чтобы фоновое переобучение успело
for шаг in range(ВЫЗОВОВ):
    for к in курсанты:
        пройти(к)
    if шаг in (4, 8):
        time.sleep(ПАУЗА_ДЛЯ_ОБУЧЕНИЯ)

учитель = вход("teacher1")
отчёт = api("GET", f"/api/forecast/group/{группа}", None, учитель)
a = отчёт["accuracy"]

# Разбивка: обученная модель против запасного прогноза до обучения
разбивка = sql(f"""select p.trained, count(*), round(avg(abs(p.predicted-p.actual))::numeric,1),
  round(avg((abs(p.predicted-p.actual)<={a['tolerance']})::int)::numeric,2),
  round(avg(abs(coalesce(round((select avg(g.score) from grades g join sessions s on s.id=g.session_id
     where s.user_id=p.user_id and g.created_at<p.made_at)), p.actual)-p.actual))::numeric,1)
  from predictions p join group_members m on m.user_id=p.user_id
  where m.group_id='{группа}' and p.checked_at is not null group by p.trained order by 1""")

итог = {"курсантов": КУРСАНТОВ, "вызовов_на_курсанта": ВЫЗОВОВ, "сид": СИД,
        "группа": группа, "достоверность": a,
        "по_обученности": [dict(zip(["обучена", "прогнозов", "ошибка", "в_допуске", "ошибка_базовой"],
                                    r.split("|"))) for r in разбивка.splitlines()]}
print(json.dumps(итог, ensure_ascii=False, indent=1))
