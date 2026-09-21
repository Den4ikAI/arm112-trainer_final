# -*- coding: utf-8 -*-
"""GeRaCl на той же задаче, что и Laya: вопрос оператора → скрытый факт.

Набор проверок тот же, чтобы сравнение было честным.
"""
import importlib.util as u
import os
import sys
import time

os.environ.setdefault("HF_HOME", "/media/denis/work4/llm/hf-cache")
sys.path.insert(0, "/media/denis/work4/llm/geracl")

s = u.spec_from_file_location("набор", "/media/denis/work4/llm/набор-вопросов.py")
набор = u.module_from_spec(s); s.loader.exec_module(набор)

import torch
from transformers import AutoTokenizer
from geracl import GeraclHF, ZeroShotClassificationPipeline


ТЕМАМИ = "--темы" in sys.argv


def главное() -> int:
    torch.set_num_threads(6)      # столько же, сколько на целевом стенде

    print(f"метки: {'короткие темы' if ТЕМАМИ else 'вопросы целиком'}")
    print("поднимаю GeRaCl…", flush=True)
    t0 = time.time()
    # В коде GeRaCl устройство зашито в config со значением "cuda",
    # и внутренний слой безусловно делает .to(self._device).
    # Видеокарты у нас нет и на целевом стенде не будет
    model = GeraclHF.from_pretrained("deepvk/GeRaCl-USER2-base", device="cpu").eval()
    if hasattr(model, "_classification_core"):
        model._classification_core._device = "cpu"
    tok = AutoTokenizer.from_pretrained("deepvk/GeRaCl-USER2-base")
    pipe = ZeroShotClassificationPipeline(model, tok, device="cpu")
    print(f"готова за {time.time() - t0:.0f} с\n", flush=True)

    верных = 0
    задержки = []
    пустых = 0
    лучшие_ответов, лучшие_пустых = [], []
    отрывы_ответов, отрывы_пустых = [], []

    for название, факты, проверки in набор.НАБОРЫ:
        print(f"══ {название} ({len(факты)} вариантов)")
        ключи = list(факты.keys())
        метки = ([набор.ТЕМЫ[к] for к in ключи] if ТЕМАМИ
                 else [факты[к] for к in ключи])

        for запрос, цель in проверки:
            t = time.time()
            # Берём сами оценки, а не только победителя: без них нельзя
            # отличить «спросил про подъезд» от «спросил про то, чего
            # заявитель не знает»
            сырое = pipe.get_similarities([запрос], метки, True, 1)[0]
            плоское = сырое.reshape(-1).tolist()
            оценки = [float(x) for x in плоское[: len(метки)]]
            задержки.append((time.time() - t) * 1000)
            лучший = max(range(len(оценки)), key=lambda i: оценки[i])
            выбор = ключи[лучший]
            топ = sorted(оценки, reverse=True)
            отрыв = топ[0] - топ[1]

            if цель is None:
                пустых += 1
                знак = "·"
            elif выбор == цель:
                верных += 1
                знак = "+"
            else:
                знак = "—"

            (лучшие_пустых if цель is None else лучшие_ответов).append(топ[0])
            (отрывы_пустых if цель is None else отрывы_ответов).append(отрыв)
            ожидалось = цель if цель else "(нечего отвечать)"
            print(f"  {знак} {запрос:36} → {выбор:10} "
                  f"оценка {топ[0]:+.2f} отрыв {отрыв:.2f}  ждали: {ожидалось}")
        print()

    сопоставимых = сум = sum(len(п) for _, _, п in набор.НАБОРЫ) - пустых
    print("── итог")
    print(f"   угадано: {верных} из {сопоставимых} "
          f"({верных / сопоставимых * 100:.0f} %)")
    print(f"   задержка на шести потоках: медиана "
          f"{sorted(задержки)[len(задержки)//2]:.0f} мс, "
          f"худшая {max(задержки):.0f} мс")
    сред = lambda v: sum(v) / len(v) if v else 0.0
    print(f"   оценка победителя: на осмысленных {сред(лучшие_ответов):+.2f}, "
          f"на вопросах без ответа {сред(лучшие_пустых):+.2f}")
    print(f"   отрыв от второго:  на осмысленных {сред(отрывы_ответов):.2f}, "
          f"на вопросах без ответа {сред(отрывы_пустых):.2f}")

    # Порог отсечения: можно ли по оценке понять, что вопрос не про то
    лучший_порог, лучшая_доля = None, 0.0
    for шаг in range(-20, 21):
        порог = шаг / 20
        отсеяно = sum(1 for x in лучшие_пустых if x < порог)
        сохранено = sum(1 for x in лучшие_ответов if x >= порог)
        доля = (отсеяно + сохранено) / (len(лучшие_пустых) + len(лучшие_ответов))
        if доля > лучшая_доля:
            лучшая_доля, лучший_порог = доля, порог
    print(f"   лучший порог отсечения {лучший_порог:+.2f}: "
          f"верно разделяет {лучшая_доля * 100:.0f} % случаев")
    print()
    print("   для сравнения, Laya на этом же наборе: 59 %")
    return 0


if __name__ == "__main__":
    sys.exit(главное())
