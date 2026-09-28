"""Сервер голоса тренажёра: распознавание и синтез речи на процессоре.

Отдельный процесс, а не часть сервера приложения: модели написаны под
Python и onnxruntime, а тренажёр должен работать и без голоса. Сервер
приложения ходит сюда по HTTP (`VOICE_BASE_URL`), недоступен голос —
разговор идёт текстом, как раньше.

Модели (обе — ONNX, видеокарта не нужна):

* распознавание — GigaAM v3 e2e RNNT через пакет onnx_asr: сразу
  с пунктуацией и заглавными буквами;
* синтез — TeraTTSv2 с готовыми голосами из поставки модели. Голоса
  никого не клонируют: это стили, которые идут вместе с весами.

Каталог моделей — `VOICE_MODELS_DIR`:

    gigaam-v3-onnx/     istupakov/gigaam-v3-onnx (файлы v3_e2e_rnnt*)
    TeraTTSv2/          TeraSpace/TeraTTSv2 целиком: код рантайма лежит
                        рядом с весами, мы его не копируем в репозиторий

Запуск: `uvicorn server:app --host 127.0.0.1 --port 8091`.
"""

from __future__ import annotations

import hashlib
import io
import logging
import os
import re
import sys
import threading
import time
import wave
from pathlib import Path

import numpy as np
import soundfile as sf
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import Response
from num2words import num2words
from pydantic import BaseModel

log = logging.getLogger("voice")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

MODELS = Path(os.environ.get("VOICE_MODELS_DIR", "/models"))
# Потоков на модель. Шесть ядер из ТЗ делятся с сервером приложения
# и базой, поэтому по умолчанию четыре
THREADS = int(os.environ.get("VOICE_THREADS", "4"))
CACHE = Path(os.environ.get("VOICE_CACHE_DIR", "/tmp/voice-cache"))
# Дольше минуты диспетчер в одну реплику не говорит; длинный файл —
# скорее ошибка клиента, и держать процессор ради него незачем
MAX_ASR_SEC = 60
MAX_TTS_CHARS = 600
ASR_RATE = 16_000

# Роль собеседника → стиль TeraTTSv2. Все — русские стили из поставки
# модели; ru_m5 и ru_f1 в README модели отмечены как рекомендованные.
# Разные стили — чтобы на слух отличать старшего бригады от начальника.
# Заявитель звучит у оператора 112, а бригада и начальник — у ДДС, так
# что совпадение ru_m1 у начальника и заявителя-мужчины в одном
# разговоре не встречается. Пол заявителя клиент берёт из отчества
VOICES = {
    "brigade": "ru_m5",
    "chief": "ru_m1",
    "applicant_m": "ru_m1",
    "applicant_f": "ru_f1",
}

app = FastAPI(title="Голос тренажёра АРМ-112")

_asr = None
_tts = None
_teratts = None
# onnxruntime умеет параллельные вызовы, но на шести ядрах два синтеза
# разом только мешают друг другу: очередь честнее
_asr_lock = threading.Lock()
_tts_lock = threading.Lock()


def _load() -> None:
    global _asr, _tts, _teratts
    import onnx_asr
    import onnxruntime as rt

    started = time.monotonic()
    options = rt.SessionOptions()
    options.intra_op_num_threads = THREADS
    options.inter_op_num_threads = 1
    _asr = onnx_asr.load_model(
        "gigaam-v3-e2e-rnnt",
        path=str(MODELS / "gigaam-v3-onnx"),
        sess_options=options,
        providers=["CPUExecutionProvider"],
    )
    log.info("распознавание загружено за %.1f с", time.monotonic() - started)

    started = time.monotonic()
    release = MODELS / "TeraTTSv2"
    sys.path.insert(0, str(release))
    import teratts  # noqa: E402 — код рантайма идёт вместе с весами

    _teratts = teratts
    _tts = teratts.load_model(release, threads=THREADS, provider="CPUExecutionProvider")
    log.info("синтез загружен за %.1f с", time.monotonic() - started)
    CACHE.mkdir(parents=True, exist_ok=True)


@app.on_event("startup")
def startup() -> None:
    _load()


@app.get("/health")
def health() -> dict:
    return {"asr": _asr is not None, "tts": _tts is not None, "voices": sorted(VOICES)}


# ─── Распознавание ───────────────────────────────────────────────────


def _to_mono_16k(data: bytes) -> np.ndarray:
    try:
        samples, rate = sf.read(io.BytesIO(data), dtype="float32", always_2d=True)
    except Exception as e:  # noqa: BLE001 — любой нечитаемый файл — ошибка клиента
        raise HTTPException(400, f"не удалось прочитать аудио: {e}") from e
    mono = samples.mean(axis=1)
    if len(mono) / rate > MAX_ASR_SEC:
        raise HTTPException(413, f"реплика длиннее {MAX_ASR_SEC} с")
    if rate != ASR_RATE:
        # Линейной интерполяции для речи достаточно: клиент и так шлёт
        # 16 кГц, пересчёт — страховка для чужих файлов
        n = int(len(mono) * ASR_RATE / rate)
        mono = np.interp(np.linspace(0, len(mono) - 1, n), np.arange(len(mono)), mono)
    return mono.astype(np.float32)


@app.post("/asr")
async def asr(request: Request) -> dict:
    audio = _to_mono_16k(await request.body())
    if len(audio) < ASR_RATE // 10:
        return {"text": "", "durationMs": 0}
    started = time.monotonic()
    with _asr_lock:
        text = _asr.recognize(audio, sample_rate=ASR_RATE)
    took = time.monotonic() - started
    duration = len(audio) / ASR_RATE
    log.info("asr: %.2f с аудио за %.2f с", duration, took)
    return {"text": str(text).strip(), "durationMs": int(duration * 1000)}


# ─── Синтез ──────────────────────────────────────────────────────────

ABBREVIATIONS = [
    (re.compile(r"№\s*"), "номер "),
    (re.compile(r"\bул\.\s*"), "улица "),
    (re.compile(r"\bд\.\s*(?=\d)"), "дом "),
    (re.compile(r"\bкв\.\s*(?=\d)"), "квартира "),
    (re.compile(r"\bкорп\.\s*(?=\d)"), "корпус "),
    (re.compile(r"\bстр\.\s*(?=\d)"), "строение "),
]
NUMBER = re.compile(r"\d+")


def normalize(text: str) -> str:
    """Текст для синтеза: сокращения и числа — словами.

    Рантайм TeraTTS разворачивает числа сам, но пропускает число перед
    точкой («дом 28.») и молча выбрасывает «№» — в адресах это теряет
    номер дома. Поэтому числа разворачиваем до него.
    """
    out = text.replace("\n", " ")
    for pattern, repl in ABBREVIATIONS:
        out = pattern.sub(repl, out)
    out = NUMBER.sub(lambda m: num2words(int(m.group()), lang="ru"), out)
    out = re.sub(r"\s+", " ", out).strip()
    # Разметка языка обязательна для рантайма; угловые скобки из текста
    # убираем, чтобы пользователь не мог вставить свою
    out = out.replace("<", " ").replace(">", " ")
    return f"<ru>{out}</ru>"


def _wav_bytes(samples: np.ndarray, rate: int) -> bytes:
    pcm = np.clip(samples, -1.0, 1.0)
    pcm = (pcm * 32767).astype("<i2")
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.tobytes())
    return buf.getvalue()


class TtsRequest(BaseModel):
    text: str
    voice: str = "brigade"


@app.post("/tts")
def tts(req: TtsRequest) -> Response:
    text = req.text.strip()
    if not text:
        raise HTTPException(400, "пустой текст")
    if len(text) > MAX_TTS_CHARS:
        raise HTTPException(413, f"текст длиннее {MAX_TTS_CHARS} символов")
    style = VOICES.get(req.voice)
    if style is None:
        raise HTTPException(400, f"неизвестный голос {req.voice!r}, есть: {sorted(VOICES)}")

    # Реплики собеседников повторяются («прибыли на место»), и синтез
    # одной и той же фразы дважды — лишняя секунда процессора
    key = hashlib.sha256(f"{style}\0{text}".encode()).hexdigest()
    cached = CACHE / f"{key}.wav"
    if cached.exists():
        return Response(cached.read_bytes(), media_type="audio/wav",
                        headers={"X-Voice-Cache": "hit"})

    started = time.monotonic()
    with _tts_lock:
        samples = _teratts.generate_speech(_tts, normalize(text), style)
    took = time.monotonic() - started
    rate = _teratts.SAMPLE_RATE
    log.info("tts: %.2f с аудио за %.2f с (%s)", len(samples) / rate, took, style)

    data = _wav_bytes(samples, rate)
    tmp = cached.with_suffix(".tmp")
    tmp.write_bytes(data)
    tmp.replace(cached)
    return Response(data, media_type="audio/wav", headers={"X-Voice-Cache": "miss"})
