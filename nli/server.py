"""Сервер кросс-энкодера NLI: сверка описания курсанта с фактами вызова.

Кросс-энкодер OpenJEV (Qwen3.5 с головой на три класса) читает посылку
и гипотезу и отвечает вероятностями «противоречит / следует / молчит».
Тренажёр подаёт посылкой описание, которое курсант записал со слов
заявителя, а гипотезами — факты вызова, сформулированные утверждениями.
Генерации нет: один проход на гипотезу, и повтор даёт тот же ответ.

Отдельный процесс, а не часть сервера голоса: здесь PyTorch и
transformers, а сервер голоса нарочно обходится одним onnxruntime и
от них не зависит. Сервер приложения ходит сюда по HTTP
(`NLI_BASE_URL`); нет сервера — сверка идёт языковой моделью, как раньше.

Почему PyTorch, а не ONNX: модель гибридная (часть слоёв — рекуррентное
линейное внимание Gated DeltaNet), и штатный экспорт в ONNX на ней
падает — оператор `linalg_solve_triangular` не поддержан. Считаем в
float32: bfloat16 на процессоре без его поддержки вчетверо медленнее
(замер — `bench.py`, `backend/docs/замеры/ОПИСАНИЯ.md`).

Переменные:
  NLI_MODEL_DIR  каталог чекпоинта, например qwen3.5-2b-nli-v5 (по
                 умолчанию) или qwen3.5-4b-nli-v5 из AlexWortega/openjev
  NLI_THREADS    потоков PyTorch, по умолчанию 6

Запуск: `uvicorn server:app --host 127.0.0.1 --port 8093`.
"""

from __future__ import annotations

import logging
import os
import threading
import time
from pathlib import Path

import torch
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

from openjev import OpenJevCrossEncoder

log = logging.getLogger("nli")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

MODEL_DIR = Path(os.environ.get("NLI_MODEL_DIR", "/models/qwen3.5-2b-nli-v5"))
THREADS = int(os.environ.get("NLI_THREADS", "6"))
# Описание карточки — абзац, фактов у вызова 5–8. Больше — ошибка
# клиента, и держать процессор ради неё незачем
MAX_PREMISE_CHARS = 4000
MAX_HYPOTHESES = 32

app = FastAPI(title="Сверка описания тренажёра АРМ-112")

_model: OpenJevCrossEncoder | None = None
# Один запрос за раз: на шести ядрах два прохода разом только мешают
# друг другу, очередь честнее
_lock = threading.Lock()


def _load() -> None:
    global _model
    torch.set_num_threads(THREADS)
    started = time.time()
    _model = OpenJevCrossEncoder(str(MODEL_DIR), dtype=torch.float32, device="cpu")
    log.info("кросс-энкодер %s поднят за %.0f с, потоков %d", MODEL_DIR.name, time.time() - started, THREADS)


@app.on_event("startup")
def startup() -> None:
    _load()


class Request(BaseModel):
    premise: str
    hypotheses: list[str]


@app.get("/health")
def health() -> dict:
    return {"ok": _model is not None, "model": MODEL_DIR.name, "threads": THREADS}


@app.post("/nli")
def nli(req: Request) -> dict:
    if _model is None:
        raise HTTPException(503, "модель ещё не поднята")
    premise = req.premise.strip()
    hypotheses = [h.strip() for h in req.hypotheses]
    if not premise or not hypotheses or any(not h for h in hypotheses):
        raise HTTPException(400, "нужны непустые посылка и гипотезы")
    if len(premise) > MAX_PREMISE_CHARS or len(hypotheses) > MAX_HYPOTHESES:
        raise HTTPException(413, "слишком длинная посылка или слишком много гипотез")

    started = time.time()
    with _lock:
        probs = _model.predict_hypotheses(premise, hypotheses)
    ms = int((time.time() - started) * 1000)
    return {
        # [contradiction, entailment, neutral] — порядок меток модели
        "probs": [[round(float(x), 4) for x in row] for row in probs],
        "labels": ["contradiction", "entailment", "neutral"],
        "model": MODEL_DIR.name,
        "ms": ms,
    }
