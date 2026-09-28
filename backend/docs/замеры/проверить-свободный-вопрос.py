# -*- coding: utf-8 -*-
"""Свободный вопрос заявителю: тот же запрос, что в `HttpLabelScorer`.

Набор — `набор-вопросов.py` (34 проверки на двух учебных вызовах,
7 вопросов нарочно ни про какой факт). Запрос и схема дословно как
в `backend/src/ml/http.rs`: варианты — вопросы сценария под ключами
сценария (city, victims…), плюс «нет_подходящего». Переменная KEYS=f
подставляет номера f1…fN — так скорер делает, если ключей нет.

Запуск: python3 проверить-свободный-вопрос.py <адрес сервера> <имя>
"""
import importlib.util as u
import json
import statistics
import sys
import time
import os
import urllib.request
from pathlib import Path

s = u.spec_from_file_location("набор", Path(__file__).parent / "набор-вопросов.py")
набор = u.module_from_spec(s); s.loader.exec_module(набор)

НЕТ = "нет_подходящего"


def спросить(адрес, запрос, ключи, вопросы):
    перечень = "\n".join(f"  {к} — {в}" for к, в in zip(ключи, вопросы))
    тело = {
        "model": "local", "temperature": 0.2, "max_tokens": 24,
        "messages": [
            {"role": "system", "content":
             "Оператор службы 112 разговаривает с заявителем. Определи, "
             "про какой из перечисленных фактов оператор сейчас спрашивает.\n"
             f"Факты:\n{перечень}\n  {НЕТ} — оператор спрашивает про что-то "
             "другое, заявителю про это сказать нечего."},
            {"role": "user", "content": f"Вопрос оператора: {запрос}"},
        ],
        "response_format": {"type": "json_schema", "json_schema": {
            "name": "answer", "strict": True,
            "schema": {"type": "object",
                       "properties": {"fact": {"type": "string", "enum": ключи + [НЕТ]}},
                       "required": ["fact"], "additionalProperties": False}}},
    }
    req = urllib.request.Request(f"{адрес}/v1/chat/completions", data=json.dumps(тело).encode(),
                                 headers={"Content-Type": "application/json"})
    t = time.time()
    with urllib.request.urlopen(req, timeout=120) as r:
        ответ = json.loads(json.load(r)["choices"][0]["message"]["content"])["fact"]
    return ответ, time.time() - t


def главное():
    адрес, имя = sys.argv[1], sys.argv[2]
    верных = пустых = пустых_верно = 0
    задержки = []
    for название, факты, проверки in набор.НАБОРЫ:
        свои = list(факты)
        вопросы = [факты[к] for к in свои]
        ключи = ([f"f{i}" for i in range(1, len(свои) + 1)]
                 if os.environ.get("KEYS") == "f" else свои)
        for запрос, цель in проверки:
            выбор, dt = спросить(адрес, запрос, ключи, вопросы)
            задержки.append(dt)
            if цель is None:
                пустых += 1
                пустых_верно += выбор == НЕТ
            else:
                верных += выбор == ключи[свои.index(цель)]
    с_фактом = набор.всего() - набор.без_ответа()
    print(f"== {имя}")
    print(f"верно {верных} из {с_фактом}; «нечего отвечать» {пустых_верно} из {пустых}")
    print(f"задержка: медиана {statistics.median(задержки):.2f} с, "
          f"максимум {max(задержки):.2f} с")


if __name__ == "__main__":
    главное()
