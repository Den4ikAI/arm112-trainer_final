# -*- coding: utf-8 -*-
"""Наш собственный гигачат на той же задаче, со схемой JSON.

Вариантов восемь, ответ ограничен enum из ключей — модель физически
не выберет девятый. Отдельное значение «нет подходящего» закрывает
случай, когда заявителю про это сказать нечего.

Сравнение честное: тот же набор, что у Laya и GeRaCl.
"""
import importlib.util as u
import json
import subprocess
import sys
import time

s = u.spec_from_file_location("набор", "/media/denis/work4/llm/набор-вопросов.py")
набор = u.module_from_spec(s); s.loader.exec_module(набор)

АДРЕС = "http://127.0.0.1:8090/v1/chat/completions"
НЕТ = "нет_подходящего"


def спросить(запрос, факты):
    ключи = list(факты.keys()) + [НЕТ]
    перечень = "\n".join(f"  {к} — {факты[к]}" for к in факты)
    тело = {
        "model": "local",
        "temperature": 0,
        "max_tokens": 24,
        "messages": [
            {"role": "system", "content":
             "Оператор службы 112 разговаривает с заявителем. Определи, "
             "про какой из перечисленных фактов оператор сейчас спрашивает.\n"
             f"Факты:\n{перечень}\n"
             f"  {НЕТ} — оператор спрашивает про что-то другое, заявителю "
             "про это сказать нечего."},
            {"role": "user", "content": f"Вопрос оператора: {запрос}"},
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "выбор", "strict": True,
                "schema": {
                    "type": "object",
                    "properties": {"факт": {"type": "string", "enum": ключи}},
                    "required": ["факт"], "additionalProperties": False,
                },
            },
        },
    }
    out = subprocess.run(
        ["curl", "-s", АДРЕС, "-H", "content-type: application/json",
         "-d", json.dumps(тело, ensure_ascii=False)],
        capture_output=True, text=True).stdout
    try:
        ответ = json.loads(out)["choices"][0]["message"]["content"]
        return json.loads(ответ)["факт"]
    except Exception:
        return "(сбой)"


def главное() -> int:
    верных = 0
    пустых_верно = 0
    пустых = 0
    задержки = []

    for название, факты, проверки in набор.НАБОРЫ:
        print(f"══ {название} ({len(факты)} вариантов)")
        for запрос, цель in проверки:
            t = time.time()
            выбор = спросить(запрос, факты)
            задержки.append(time.time() - t)

            if цель is None:
                пустых += 1
                верно = выбор == НЕТ
                пустых_верно += верно
                знак = "+" if верно else "—"
            else:
                верно = выбор == цель
                верных += верно
                знак = "+" if верно else "—"

            ожидалось = цель if цель else НЕТ
            print(f"  {знак} {запрос:36} → {выбор:16} ждали: {ожидалось}")
        print()

    сопоставимых = sum(len(п) for _, _, п in набор.НАБОРЫ) - пустых
    print("── итог")
    print(f"   угадано: {верных} из {сопоставимых} "
          f"({верных / сопоставимых * 100:.0f} %)")
    print(f"   «нечего отвечать» распознано: {пустых_верно} из {пустых}")
    print(f"   задержка: медиана {sorted(задержки)[len(задержки)//2]:.1f} с, "
          f"худшая {max(задержки):.1f} с")
    print()
    print("   на этом же наборе: GeRaCl 81 %, Laya 59 %")
    return 0


if __name__ == "__main__":
    sys.exit(главное())
