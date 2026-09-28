# -*- coding: utf-8 -*-
"""Сколько ждёт отчёт по группе, когда очередь модели забита.

Норматив ТЗ на аналитический отчёт — 30 секунд. Замер нагрузки мерил
отчёт на пустой очереди; здесь — когда за спиной у отчёта сотня задач
анализа текста моделью, как бывает, когда класс разом сохранил
карточки.

Запуск: python3 отчёт-при-занятой-модели.py http://127.0.0.1:8185 <id группы>
"""
import json
import sys
import time
import urllib.request

БАЗА, ГРУППА = sys.argv[1], sys.argv[2]


def вызов(метод, путь, тело=None, токен=None):
    req = urllib.request.Request(БАЗА + путь, method=метод,
                                 data=json.dumps(тело).encode() if тело is not None else None,
                                 headers={"Content-Type": "application/json"})
    if токен:
        req.add_header("Authorization", "Bearer " + токен)
    with urllib.request.urlopen(req, timeout=120) as r:
        s = r.read()
        return json.loads(s) if s else None


for _ in range(30):
    try:
        вызов("GET", "/api/admin/health")
        break
    except Exception:
        time.sleep(1)

токен = вызов("POST", "/api/auth/login", {"login": "teacher1", "password": "arm112"})["token"]
t0 = time.time()
задача = вызов("POST", f"/api/reports/group/{ГРУППА}", None, токен)
jid = задача.get("jobId") or задача.get("id")
while True:
    j = вызов("GET", f"/api/jobs/{jid}", None, токен)
    if j.get("status") in ("done", "failed") or time.time() - t0 > 600:
        break
    time.sleep(0.2)
print(json.dumps({"статус": j.get("status"), "секунд": round(time.time() - t0, 2)}, ensure_ascii=False))
