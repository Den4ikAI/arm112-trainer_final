"""Переозвучить записанный демо-ролик, не снимая его заново.

    python3 docs/demo/revoice.py narrator /tmp/arm112-demo/gpu        # системный python: firered.py требует requests
    $DEMO_PY  docs/demo/revoice.py phone    /tmp/arm112-demo/gpu        # venv сервера голоса: teratts, numpy
    $DEMO_PY  docs/demo/assemble.py /tmp/arm112-demo/gpu ролик.mp4

Работает поверх записи kit.ts (sounds.json + audio/*.wav), исходный звук
стоит сохранить заранее (`cp -r audio audio-orig`).

narrator — каждый звук voice == 'narrator' синтезируется заново голосом
FireRed TTS3 (HF Space, скрипт FIRERED_DIR/firered.py, образец FR_REF с
текстом FR_REF_TEXT) тем же текстом, что уходил диктору (spoken() из
kit.ts), проверяется ASR-ом сервера голоса (:8092; при трёх seed берётся
самый разборчивый), подгоняется по длительности под исходный файл
(atempo 0,9–1,25, короче — тишина) и заменяет его.

phone — заявительница (applicant_f) синтезируется локально TeraTTSv2
стилем x_alina (клон голоса Алины, см. PROVENANCE.md); затем на все
реплики «по телефону» — заявитель, старший бригады, начальник —
накладывается телефонный кодек: полоса 300–3400 Гц, 8 кГц, μ-law.
Диктор, курсант и микрофон диспетчера — без кодека.
"""

import json
import os
import re
import subprocess
import sys
import wave
import hashlib
import urllib.request
from concurrent.futures import ThreadPoolExecutor

sys.path.insert(0, os.environ.get("FIRERED_DIR", "/media/denis/work4/lct_hack-video"))
try:
    import firered  # noqa: E402
except ImportError:  # этап phone работает без него
    firered = None

REF = os.environ.get("FR_REF", "/media/denis/work4/lct_hack-video/ref-sasha.wav")
REF_TEXT = os.environ.get("FR_REF_TEXT", "Здравствуйте, я Головченко Александр. Это мой настоящий голос, и примерно так я должен говорить. Именно этим голосом я хочу, чтобы была озвучена запись.")
ASR = "http://127.0.0.1:8092/asr"
TMP = os.environ.get("REVOICE_CACHE", "/tmp/arm112-demo/firered-cache")
os.makedirs(TMP, exist_ok=True)


def spoken(t):
    t = re.sub(r"\[([^|\]]*)\|([^\]]*)\]", r"\2", t)
    return re.sub(r"\s+", " ", re.sub(r"<[^>]+>", " ", t)).strip()


def wav_info(path):
    with wave.open(path) as w:
        return w.getframerate(), w.getnchannels(), w.getnframes() / w.getframerate()


def duration(path):
    out = subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "format=duration",
                                   "-of", "csv=p=0", path]).decode().strip()
    return float(out)


def asr_text(path):
    p16 = path + ".16k.wav"
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", path, "-ar", "16000", "-ac", "1", p16], check=True)
    try:
        with open(p16, "rb") as f:
            req = urllib.request.Request(ASR, data=f.read(), headers={"Content-Type": "audio/wav"})
        return json.load(urllib.request.urlopen(req, timeout=120)).get("text", "")
    except Exception:
        return None  # сервер голоса недоступен — проверка пропускается
    finally:
        if os.path.exists(p16):
            os.remove(p16)


def norm(s):
    return re.sub(r"[^а-яёa-z0-9 ]", "", s.lower().replace("ё", "е"))


def similarity(a, b):
    wa, wb = set(norm(a).split()), set(norm(b).split())
    if not wa:
        return 0.0
    return len(wa & wb) / len(wa)


def fit(src, target_sec, rate, ch, out):
    """Подогнать длительность src под target_sec: atempo 0.8–1.25, тишина, хвост."""
    cur = duration(src)
    ratio = cur / target_sec  # >1 — длиннее, надо ускорить
    tempo = min(1.25, max(0.9, ratio))
    filt = f"atempo={tempo:.4f}"
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", src, "-af", filt,
                    "-ar", str(rate), "-ac", str(ch), out + ".tmp.wav"], check=True)
    d = duration(out + ".tmp.wav")
    pad = max(0.0, target_sec - d)
    # apad с pad_dur=0 дописывает тишину бесконечно — при нулевом паде фильтр не ставим
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", out + ".tmp.wav",
                    *(["-af", f"apad=pad_dur={pad:.3f}"] if pad > 0.01 else []),
                    "-ar", str(rate), "-ac", str(ch), "-c:a", "pcm_s16le", out], check=True)
    os.remove(out + ".tmp.wav")
    return cur, d + pad


def one(i, s):
    text = spoken(s["text"])
    rate, ch, sec = wav_info(s["file"])
    best = None
    for attempt, seed in enumerate([1234, 7, 42]):
        raw = os.path.join(TMP, hashlib.sha1(f"{REF}|{seed}|{text}".encode()).hexdigest() + ".wav")
        if not os.path.exists(raw):
            try:
                firered.synth(REF, REF_TEXT, text, raw, seed=seed)
            except Exception as e:
                print(f"[{i}] seed {seed}: ошибка {e}", flush=True)
                continue
        heard = asr_text(raw)
        if heard is None:
            print(f"[{i}] ASR недоступен, беру seed {seed} без проверки", flush=True)
            best = (1.0, raw)
            break
        sim = similarity(text, heard)
        print(f"[{i}] seed {seed}: сходство {sim:.2f} | {heard[:80]}", flush=True)
        if best is None or sim > best[0]:
            best = (sim, raw)
        if sim >= 0.75:
            break
    if best is None:
        print(f"[{i}] НЕ переозвучен: {text[:60]}", flush=True)
        return None
    cur, new = fit(best[1], sec, rate, ch, s["file"])
    print(f"[{i}] готово: было {sec:.1f} с, FireRed {cur:.1f} с → {new:.1f} с (сх. {best[0]:.2f})", flush=True)
    return i


def prefetch(texts, workers=2):
    """Заранее синтезировать титры (seed 1234) в кэш, пока идёт запись."""
    def go(t):
        t = spoken(t)
        raw = os.path.join(TMP, hashlib.sha1(f"{REF}|1234|{t}".encode()).hexdigest() + ".wav")
        if os.path.exists(raw):
            return
        try:
            firered.synth(REF, REF_TEXT, t, raw, seed=1234)
            print("кэш:", t[:60], flush=True)
        except Exception as e:
            print("ошибка:", t[:60], e, flush=True)
    with ThreadPoolExecutor(workers) as ex:
        list(ex.map(go, texts))


def narrator(run, workers=2):
    if run == "--prefetch":
        src = open(sys.argv[2], encoding="utf-8").read()
        texts = re.findall(r"d\.say(?:While)?\(\s*'((?:[^'\\]|\\.)*)'", src)
        texts += re.findall(r"\? '((?:[^'\\]|\\.)*)'\s*\n\s*: '((?:[^'\\]|\\.)*)'", src) and \
            [x for pair in re.findall(r"\? '((?:[^'\\]|\\.)*)'\s*\n\s*: '((?:[^'\\]|\\.)*)'", src) for x in pair]
        print("титров:", len(texts))
        prefetch(texts, workers)
        return
    sounds = json.load(open(os.path.join(run, "sounds.json")))
    jobs = [(i, s) for i, s in enumerate(sounds) if s["voice"] == "narrator"]
    print(f"фраз диктора: {len(jobs)}", flush=True)
    with ThreadPoolExecutor(workers) as ex:
        list(ex.map(lambda a: one(*a), jobs))


from pathlib import Path

PHONE = {"applicant_f", "applicant_m", "brigade", "chief"}










def phone_codec(path):
    """Полоса 300–3400 Гц → 8 кГц → μ-law → обратно в формат исходного wav."""
    rate, ch, _ = wav_info(path)
    mulaw = path + ".mulaw.wav"
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", path, "-af", "highpass=f=300,lowpass=f=3400",
                    "-ar", "8000", "-ac", "1", "-c:a", "pcm_mulaw", mulaw], check=True)
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", mulaw, "-ar", str(rate), "-ac", str(ch),
                    "-c:a", "pcm_s16le", path], check=True)
    os.remove(mulaw)


def phone(run):
    """Заявительница стилем x_alina, телефонный кодек на реплики по телефону."""
    sys.path.insert(0, os.environ.get("TERATTS_DIR", "/media/denis/work4/llm/voice/TeraTTSv2"))
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "voice"))
    import numpy as np
    import teratts
    from server import normalize
    sounds = json.load(open(os.path.join(run, "sounds.json")))
    applicants = [s for s in sounds if s["voice"] == "applicant_f"]
    if applicants:
        m = teratts.load_model(Path("/media/denis/work4/llm/voice/TeraTTSv2"), threads=6,
                               provider="CPUExecutionProvider")
        for s in applicants:
            rate, ch, sec = wav_info(s["file"])
            raw = s["file"] + ".alina.wav"
            audio = teratts.generate_speech(m, normalize(s["text"]), "x_alina")
            pcm = (np.clip(np.asarray(audio, dtype="float32"), -1, 1) * 32767).astype("<i2")
            with wave.open(raw, "wb") as w:
                w.setnchannels(1); w.setsampwidth(2); w.setframerate(teratts.SAMPLE_RATE)
                w.writeframes(pcm.tobytes())
            fit(raw, sec, rate, ch, s["file"])
            os.remove(raw)
            print(f"заявительница: {duration(s['file']):.1f} с | {s['text'][:60]}", flush=True)
    for s in sounds:
        if s["voice"] in PHONE:
            phone_codec(s["file"])
            heard = asr_text(s["file"]) or "(ASR недоступен)"
            print(f"телефон [{s['voice']}]: {heard[:90]}  <=  {s['text'][:60]}", flush=True)


if __name__ == "__main__":
    stage, run = sys.argv[1], sys.argv[2]
    if stage == "narrator":
        narrator(run, int(sys.argv[3]) if len(sys.argv) > 3 else 2)
    elif stage == "phone":
        phone(run)
    else:
        sys.exit("этап: narrator | phone")
