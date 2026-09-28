"""Собрать MP4 из записи демо-сценария.

    python assemble.py <каталог записи> <итоговый.mp4>

В каталоге — то, что оставил frontend/e2e/demo/kit.ts:

    frames/*.jpg, frames.json   кадры CDP screencast с отметками времени
    sounds.json, audio/*.wav    закадровый голос и реплики собеседников
    speedup.json                отрезки ожидания модели, которые сжимаются
                                (на экране они помечены плашкой «ускорено ×N»)

Кадры приходят только когда экран меняется, поэтому у каждого своя
длительность — до следующего. Звук кладётся на те же отметки времени,
что и кадры, и сводится в одну дорожку. Нужны ffmpeg, numpy, soundfile
(есть в venv сервера голоса).
"""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
import soundfile as sf

RATE = 44_100
TAIL = 1.2  # секунд последнего кадра после последнего звука


def main(src: Path, out: Path) -> None:
    frames = json.loads((src / "frames.json").read_text())
    sounds = json.loads((src / "sounds.json").read_text())
    speed = src / "speedup.json"
    windows = json.loads(speed.read_text()) if speed.exists() else []
    if not frames:
        sys.exit("кадров нет")
    t0 = frames[0]["at"]

    def remap(t: float) -> float:
        """Время записи → время ролика с учётом сжатых отрезков."""
        shift = 0.0
        for w in windows:
            a, b, k = w["from"], w["to"], w["factor"]
            if t <= a:
                break
            if t < b:
                return t - shift - (t - a) * (1 - 1 / k)
            shift += (b - a) * (1 - 1 / k)
        return t - shift

    windows.sort(key=lambda w: w["from"])
    frames = [{**f, "at": remap(f["at"])} for f in frames]
    sounds = [{**s, "at": remap(s["at"])} for s in sounds]
    t0 = remap(t0)

    # Звуковая дорожка
    clips = []
    end = frames[-1]["at"] - t0
    for s in sounds:
        data, rate = sf.read(s["file"], dtype="float32", always_2d=False)
        if data.ndim > 1:
            data = data.mean(axis=1)
        if rate != RATE:
            # Все голоса TeraTTSv2 — 44,1 кГц; на случай другого — линейно
            n = int(len(data) * RATE / rate)
            data = np.interp(np.linspace(0, len(data) - 1, n), np.arange(len(data)), data).astype("float32")
        start = max(0.0, s["at"] - t0)
        clips.append((start, data))
        end = max(end, start + len(data) / RATE)
    end += TAIL

    track = np.zeros(int(end * RATE) + RATE, dtype="float32")
    for start, data in clips:
        i = int(start * RATE)
        track[i:i + len(data)] += data
    peak = float(np.max(np.abs(track))) or 1.0
    track *= min(1.0, 0.89 / peak)

    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        wav = tmp / "voice.wav"
        sf.write(wav, track[: int(end * RATE)], RATE)

        # Список кадров с длительностями для demuxer concat
        lines = []
        for i, f in enumerate(frames):
            nxt = frames[i + 1]["at"] if i + 1 < len(frames) else t0 + end
            dur = max(0.001, nxt - f["at"])
            lines.append(f"file '{(src / 'frames' / f['file']).as_posix()}'")
            lines.append(f"duration {dur:.4f}")
        # Последний кадр повторяется: иначе concat обрежет его длительность
        lines.append(f"file '{(src / 'frames' / frames[-1]['file']).as_posix()}'")
        (tmp / "frames.txt").write_text("\n".join(lines) + "\n")

        subprocess.run([
            "ffmpeg", "-y", "-loglevel", "error",
            "-f", "concat", "-safe", "0", "-i", str(tmp / "frames.txt"),
            "-i", str(wav),
            "-vf", "fps=30,scale=1920:1080:flags=lanczos,format=yuv420p",
            "-c:v", "libx264", "-preset", "slow", "-crf", "20",
            "-c:a", "aac", "-b:a", "160k",
            "-t", f"{end:.3f}",
            "-movflags", "+faststart",
            str(out),
        ], check=True)
    print(f"{out}: {end:.1f} с, кадров {len(frames)}, звуков {len(sounds)}")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]))
