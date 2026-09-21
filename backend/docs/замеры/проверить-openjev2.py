# -*- coding: utf-8 -*-
"""OpenJEV через его собственный класс, с перебором формулировок гипотезы.

В первом заходе я собрал вход руками и написал гипотезу по-русски
своими словами. У модели есть штатный `rerank`, и формулировка там
другая — «The correct answer is: {}». Разница в формулировке гипотезы
для NLI может значить больше, чем сама модель, поэтому перебираем.

Заодно берём `predict_hypotheses`: он считает все гипотезы с общим
префиксом, один разбор посылки вместо шести.
"""
import importlib.util as u
import os
import sys
import time

os.environ.setdefault("HF_HOME", "/media/denis/work4/llm/hf-cache")
sys.path.insert(0, "/media/denis/work4/llm/openjev")

s = u.spec_from_file_location("набор", "/media/denis/work4/llm/набор-вопросов.py")
набор = u.module_from_spec(s); s.loader.exec_module(набор)

import torch
from modeling_openjev import OpenJevCrossEncoder

ПУТЬ = "/media/denis/work4/llm/openjev/qwen3.5-0.8b-nli-v2s-long"
СЛЕДУЕТ = 1

# Что подставляем вместо посылки и гипотезы. Тема — короткое название
# факта, вопрос — то, что напечатал оператор
ВАРИАНТЫ = [
    ("штатный, по-английски",
     lambda в: в,
     "The correct answer is: {}"),
    ("штатный, по-русски",
     lambda в: в,
     "Правильный ответ: {}"),
    ("мой первый заход",
     lambda в: в,
     "Оператор спрашивает про: {}"),
    ("посылка повествовательная",
     lambda в: f"Оператор службы 112 задал заявителю вопрос: «{в}»",
     "Оператор спрашивает про: {}"),
    ("посылка повествовательная, гипотеза штатная",
     lambda в: f"Оператор службы 112 задал заявителю вопрос: «{в}»",
     "The correct answer is: {}"),
]


def главное() -> int:
    torch.set_num_threads(6)

    print("поднимаю OpenJEV 0.8B…", flush=True)
    t0 = time.time()
    jev = OpenJevCrossEncoder(ПУТЬ, dtype=torch.float32)
    print(f"готова за {time.time() - t0:.0f} с\n", flush=True)

    for название, посылка, формат in ВАРИАНТЫ:
        верных = 0
        задержки = []
        осмысленные, пустые = [], []

        for _, факты, проверки in набор.НАБОРЫ:
            ключи = list(факты.keys())
            гипотезы = [формат.format(набор.ТЕМЫ[к]) for к in ключи]

            for запрос, цель in проверки:
                t = time.time()
                вероятности = jev.predict_hypotheses(посылка(запрос), гипотезы)
                задержки.append((time.time() - t) * 1000)

                следование = вероятности[:, СЛЕДУЕТ]
                лучший = int(следование.argmax())
                выбор, оценка = ключи[лучший], float(следование[лучший])

                if цель is None:
                    пустые.append(оценка)
                else:
                    осмысленные.append(оценка)
                    верных += выбор == цель

        сред = lambda v: sum(v) / len(v) if v else 0.0
        отрыв = сред(осмысленные) - сред(пустые)
        print(f"── {название}")
        print(f"   гипотеза: {формат.format('<тема>')!r}")
        print(f"   угадано: {верных} из {len(осмысленные)} "
              f"({верных / len(осмысленные) * 100:.0f} %)")
        print(f"   следование: осмысленные {сред(осмысленные):.2f}, "
              f"пустые {сред(пустые):.2f}, разрыв {отрыв:.2f}")
        print(f"   задержка: медиана {sorted(задержки)[len(задержки)//2]:.0f} мс")
        print()

    print("   на этом же наборе: гигачат 100 %, GeRaCl 81 %, Laya 59 %")
    return 0


if __name__ == "__main__":
    sys.exit(главное())
