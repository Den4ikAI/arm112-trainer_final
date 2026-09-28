# -*- coding: utf-8 -*-
"""Живой заявитель: время реплики, доля отбраковки, примеры.

Ходит в тренажёр по HTTP, как курсант: начинает вызов и задаёт вопросы
своими словами (`/ask-free`). Сравнивает ответ с заготовленной фразой
сценария: совпало — реплика отбракована или модель не успела; иначе —
живая. Причины отбраковки берёт из журнала действий.

Запуск (тренажёр с `applicant.live_model = 1` на своей базе):
  python3 проверить-живого-заявителя.py http://127.0.0.1:8177 arm112_test_live "Qwen3.8-27B (4090)"
"""
import json
import subprocess
import sys
import time
import urllib.request

БАЗА, ИМЯ_БАЗЫ, МОДЕЛЬ = sys.argv[1], sys.argv[2], sys.argv[3]

# Вопросы — как спросит оператор, не цитатой сценария. Последний в
# каждом вызове — не по делу, предпоследний — повтор уже выясненного
ВЫЗОВЫ = {
    "Алло! Горит крыша": ["А город какой у вас?", "Люди не пострадали?", "Газ в дом заведён?",
                          "Дом частный или многоквартирный?", "Огонь видно или только дым?",
                          "Так пострадавших нет, верно?", "Какой у вас номер паспорта?"],
    "Здравствуйте, у нас в доме мусоропровод": ["Огонь видите?", "Какой подъезд, код домофона?",
                                                "Никто не пострадал?", "Подъезд какой, повторите?",
                                                "Сколько вам лет?"],
    "Срочно! Горит мусорный контейнер": ["Пострадавшие есть?", "Газом не пахнет?",
                                          "Давно горит?", "Как вас зовут?", "Точный адрес назовите",
                                          "Пострадавших точно нет?", "Какая погода у вас?"],
}


def sql(q):
    out = subprocess.run(["docker", "exec", "arm112-db", "psql", "-U", "arm112", "-d", ИМЯ_БАЗЫ,
                          "-Atc", q], capture_output=True, text=True, check=True).stdout
    return [l for l in out.splitlines() if l]


def отбраковок(session):
    return int(sql(f"select count(*) from audit_log where action='applicant.fallback' "
                   f"and entity_id='{session}'")[0])


def запрос(метод, путь, тело=None, токен=None):
    req = urllib.request.Request(БАЗА + путь, method=метод,
                                 data=json.dumps(тело).encode() if тело is not None else None,
                                 headers={"Content-Type": "application/json",
                                          **({"Authorization": f"Bearer {токен}"} if токен else {})})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def главное():
    токен = запрос("POST", "/api/auth/login", {"login": "student4", "password": "arm112"})["token"]
    строки, времена, живых, всего = [], [], 0, 0
    for начало, вопросы in ВЫЗОВЫ.items():
        sid = sql(f"select id from scenarios where legend like '{начало}%' and status='approved' "
                  f"order by ticket nulls last limit 1")[0]
        заготовки = set(sql(f"select answer from scenario_hidden_facts where scenario_id='{sid}'"))
        занятие = запрос("POST", "/api/sessions", {"scenarioId": sid, "mode": "card112"}, токен)
        session = занятие["session"]["id"]
        строки.append(f"\n### {занятие['scenario']['legend'][:70]}…\n")
        for в in вопросы:
            было = отбраковок(session)
            t0 = time.time()
            r = запрос("POST", f"/api/sessions/{session}/ask-free", {"text": в}, токен)
            dt = time.time() - t0
            ответ = r["answer"]
            # Отбраковку видно по журналу действий: совпадение с
            # заготовкой ещё не значит отбраковку — модель могла сказать
            # то же самое
            живая = отбраковок(session) == было
            всего += 1
            живых += живая
            времена.append(dt)
            метка = ("_(отбракована — заготовка)_ " if not живая
                     else "_(модель слово в слово как сценарий)_ " if ответ in заготовки else "")
            строки.append(f"- **{в}** → {метка}{ответ}  "
                          f"<sub>{dt:.1f} с{'' if r['factId'] else ', вопрос не по делу'}</sub>")
        причины = sql(f"select (payload->>'reason') || ' — «' || coalesce(payload->>'rejected', '—') || '»' "
                      f"from audit_log where action='applicant.fallback' "
                      f"and entity_id='{session}' order by at")
        if причины:
            строки.append("\nОтбраковано:\n" + "\n".join(f"- {п}" for п in причины))
    времена.sort()
    print(f"## {МОДЕЛЬ}\n")
    print(f"Вопросов: {всего}, живых реплик: {живых}, заготовок: {всего - живых} "
          f"({(всего - живых) / всего:.0%})")
    print(f"Время ответа (сопоставление + реплика + проверка): медиана {времена[len(времена) // 2]:.2f} с, "
          f"максимум {времена[-1]:.2f} с")
    print("\n".join(строки))


if __name__ == "__main__":
    главное()
