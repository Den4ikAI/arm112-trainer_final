"""Замер кросс-энкодера на процессоре: float32 против bfloat16 и попытка ONNX.

Запуск: python bench.py <каталог чекпоинта> [потоков]
Результат — в backend/docs/замеры/ОПИСАНИЯ.md.
"""
import os
import sys
import tempfile
import time

import torch

from openjev import OpenJevCrossEncoder

PATH = sys.argv[1]
torch.set_num_threads(int(sys.argv[2]) if len(sys.argv) > 2 else 6)

P = "Гор. крыша ч/д, откр. пламя. Пострадавших нет. Газа в доме нет. г. Королёв."
H = ["Горит крыша частного дома.", "Пострадавших нет.", "Дом не газифицирован.",
     "Дом частный, одноэтажный.", "Видно открытое пламя."]

for dt in (torch.float32, torch.bfloat16):
    m = OpenJevCrossEncoder(PATH, dtype=dt, device="cpu")
    m.predict_hypotheses(P, H)
    t = time.time()
    for _ in range(3):
        p = m.predict_hypotheses(P, H)
    print(f"{dt}: {(time.time() - t) / 3:.2f} с на описание, метки {p.argmax(1).tolist()}", flush=True)
    del m

m = OpenJevCrossEncoder(PATH, dtype=torch.float32, device="cpu")
enc = m._encode([(P, H[0])])


class Обёртка(torch.nn.Module):
    def __init__(self, модель):
        super().__init__()
        self.m = модель

    def forward(self, ids, mask):
        return self.m(input_ids=ids, attention_mask=mask).logits


for dynamo in (False, True):
    try:
        t = time.time()
        torch.onnx.export(Обёртка(m.model), (enc["input_ids"], enc["attention_mask"]),
                          os.path.join(tempfile.mkdtemp(), "openjev.onnx"),
                          input_names=["ids", "mask"], output_names=["logits"],
                          dynamic_axes={"ids": {1: "n"}, "mask": {1: "n"}},
                          opset_version=18, dynamo=dynamo)
        print(f"ONNX (dynamo={dynamo}): экспорт прошёл за {time.time() - t:.0f} с", flush=True)
    except Exception as e:
        print(f"ONNX (dynamo={dynamo}): не экспортируется — {type(e).__name__}: {str(e)[:300]}", flush=True)
