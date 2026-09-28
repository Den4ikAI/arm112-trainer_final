# -*- coding: utf-8 -*-
"""Время одной реплики заявителя без ограничения по времени — ЦП против GPU.

Тот же запрос, что делает `domain/live_applicant.rs` (сокращённый
системный промпт, схема с лимитом длины), к двум серверам модели.

Запуск: python3 одна-реплика.py
"""
import json
import time
import urllib.request

СИСТЕМА = ("Ты — заявитель, который звонит в службу 112. Отвечаешь оператору одной-двумя короткими "
           "фразами, как живой взволнованный человек по телефону: своими словами. Сведения — только "
           "из разделов «Что ты знаешь» и «Ответ на этот вопрос». Ничего не придумывай.")
ВОПРОС = ("Что ты знаешь:\nТы: Иванова Инна Степановна (очевидец).\nТвоя первая фраза в звонке: Алло! "
          "Горит крыша частного дома! Пожалуйста, приезжайте скорее, это на Станционной улице, дом 28!\n\n"
          "Оператор спрашивает: А город какой у вас?\n\nОтвет на этот вопрос (суть обязательна): Нет, что вы, "
          "это Королёв, Московская область! Улица Станционная, 28.\n\nТвоя реплика:")
СХЕМА = {"type": "object", "additionalProperties": False, "required": ["reply"],
         "properties": {"reply": {"type": "string", "minLength": 1, "maxLength": 240}}}

for адрес, имя in [("http://127.0.0.1:8090", "GigaChat 3.1, ЦП"), ("http://127.0.0.1:8091", "Qwen3.8-27B, RTX 4090")]:
    тело = {"model": "local", "temperature": 0.2, "max_tokens": 200,
            "messages": [{"role": "system", "content": СИСТЕМА}, {"role": "user", "content": ВОПРОС}],
            "response_format": {"type": "json_schema",
                                "json_schema": {"name": "answer", "strict": True, "schema": СХЕМА}}}
    t0 = time.time()
    req = urllib.request.Request(адрес + "/v1/chat/completions", data=json.dumps(тело).encode(),
                                 headers={"Content-Type": "application/json"})
    ответ = json.load(urllib.request.urlopen(req, timeout=300))
    print(f"{имя}: {time.time() - t0:.1f} с — {ответ['choices'][0]['message']['content']}")
