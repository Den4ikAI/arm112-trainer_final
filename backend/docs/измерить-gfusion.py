#!/usr/bin/env python3
"""Сколько токенов GFusion выдаёт за один проход модели.

Вопрос, ради которого всё затевалось. На процессоре при генерации
по одному токену мы упираемся в чтение весов, а не в счёт: каждый
проход тащит через шину активные 1,8 млрд параметров. Спекулятивное
декодирование на встроенной голове MTP даёт нам 1,73 токена за
проход и +44 %.

Диффузия обещает больше: блок из 32 позиций уточняется целиком.
Но каждый её проход считает 32 позиции вместо одной. Замер
llama-bench на том же железе: проход по одной позиции — 35 мс,
по 32 — 468 мс, то есть дороже в 13,3 раза, а не в 32. Значит
всё упирается в число шагов уточнения на блок. Оно здесь и меряется,
включая то, как на него влияет бюджет энтропии gamma.

Число шагов — свойство модели, а не железа, поэтому считаем на
всех ядрах. Токены в секунду отсюда брать нельзя: модель идёт
в bf16, наш гигачат — в q4_K_M.
"""
import sys
import time

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

ПУТЬ = "/media/denis/work4/llm/gfusion"
БЛОК = 32
ПОТОЛОК_ШАГОВ = 32       # столько же, сколько в generate по умолчанию
НОВЫХ_ТОКЕНОВ = 64
ПРОХОД_32 = 0.4676       # секунды на проход по 32 позициям, llama-bench
НАШ_MTP = 14.8           # ток/с на том же железе

ВОПРОС = (
    "Ты помогаешь готовить учебный вызов для тренажёра оператора "
    "службы 112. Опиши одной фразой, что говорит заявитель, у которого "
    "в многоквартирном доме горит мусоропровод."
)

ЗАПАСНАЯ = f"user<|role_sep|>\n{ВОПРОС}<|message_sep|>\n\nassistant<|role_sep|>\n"


def подсказка(tok):
    """Шаблон чата лежит отдельным файлом; если не подхватится — руками."""
    try:
        собрано = tok.apply_chat_template(
            [{"role": "user", "content": ВОПРОС}],
            add_generation_prompt=True, return_tensors="pt",
        )
        ids = собрано["input_ids"] if hasattr(собрано, "keys") else собрано
        print("подсказка собрана по шаблону модели", flush=True)
        return ids
    except Exception as e:
        print(f"шаблон не сработал ({e}), собираю вручную", flush=True)
        return tok(ЗАПАСНАЯ, return_tensors="pt").input_ids


def отчёт(гамма, токенов, счёт, длина, секунд, текст):
    разбор = длина // БЛОК                     # проходы на разбор подсказки
    генерация = max(1, счёт["проходов"] - разбор)
    блоков = max(1, -(-токенов // БЛОК))
    шагов = генерация / блоков
    за_проход = токенов / генерация
    прогноз = БЛОК / ((шагов + 1) * ПРОХОД_32)

    print()
    print(f"── бюджет энтропии gamma = {гамма}")
    print(f"   выдано токенов:        {токенов}")
    print(f"   проходов на генерацию: {генерация} (на разбор подсказки: {разбор})")
    print(f"   ШАГОВ НА БЛОК:         {шагов:.1f}")
    print(f"   ТОКЕНОВ ЗА ПРОХОД:     {за_проход:.2f}   (у нас на MTP — 1,73)")
    print(f"   время:                 {секунд:.0f} с в bf16 на 32 ядрах")
    print(f"   пересчёт на 6 потоков и q4_K_M: {прогноз:.1f} ток/с "
          f"против наших {НАШ_MTP}")
    print(f"   вывод: {'быстрее' if прогноз > НАШ_MTP else 'медленнее'}")
    print(f"   ответ: {текст[:220]!r}")


def главное() -> int:
    torch.set_num_threads(32)

    print("читаю токенизатор…", flush=True)
    tok = AutoTokenizer.from_pretrained(ПУТЬ, trust_remote_code=True)

    print("читаю веса…", flush=True)
    t0 = time.time()
    try:
        model = AutoModelForCausalLM.from_pretrained(
            ПУТЬ, trust_remote_code=True, torch_dtype=torch.bfloat16)
    except TypeError:
        model = AutoModelForCausalLM.from_pretrained(
            ПУТЬ, trust_remote_code=True, dtype=torch.bfloat16)
    model.eval()
    print(f"загружено за {time.time() - t0:.0f} с", flush=True)

    # Один проход — одно чтение весов, независимо от числа позиций в нём
    счёт = {"проходов": 0, "позиций": 0}
    исходный = model.forward

    def считающий(*args, **kwargs):
        счёт["проходов"] += 1
        ids = kwargs.get("input_ids")
        if ids is None and args:
            ids = args[0]
        if ids is not None:
            счёт["позиций"] += int(ids.shape[-1])
        return исходный(*args, **kwargs)

    model.forward = считающий

    ids = подсказка(tok)
    длина = int(ids.shape[1])
    print(f"длина подсказки: {длина} токенов", flush=True)

    # gamma — сколько позиций раскрывается за шаг. По умолчанию 0,15,
    # и это осторожно. Если параллельность диффузии вообще может себя
    # окупить, то при большем бюджете
    for гамма in (0.15, 0.4, 0.8):
        print(f"\nсчитаю при gamma={гамма}…", flush=True)
        счёт["проходов"] = 0
        счёт["позиций"] = 0
        t0 = time.time()
        with torch.no_grad():
            out = model.generate(
                ids, block_size=БЛОК, denoise_steps=ПОТОЛОК_ШАГОВ,
                max_new_tokens=НОВЫХ_ТОКЕНОВ, do_sample=False, gamma=гамма,
            )
        отчёт(гамма, int(out.shape[1]), счёт, длина, time.time() - t0,
              tok.decode(out[0], skip_special_tokens=True))

    return 0


if __name__ == "__main__":
    sys.exit(главное())
