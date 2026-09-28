# -*- coding: utf-8 -*-
"""Сверка описания оператора с фактами вызова: NLI против LLM.

Задача та же, что у нейросетевого анализа ручного ввода
(domain/text_analysis.rs): описание «со слов заявителя» либо
подтверждает факт вызова, либо противоречит ему, либо о нём молчит.
Здесь она поставлена в чистом виде — на каждую пару «описание, факт»
одна из трёх меток, — чтобы сравнить разные модели на равных.

Набор — `набор-описаний.json`: 3 учебных вызова, 14 описаний, 66 пар.

Запуск:
  python3 проверка-описаний.py nli <путь к чекпоинту OpenJEV>
  python3 проверка-описаний.py llm <адрес OpenAI-совместимого сервера> <имя>
  python3 проверка-описаний.py pipeline <адрес модели> <имя> <адрес nli/server.py>
"""
import json
import os
import sys
import time
from pathlib import Path

НАБОР = json.loads((Path(__file__).parent / "набор-описаний.json").read_text(encoding="utf-8"))
МЕТКИ = ["C", "E", "N"]  # порядок выхода OpenJEV: contradiction, entailment, neutral


def пары():
    for вызов in НАБОР["вызовы"]:
        for оп in вызов["описания"]:
            yield вызов, оп


def итог(имя, ответы, время):
    """ответы: список (истина, предсказание)."""
    всего = len(ответы)
    верно = sum(1 for и, п in ответы if и == п)
    по_меткам = {}
    for м in "ECN":
        есть = [(и, п) for и, п in ответы if и == м]
        по_меткам[м] = (sum(1 for и, п in есть if п == м), len(есть))
    # Ложная тревога: описание подтверждает факт, а модель кричит
    # «противоречие» — самая дорогая ошибка для курсанта
    ложных = sum(1 for и, п in ответы if и != "C" and п == "C")
    print(f"\n== {имя}")
    print(f"верно {верно} из {всего} ({верно / всего:.0%})")
    for м, (в, н) in по_меткам.items():
        print(f"  {м}: {в} из {н}")
    print(f"  ложных «противоречит»: {ложных}")
    print(f"  время на одно описание: {время / 14:.2f} с")
    return {"модель": имя, "верно": верно, "всего": всего,
            "по_меткам": по_меткам, "ложных_C": ложных, "с_на_описание": время / 14}


def гипотеза(вызов, к):
    """Что подаётся гипотезой (переменная HYP). утверждение — факт, переписанный
    нами утверждением; сырой — ответ заявителя как есть; вопрос-ответ —
    «На вопрос „…“ заявитель ответил: „…“»: так факт лежит в сценарии."""
    вид = os.environ.get("HYP", "утверждение")
    if вид == "утверждение":
        return вызов["факты"][к]
    ответ = вызов["сырые_ответы"][к]
    if вид == "сырой" or к not in вызов["вопросы"]:
        return ответ
    return f"На вопрос «{вызов['вопросы'][к]}» заявитель ответил: «{ответ}»"


def nli(путь):
    import torch
    sys.path.insert(0, "/media/denis/work4/llm/openjev")
    from modeling_openjev import OpenJevCrossEncoder

    torch.set_num_threads(int(os.environ.get("THREADS", "6")))
    jev = OpenJevCrossEncoder(путь, dtype=torch.float32, device="cpu")
    ответы, подробно = [], []
    t0 = time.time()
    for вызов, оп in пары():
        ключи = list(оп["метки"])
        p = jev.predict_hypotheses(оп["текст"], [гипотеза(вызов, к) for к in ключи])
        for к, строка in zip(ключи, p):
            пред = МЕТКИ[int(строка.argmax())]
            ответы.append((оп["метки"][к], пред))
            подробно.append({"описание": оп["текст"], "факт": вызов["факты"][к],
                             "истина": оп["метки"][к], "ответ": пред,
                             "p": [round(float(x), 3) for x in строка]})
    время = time.time() - t0
    вид = os.environ.get("HYP", "утверждение")
    return итог(f"OpenJEV {Path(путь).name} ({вид})", ответы, время), подробно


СИСТЕМА = (
    "Ты проверяешь карточку оператора службы 112. Дано описание происшествия, "
    "которое оператор записал со слов заявителя, и список фактов, известных из "
    "разговора. Для каждого факта ответь одним словом: «подтверждает» — описание "
    "сообщает этот факт (сокращения вроде «ч/д», «пострад. нет» считаются); "
    "«противоречит» — описание утверждает обратное или другое; «молчит» — в "
    "описании об этом ничего нет. Не додумывай за оператора."
)
СЛОВА = {"подтверждает": "E", "противоречит": "C", "молчит": "N"}


def llm(адрес, имя):
    import urllib.request

    ответы, подробно = [], []
    t0 = time.time()
    for вызов, оп in пары():
        ключи = list(оп["метки"])
        схема = {"type": "object", "additionalProperties": False, "required": ключи,
                 "properties": {к: {"type": "string", "enum": list(СЛОВА)} for к in ключи}}
        факты = "\n".join(f"{к}: {вызов['факты'][к]}" for к in ключи)
        тело = {
            "model": "local", "temperature": 0, "max_tokens": 200,
            "messages": [
                {"role": "system", "content": СИСТЕМА},
                {"role": "user", "content": f"Описание оператора:\n{оп['текст']}\n\nФакты:\n{факты}"},
            ],
            "response_format": {"type": "json_schema",
                                "json_schema": {"name": "answer", "strict": True, "schema": схема}},
        }
        req = urllib.request.Request(f"{адрес}/v1/chat/completions",
                                     data=json.dumps(тело).encode(),
                                     headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=300) as r:
            текст = json.load(r)["choices"][0]["message"]["content"]
        разбор = json.loads(текст)
        for к in ключи:
            пред = СЛОВА.get(разбор.get(к), "?")
            ответы.append((оп["метки"][к], пред))
            подробно.append({"описание": оп["текст"], "факт": вызов["факты"][к],
                             "истина": оп["метки"][к], "ответ": пред})
    время = time.time() - t0
    return итог(имя, ответы, время), подробно


def промпт_утверждений():
    """Системная подсказка подготовки утверждений — дословно из
    backend/src/domain/fact_statements.rs, чтобы мерить ровно то, что в
    продукте, а не свою копию."""
    import re
    исходник = (Path(__file__).parents[2] / "src/domain/fact_statements.rs").read_text(encoding="utf-8")
    тело = re.search(r'const SYSTEM: &str = "\\\n(.*?)";\n', исходник, re.S).group(1)
    # Продолжение строки в Rust: «\» + перевод строки + отступ исчезают
    тело = re.sub(r"\\\n\s*", "", тело)
    return тело.replace('\\"', '"')


def очистка(s):
    """То же, что fact_statements::clean: кавычки и «Нет,» в начале."""
    s = s.strip().strip("«»\"").strip()
    while True:
        низ = s.lower()
        слово = next((w for w in ("нет", "да", "ну", "что вы", "алло")
                      if низ.startswith(w) and низ[len(w):len(w) + 1] in (",", "!", ".", " ")), None)
        if not слово:
            break
        остаток = s[len(слово):].lstrip(",!. ")
        if sum(c.isalpha() for c in остаток) < 3:
            break
        s = остаток
    s = s.rstrip("!?. ")
    return s[:1].upper() + s[1:] + "." if s else s


def pipeline(адрес_llm, имя_llm, адрес_nli):
    """Путь продукта целиком: утверждения к фактам пишет языковая модель
    по промпту из fact_statements.rs, сверяет сервер nli/server.py.

    Легенда в прогон не входит: модель режет первую фразу на утверждения
    по-своему, и механически сопоставить их с разметкой набора (A1, B1,
    B2, C1) нельзя. Меряются факты, которые заявитель сообщает в ответ на
    вопрос, — 49 пар. На тех же 49 парах для сравнения — утверждения,
    написанные нами, и сырые ответы.
    """
    import urllib.request

    def post(url, тело, timeout=600):
        req = urllib.request.Request(url, data=json.dumps(тело).encode(),
                                     headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.load(r)

    система = промпт_утверждений()
    варианты = {"модель": {}, "наши": {}, "сырой": {}}
    время_утв = []
    for вызов in НАБОР["вызовы"]:
        ключи = list(вызов["вопросы"])
        схема = {
            "type": "object",
            "properties": {
                "legend": {"type": "array", "maxItems": 6, "items": {"type": "string"}},
                "facts": {"type": "array", "items": {
                    "type": "object",
                    "properties": {"key": {"type": "string", "enum": ключи},
                                   "statement": {"type": "string"}},
                    "required": ["key", "statement"]}},
            },
            "required": ["legend", "facts"],
        }
        # Легенда — сырая первая фраза вызова (первый ключ набора)
        легенда = next(iter(вызов["сырые_ответы"].values()))
        тело = {"legend": легенда, "facts": [
            {"key": к, "question": вызов["вопросы"][к], "answer": вызов["сырые_ответы"][к]}
            for к in ключи]}
        t0 = time.time()
        ответ = post(f"{адрес_llm}/v1/chat/completions", {
            "model": "local", "temperature": 0.2, "max_tokens": 800,
            "messages": [{"role": "system", "content": система},
                         {"role": "user", "content": json.dumps(тело, ensure_ascii=False)}],
            "response_format": {"type": "json_schema",
                                "json_schema": {"name": "answer", "strict": True, "schema": схема}},
        })
        время_утв.append(time.time() - t0)
        разбор = json.loads(ответ["choices"][0]["message"]["content"])
        для_вызова = {f["key"]: очистка(f["statement"]) for f in разбор["facts"] if f.get("statement")}
        for к in ключи:
            варианты["модель"][к] = для_вызова.get(к, вызов["сырые_ответы"][к])
            варианты["наши"][к] = вызов["факты"][к]
            варианты["сырой"][к] = вызов["сырые_ответы"][к]

    итоги, подробно = [], []
    for вид, гипотезы in варианты.items():
        ответы, t_nli = [], 0.0
        for вызов, оп in пары():
            ключи = [к for к in оп["метки"] if к in вызов["вопросы"]]
            t0 = time.time()
            p = post(f"{адрес_nli}/nli", {"premise": оп["текст"],
                                           "hypotheses": [гипотезы[к] for к in ключи]})["probs"]
            t_nli += time.time() - t0
            for к, строка in zip(ключи, p):
                пред = МЕТКИ[max(range(3), key=lambda i: строка[i])]
                ответы.append((оп["метки"][к], пред))
                подробно.append({"вид": вид, "описание": оп["текст"], "гипотеза": гипотезы[к],
                                 "истина": оп["метки"][к], "ответ": пред})
        модель_nli = post(f"{адрес_nli}/nli", {"premise": "а", "hypotheses": ["а"]})["model"]
        итоги.append(итог(f"путь продукта: {модель_nli}, утверждения — {вид}"
                          + (f" ({имя_llm})" if вид == "модель" else ""), ответы, t_nli))
    print(f"\nутверждения моделью {имя_llm}: {sum(время_утв):.1f} с на {len(время_утв)} вызова")
    return {"модель": f"путь продукта {имя_llm} + {модель_nli}", "итоги": итоги,
            "утверждения_модели": варианты["модель"],
            "с_на_утверждения_вызова": время_утв}, подробно


if __name__ == "__main__":
    режим = sys.argv[1]
    if режим == "nli":
        сводка, подробно = nli(sys.argv[2])
    elif режим == "pipeline":
        сводка, подробно = pipeline(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        сводка, подробно = llm(sys.argv[2], sys.argv[3])
    выход = Path(__file__).parent / "описания-результаты"
    выход.mkdir(exist_ok=True)
    файл = выход / (сводка["модель"].replace(" ", "_").replace("/", "_") + ".json")
    файл.write_text(json.dumps({"сводка": сводка, "пары": подробно}, ensure_ascii=False, indent=1),
                    encoding="utf-8")
