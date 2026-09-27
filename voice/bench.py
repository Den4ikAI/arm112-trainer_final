"""Замер голоса на процессоре: синтез реплик и распознавание.

Меряет модели напрямую, без HTTP, по несколько прогонов на фразу —
первый прогон отдельно: он прогревает onnxruntime.

    VOICE_MODELS_DIR=... python bench.py [потоков]

Результат печатается таблицей в markdown — её кладут в
backend/docs/замеры/ГОЛОС.md.
"""

from __future__ import annotations

import os
import statistics
import sys
import time
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
os.environ.setdefault("VOICE_CACHE_DIR", "/tmp/voice-bench-cache")
import server  # noqa: E402 — берём ту же загрузку и нормализацию, что в работе

THREADS = int(sys.argv[1]) if len(sys.argv) > 1 else server.THREADS
server.THREADS = THREADS
RUNS = 5

# Реплики из тренажёра: доклад старшего и вопрос начальника
PHRASES = [
    ("brigade", "Старший бригады 2: прибыли на место."),
    ("brigade", "Старший бригады 1: пока без изменений."),
    ("chief", "Диспетчер, доложите обстановку по происшествию на Станционной улице, дом 28."),
]


def main() -> None:
    server._load()
    tts, teratts = server._tts, server._teratts
    rate = teratts.SAMPLE_RATE
    print(f"Потоков onnxruntime: {THREADS}\n")
    print("| Реплика | Голос | Звук, с | Первый прогон, с | Медиана, с | RTF |")
    print("|---|---|---|---|---|---|")
    samples_for_asr = []
    for voice, text in PHRASES:
        style = server.VOICES[voice]
        times = []
        wave = None
        for _ in range(RUNS):
            t = time.monotonic()
            wave = teratts.generate_speech(tts, server.normalize(text), style)
            times.append(time.monotonic() - t)
        dur = len(wave) / rate
        med = statistics.median(times[1:])
        print(f"| {text} | {voice} ({style}) | {dur:.2f} | {times[0]:.2f} | {med:.2f} | {med / dur:.2f} |")
        n = int(len(wave) * server.ASR_RATE / rate)
        samples_for_asr.append(
            np.interp(np.linspace(0, len(wave) - 1, n), np.arange(len(wave)), wave).astype(np.float32)
        )

    print("\n| Реплика, с | Первый прогон, с | Медиана, с | RTF | Распознано |")
    print("|---|---|---|---|---|")
    for audio in samples_for_asr:
        times = []
        text = ""
        for _ in range(RUNS):
            t = time.monotonic()
            text = server._asr.recognize(audio, sample_rate=server.ASR_RATE)
            times.append(time.monotonic() - t)
        dur = len(audio) / server.ASR_RATE
        med = statistics.median(times[1:])
        print(f"| {dur:.2f} | {times[0]:.2f} | {med:.2f} | {med / dur:.3f} | {text} |")


if __name__ == "__main__":
    main()
