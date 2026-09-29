# -*- coding: utf-8 -*-
"""Готовит учебный набор: по описаниям из темы.json модель тренажёра
пишет вызовы 112 и происшествия ДДС — тем же путём, что кнопка
«Создать по описанию (ИИ)» у преподавателя. Результат — учебный-набор.json,
который потом одинаково заливается на любой стенд (залить.py).

Запуск: python3 сгенерировать.py http://127.0.0.1:8082
"""
import json
import sys
import time
import urllib.request
from pathlib import Path

ТУТ = Path(__file__).parent
БАЗА = sys.argv[1].rstrip("/")


def запрос(метод, путь, тело=None, токен=None):
    req = urllib.request.Request(БАЗА + путь, method=метод,
                                 data=json.dumps(тело).encode() if тело is not None else None,
                                 headers={"Content-Type": "application/json",
                                          **({"Authorization": f"Bearer {токен}"} if токен else {})})
    with urllib.request.urlopen(req, timeout=600) as r:
        сырое = r.read()
        return json.loads(сырое) if сырое else None


токен = запрос("POST", "/api/auth/login", {"login": "teacher1", "password": "arm112"})["token"]
темы = json.loads((ТУТ / "темы.json").read_text(encoding="utf-8"))["темы"]
итог = {"вызовы": [], "происшествия": []}
for т in темы:
    for i, описание in enumerate(т["112"], 1):
        t0 = time.time()
        с = запрос("POST", "/api/scenarios/from-prompt",
                   {"title": f"{т['название']} — вызов {i}", "prompt": описание}, токен)
        print(f"112 {т['id']}-{i}: {time.time()-t0:.0f} с, легенда: {с.get('legend','')[:70]!r}", flush=True)
        итог["вызовы"].append({"тема": т["id"], "описание": описание, "сценарий": с})
    for i, описание in enumerate(т["ДДС"], 1):
        t0 = time.time()
        п = запрос("POST", "/api/dds-incidents/from-prompt",
                   {"title": f"{т['название']} — происшествие {i}", "prompt": описание}, токен)
        print(f"ДДС {т['id']}-{i}: {time.time()-t0:.0f} с, типы {п.get('typeIds')}, службы {п.get('services')}", flush=True)
        итог["происшествия"].append({"тема": т["id"], "описание": описание, "происшествие": п})
(ТУТ / "учебный-набор.json").write_text(json.dumps(итог, ensure_ascii=False, indent=1), encoding="utf-8")
print("готово")
