# -*- coding: utf-8 -*-
"""OpenJEV на той же задаче: вопрос оператора → скрытый факт вызова.

Кросс-энкодер NLI: на вход посылка и гипотеза, на выход — следует,
противоречит или нейтрально. Для нашей задачи посылка — вопрос
оператора, гипотеза — «оператор спрашивает про <тему>». Побеждает
тема с наибольшей вероятностью следования.

Модель помечена как английская, база — Qwen3.5, он многоязычный.
Проверяем, что из этого выйдет на русском.

Набор тот же, что у Laya, GeRaCl и гигачата.
"""
import importlib.util as u
import os
import sys
import time

os.environ.setdefault("HF_HOME", "/media/denis/work4/llm/hf-cache")

s = u.spec_from_file_location("набор", "/media/denis/work4/llm/набор-вопросов.py")
набор = u.module_from_spec(s); s.loader.exec_module(набор)

import torch
from transformers import AutoModelForSequenceClassification, AutoTokenizer

ПУТЬ = "/media/denis/work4/llm/openjev/qwen3.5-0.8b-nli-v2s-long"
СЛЕДУЕТ = 1      # id2label: 0 contradiction, 1 entailment, 2 neutral


def главное() -> int:
    torch.set_num_threads(6)      # столько же, сколько на целевом стенде

    print("поднимаю OpenJEV 0.8B…", flush=True)
    t0 = time.time()
    tok = AutoTokenizer.from_pretrained(ПУТЬ)
    model = AutoModelForSequenceClassification.from_pretrained(
        ПУТЬ, dtype=torch.float32).eval()
    print(f"готова за {time.time() - t0:.0f} с\n", flush=True)

    верных = 0
    пустых = 0
    задержки = []
    лучшие_ответов, лучшие_пустых = [], []
    исходы = []      # (следование, был ли ответ верным)

    for название, факты, проверки in набор.НАБОРЫ:
        print(f"══ {название} ({len(факты)} вариантов)")
        ключи = list(факты.keys())
        гипотезы = [f"Оператор спрашивает про: {набор.ТЕМЫ[к]}" for к in ключи]

        for запрос, цель in проверки:
            t = time.time()
            пары = [f"Premise: {запрос}\nHypothesis: {г}" for г in гипотезы]
            партия = tok(пары, return_tensors="pt", padding=True, truncation=True)
            with torch.no_grad():
                logits = model(**партия).logits
            следование = torch.softmax(logits, dim=-1)[:, СЛЕДУЕТ].tolist()
            задержки.append((time.time() - t) * 1000)

            лучший = max(range(len(следование)), key=lambda i: следование[i])
            выбор = ключи[лучший]
            оценка = следование[лучший]

            if цель is None:
                пустых += 1
                лучшие_пустых.append(оценка)
                знак = "·"
            else:
                верно = выбор == цель
                лучшие_ответов.append(оценка)
                исходы.append((оценка, верно))
                верных += верно
                знак = "+" if верно else "—"

            ожидалось = цель if цель else "(нечего отвечать)"
            print(f"  {знак} {запрос:36} → {выбор:10} следование {оценка:.2f}"
                  f"  ждали: {ожидалось}")
        print()

    сопоставимых = sum(len(п) for _, _, п in набор.НАБОРЫ) - пустых
    сред = lambda v: sum(v) / len(v) if v else 0.0
    print("── итог")
    print(f"   угадано: {верных} из {сопоставимых} "
          f"({верных / сопоставимых * 100:.0f} %)")
    print(f"   следование: на осмысленных {сред(лучшие_ответов):.2f}, "
          f"на вопросах без ответа {сред(лучшие_пустых):.2f}")
    print(f"   задержка на шести потоках: медиана "
          f"{sorted(задержки)[len(задержки)//2]:.0f} мс, "
          f"худшая {max(задержки):.0f} мс")

    # Порог отсечения. Отклонить неверный ответ — тоже удача:
    # «заявитель не понял вопроса» лучше, чем ответ не про то
    print()
    print("   порог │ отклонено пустых │ верных сохранено │ неверных отсеяно")
    for порог in (0.10, 0.15, 0.20, 0.25, 0.30, 0.40, 0.50):
        пустые_ок = sum(1 for x in лучшие_пустых if x < порог)
        верные_ок = sum(1 for о, в in исходы if в and о >= порог)
        неверные_ок = sum(1 for о, в in исходы if not в and о < порог)
        неверных = sum(1 for _, в in исходы if not в)
        print(f"    {порог:.2f} │ {пустые_ок:>10} из {len(лучшие_пустых)} │"
              f" {верные_ок:>10} из {верных} │ {неверные_ок:>10} из {неверных}")
    print()
    print("   на этом же наборе: гигачат 100 %, GeRaCl 81 %, Laya 59 %")
    return 0


if __name__ == "__main__":
    sys.exit(главное())
