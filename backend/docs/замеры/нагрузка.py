#!/usr/bin/env python3
"""Нагрузочная проверка требований ТЗ §7 на сервере тренажёра.

Запускать против ОТДЕЛЬНОЙ базы, не стенда: скрипт заводит сотню
учётных записей и сотни карточек. Порядок — в НАГРУЗКА.md.

    python3 нагрузка.py --base http://127.0.0.1:8095 setup
    python3 нагрузка.py --base ... mixed --users 100 --seconds 60
    python3 нагрузка.py --base ... sessions --parallel 50
    python3 нагрузка.py --base ... writes --parallel 50 --seconds 20
    python3 нагрузка.py --base ... reports

Каждая команда печатает сводку и пишет сырые данные в --out (JSON).
Нужен только aiohttp.
"""

import argparse
import asyncio
import json
import random
import statistics
import time
from collections import defaultdict
from pathlib import Path

import aiohttp

PASSWORD = "нагрузка-пароль-1"
STUDENTS = 80
TEACHERS = 20

# Эталон билета 3, вызов 1 — горит крыша частного дома в Королёве
# (тот же, что в tests/grade_api.rs). Отвечаем верно: нагрузка должна
# проходить весь путь до разбора, а не обрываться на ошибке ввода
ADDRESS = {"region": "Московская область", "settlement": "Королёв",
           "street": "Станционная", "house": "28"}
ANSWERS = {"where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
           "house-kind": ["Дом частный"], "people-threat": ["Нет"],
           "inhouse": ["Крыша"], "gasified": ["Нет"]}


class Recorder:
    """Время каждого запроса по имени операции и коды ответов."""

    def __init__(self):
        self.lat = defaultdict(list)
        self.codes = defaultdict(lambda: defaultdict(int))
        self.errors = []

    async def call(self, http, name, method, url, token=None, body=None, expect=(200, 201, 204)):
        headers = {"Authorization": f"Bearer {token}"} if token else {}
        t0 = time.perf_counter()
        try:
            async with http.request(method, url, json=body, headers=headers) as r:
                raw = await r.read()
                dt = time.perf_counter() - t0
                self.lat[name].append(dt)
                self.codes[name][r.status] += 1
                if r.status not in expect:
                    self.errors.append((name, r.status, raw[:200].decode("utf-8", "replace")))
                    return r.status, None
                if not raw:
                    return r.status, None
                try:
                    return r.status, json.loads(raw)
                except ValueError:
                    return r.status, raw
        except Exception as e:  # сетевой отказ — тоже ошибка под нагрузкой
            self.lat[name].append(time.perf_counter() - t0)
            self.codes[name]["exc"] += 1
            self.errors.append((name, "exc", repr(e)[:200]))
            return None, None

    def summary(self):
        out = {}
        everything = []
        for name, xs in sorted(self.lat.items()):
            xs_sorted = sorted(xs)
            everything += xs
            out[name] = stats(xs_sorted) | {"codes": dict(self.codes[name])}
        out["ВСЕ"] = stats(sorted(everything))
        out["ошибок"] = len(self.errors)
        out["примеры_ошибок"] = self.errors[:10]
        return out


def pct(xs, p):
    if not xs:
        return None
    k = min(len(xs) - 1, max(0, round(p / 100 * (len(xs) - 1))))
    return xs[k]


def stats(xs):
    if not xs:
        return {"n": 0}
    return {"n": len(xs),
            "p50_ms": round(pct(xs, 50) * 1000, 1),
            "p95_ms": round(pct(xs, 95) * 1000, 1),
            "p99_ms": round(pct(xs, 99) * 1000, 1),
            "max_ms": round(xs[-1] * 1000, 1),
            "mean_ms": round(statistics.fmean(xs) * 1000, 1)}


def print_table(title, summ):
    print(f"\n== {title}")
    print(f"{'операция':32} {'n':>6} {'p50':>8} {'p95':>8} {'p99':>8} {'max':>8}  коды")
    for name, s in summ.items():
        if not isinstance(s, dict) or "n" not in s or s["n"] == 0:
            continue
        print(f"{name:32} {s['n']:>6} {s['p50_ms']:>8} {s['p95_ms']:>8} "
              f"{s['p99_ms']:>8} {s['max_ms']:>8}  {s.get('codes', '')}")
    print(f"ошибок: {summ['ошибок']}")
    for e in summ["примеры_ошибок"]:
        print("  ", e)


async def login(rec, http, base, user, password=PASSWORD):
    _, b = await rec.call(http, "вход", "POST", f"{base}/api/auth/login",
                          body={"login": user, "password": password})
    return b["token"] if b else None


async def scenario_id(http, base, token):
    async with http.get(f"{base}/api/scenarios",
                        headers={"Authorization": f"Bearer {token}"}) as r:
        items = await r.json()
    for s in items:
        if s.get("ticket") == 3 and s.get("callNo") == 1:
            return s["id"]
    raise SystemExit(f"нет сценария «билет 3, вызов 1»: {items[:2]}")


# ─── Подготовка ───────────────────────────────────────────────────────

async def setup(args):
    rec = Recorder()
    async with aiohttp.ClientSession() as http:
        admin = await login(rec, http, args.base, "admin", "arm112")
        for i in range(STUDENTS):
            await rec.call(http, "завести", "POST", f"{args.base}/api/admin/users", admin,
                           {"login": f"load-s{i:03}", "password": PASSWORD,
                            "fio": f"Нагрузка Курсант {i:03}", "role": "student",
                            "serviceId": "mchs", "workstation": str(i % 12 + 1)},
                           expect=(200, 201, 409))
        for i in range(TEACHERS):
            await rec.call(http, "завести", "POST", f"{args.base}/api/admin/users", admin,
                           {"login": f"load-t{i:02}", "password": PASSWORD,
                            "fio": f"Нагрузка Преподаватель {i:02}", "role": "teacher"},
                           expect=(200, 201, 409))
    print_table("подготовка", rec.summary())


# ─── Смешанная нагрузка: 100 пользователей ────────────────────────────

async def student_loop(rec, http, base, login_name, sid, deadline, think):
    tok = await login(rec, http, base, login_name)
    if not tok:
        return 0
    done = 0
    while time.perf_counter() < deadline:
        _, b = await rec.call(http, "сессия.начать", "POST", f"{base}/api/sessions", tok,
                              {"scenarioId": sid, "mode": "card112"})
        if not b:
            await asyncio.sleep(1)
            continue
        s = b["session"]["id"]
        for q in b["scenario"]["questions"]:
            await rec.call(http, "сессия.вопрос", "POST", f"{base}/api/sessions/{s}/ask", tok,
                           {"factId": q["factId"]})
            await asyncio.sleep(think)
        await rec.call(http, "карточка.правка", "PATCH", f"{base}/api/sessions/{s}/card", tok,
                       {"address": ADDRESS})
        await asyncio.sleep(think)
        await rec.call(http, "классификатор.подбор", "POST", f"{base}/api/classifier/evaluate",
                       tok, {"typeIds": ["101"], "answers": ANSWERS})
        await rec.call(http, "карточка.правка", "PATCH", f"{base}/api/sessions/{s}/card", tok,
                       {"typeIds": ["101"], "answers": ANSWERS,
                        "description": "Горит крыша частного дома, людей внутри нет"})
        await asyncio.sleep(think)
        _, saved = await rec.call(http, "карточка.сохранить", "POST",
                                  f"{base}/api/sessions/{s}/save", tok)
        await rec.call(http, "разбор", "GET", f"{base}/api/sessions/{s}/grade", tok)
        await asyncio.sleep(think)

        # Та же курсантка — диспетчер своей службы: журнал, карточка,
        # отметка открытия и первая запись
        _, j = await rec.call(http, "журнал", "GET", f"{base}/api/cards?scope=service&size=10", tok)
        card_id = saved["card"]["id"] if saved else None
        if card_id:
            _, one = await rec.call(http, "карточка.открыть", "GET", f"{base}/api/cards/{card_id}", tok)
            await rec.call(http, "карточка.открыта", "POST", f"{base}/api/cards/{card_id}/opened", tok)
            own = next((x for x in (one or {}).get("services", []) if x["serviceId"] == "mchs"), None)
            if own and "Принята" in own["allowedNext"]:
                await rec.call(http, "ддс.статус", "POST", f"{base}/api/cards/{card_id}/responses", tok,
                               {"serviceId": "mchs", "status": "Принята",
                                "comment": "Принята, бригада направляется"})
        done += 1
        await asyncio.sleep(think)
    return done


async def teacher_loop(rec, http, base, login_name, deadline, think):
    tok = await login(rec, http, base, login_name)
    if not tok:
        return 0
    n = 0
    while time.perf_counter() < deadline:
        _, j = await rec.call(http, "журнал.преподаватель", "GET",
                              f"{base}/api/cards?scope=all&size=10", tok)
        items = (j or {}).get("items", [])
        if items:
            c = random.choice(items)
            await rec.call(http, "карточка.открыть", "GET", f"{base}/api/cards/{c['id']}", tok)
        await rec.call(http, "сценарии", "GET", f"{base}/api/scenarios", tok)
        await rec.call(http, "занятия", "GET", f"{base}/api/lessons", tok)
        await rec.call(http, "группы", "GET", f"{base}/api/groups", tok)
        n += 1
        await asyncio.sleep(think)
    return n


async def mixed(args):
    rec = Recorder()
    conn = aiohttp.TCPConnector(limit=0)
    async with aiohttp.ClientSession(connector=conn, timeout=aiohttp.ClientTimeout(total=60)) as http:
        admin = await login(rec, http, args.base, "admin", "arm112")
        sid = await scenario_id(http, args.base, admin)
        students = int(args.users * STUDENTS / (STUDENTS + TEACHERS))
        teachers = args.users - students
        deadline = time.perf_counter() + args.seconds
        t0 = time.perf_counter()
        tasks = [student_loop(rec, http, args.base, f"load-s{i:03}", sid, deadline, args.think)
                 for i in range(students)]
        tasks += [teacher_loop(rec, http, args.base, f"load-t{i:02}", deadline, args.think)
                  for i in range(teachers)]
        res = await asyncio.gather(*tasks)
        wall = time.perf_counter() - t0
    summ = rec.summary()
    total = sum(v["n"] for k, v in summ.items() if isinstance(v, dict) and "n" in v and k != "ВСЕ")
    summ["_условия"] = {"пользователей": args.users, "курсантов": students,
                        "преподавателей": teachers, "секунд": round(wall, 1),
                        "пауза_с": args.think, "карточек_сохранено": sum(res[:students]),
                        "запросов": total, "запросов_в_с": round(total / wall, 1)}
    print_table(f"смешанная нагрузка: {args.users} пользователей, пауза {args.think} с", summ)
    print(summ["_условия"])
    return summ


# ─── Одновременные вызовы от начала до разбора ───────────────────────

async def one_session(rec, http, base, login_name, sid):
    t0 = time.perf_counter()
    tok = await login(rec, http, base, login_name)
    _, b = await rec.call(http, "сессия.начать", "POST", f"{base}/api/sessions", tok,
                          {"scenarioId": sid, "mode": "card112"})
    if not b:
        return None
    s = b["session"]["id"]
    for q in b["scenario"]["questions"]:
        await rec.call(http, "сессия.вопрос", "POST", f"{base}/api/sessions/{s}/ask", tok,
                       {"factId": q["factId"]})
    await rec.call(http, "карточка.правка", "PATCH", f"{base}/api/sessions/{s}/card", tok,
                   {"address": ADDRESS, "typeIds": ["101"], "answers": ANSWERS,
                    "description": "Горит крыша частного дома"})
    _, saved = await rec.call(http, "карточка.сохранить", "POST", f"{base}/api/sessions/{s}/save", tok)
    if not saved:
        return None
    return {"сек": time.perf_counter() - t0, "балл": saved["grade"].get("score"),
            "карточка": saved["card"]["number"], "сессия": s}


async def sessions(args):
    rec = Recorder()
    conn = aiohttp.TCPConnector(limit=0)
    async with aiohttp.ClientSession(connector=conn) as http:
        admin = await login(rec, http, args.base, "admin", "arm112")
        sid = await scenario_id(http, args.base, admin)
        # Все стартуют одновременно: барьер — gather по готовым корутинам
        res = await asyncio.gather(*[one_session(rec, http, args.base, f"load-s{i:03}", sid)
                                     for i in range(args.parallel)])
    ok = [r for r in res if r]
    summ = rec.summary()
    numbers = {r["карточка"] for r in ok}
    summ["_условия"] = {"одновременно": args.parallel, "доведено_до_разбора": len(ok),
                        "уникальных_номеров": len(numbers),
                        "сессия_p50_с": round(statistics.median(r["сек"] for r in ok), 2) if ok else None,
                        "сессия_max_с": round(max(r["сек"] for r in ok), 2) if ok else None,
                        "баллы": sorted({r["балл"] for r in ok})}
    print_table(f"{args.parallel} одновременных вызовов", summ)
    print(summ["_условия"])
    return summ


# ─── Запись: сколько операций в секунду держит ────────────────────────

async def writer(rec, http, base, login_name, sid, deadline):
    tok = await login(rec, http, base, login_name)
    _, b = await rec.call(http, "сессия.начать", "POST", f"{base}/api/sessions", tok,
                          {"scenarioId": sid, "mode": "card112"})
    s = b["session"]["id"]
    n = 0
    while time.perf_counter() < deadline:
        # Каждая правка — запись карточки и строки аудита: ровно то, что
        # шлёт интерфейс при вводе
        await rec.call(http, "карточка.правка", "PATCH", f"{base}/api/sessions/{s}/card", tok,
                       {"description": f"уточнение {n}", "address": {"house": str(n % 90 + 1)}})
        n += 1
    return n


# Где база прогона: контейнер и имя базы. Задаются ключами --pg-*
PG = {"container": "arm112-db", "db": "arm112_load"}


async def pg_counters():
    proc = await asyncio.create_subprocess_exec(
        "docker", "exec", PG["container"], "psql", "-U", "arm112", "-d", "postgres", "-Atc",
        "SELECT xact_commit, tup_inserted, tup_updated, tup_deleted FROM pg_stat_database "
        f"WHERE datname = '{PG['db']}'", stdout=asyncio.subprocess.PIPE)
    out, _ = await proc.communicate()
    c, i, u, d = map(int, out.decode().strip().split("|"))
    return {"commits": c, "ins": i, "upd": u, "del": d}


async def writes(args):
    rec = Recorder()
    conn = aiohttp.TCPConnector(limit=0)
    async with aiohttp.ClientSession(connector=conn) as http:
        admin = await login(rec, http, args.base, "admin", "arm112")
        sid = await scenario_id(http, args.base, admin)
        # Статистика pg_stat_database копится с задержкой до полусекунды
        await asyncio.sleep(1)
        before = await pg_counters()
        t0 = time.perf_counter()
        deadline = t0 + args.seconds
        counts = await asyncio.gather(*[writer(rec, http, args.base, f"load-s{i:03}", sid, deadline)
                                        for i in range(args.parallel)])
        wall = time.perf_counter() - t0
        await asyncio.sleep(1.5)
        after = await pg_counters()
    summ = rec.summary()
    ok = summ.get("карточка.правка", {}).get("codes", {}).get(200, 0)
    rows = sum(after[k] - before[k] for k in ("ins", "upd", "del"))
    summ["_условия"] = {"параллельно": args.parallel, "секунд": round(wall, 1),
                        "правок_успешно": ok, "правок_в_с": round(ok / wall, 1),
                        "коммитов_в_базе_в_с": round((after["commits"] - before["commits"]) / wall, 1),
                        "строк_изменено_в_с": round(rows / wall, 1)}
    print_table(f"запись: {args.parallel} потоков правок, {args.seconds} с", summ)
    print(summ["_условия"])
    return summ


# ─── Отчёты на наполненной базе ──────────────────────────────────────

def server_seconds(job):
    """От постановки в очередь до готовности — по часам сервера."""
    from datetime import datetime
    if not job or not job.get("finishedAt"):
        return None
    a = datetime.fromisoformat(job["createdAt"].replace("Z", "+00:00"))
    b = datetime.fromisoformat(job["finishedAt"].replace("Z", "+00:00"))
    return round((b - a).total_seconds(), 2)


async def wait_job(rec, http, base, tok, job_id, timeout=120):
    t0 = time.perf_counter()
    while time.perf_counter() - t0 < timeout:
        _, j = await rec.call(http, "задача.состояние", "GET", f"{base}/api/jobs/{job_id}", tok)
        if j and j["status"] in ("done", "failed"):
            return j, time.perf_counter() - t0
        await asyncio.sleep(0.2)
    return None, timeout


async def reports(args):
    rec = Recorder()
    out = {}
    async with aiohttp.ClientSession() as http:
        tok = await login(rec, http, args.base, "teacher1", "arm112")
        _, groups = await rec.call(http, "группы", "GET", f"{args.base}/api/groups", tok)
        gid = groups[0]["id"]
        # Отчёт по вызову: свежий вызов курсанта из группы преподавателя
        admin = await login(rec, http, args.base, "admin", "arm112")
        sid = await scenario_id(http, args.base, admin)
        sess = (await one_session(rec, http, args.base, "load-s000", sid))["сессия"]
        for i in range(5):
            await rec.call(http, "отчёт.вызов.json", "GET", f"{args.base}/api/reports/session/{sess}", tok)
            await rec.call(http, "отчёт.вызов.csv", "GET",
                           f"{args.base}/api/reports/session/{sess}?format=csv", tok)
            await rec.call(http, "прогноз.группа", "GET", f"{args.base}/api/forecast/group/{gid}", tok)
            await rec.call(http, "рекомендации.группа", "GET", f"{args.base}/api/advice/group/{gid}", tok)
        for kind, path in (("отчёт.группа", "reports"), ("сводка.группа", "insights")):
            times = []
            for i in range(3):
                t0 = time.perf_counter()
                _, acc = await rec.call(http, f"{kind}.заказ", "POST",
                                        f"{args.base}/api/{path}/group/{gid}", tok,
                                        expect=(200, 202))
                if not acc:
                    print("заказ не принят:", rec.errors[-3:])
                    break
                job, _ = await wait_job(rec, http, args.base, tok, acc["jobId"])
                times.append({"клиент_с": round(time.perf_counter() - t0, 2),
                              "статус": job["status"] if job else "timeout",
                              "сервер_с": server_seconds(job)})
            out[kind] = times
    summ = rec.summary()
    summ["_задачи"] = out
    print_table("отчёты", summ)
    print(json.dumps(out, ensure_ascii=False, indent=1))
    return summ


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="http://127.0.0.1:8095")
    ap.add_argument("--out", default=None, help="куда записать сырые результаты (JSON)")
    ap.add_argument("--pg-container", default=PG["container"],
                    help="контейнер PostgreSQL — для счётчиков записи")
    ap.add_argument("--pg-db", default=PG["db"], help="база прогона в этом контейнере")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("setup")
    m = sub.add_parser("mixed")
    m.add_argument("--users", type=int, default=100)
    m.add_argument("--seconds", type=int, default=60)
    m.add_argument("--think", type=float, default=0.0,
                   help="пауза между действиями пользователя, с")
    s = sub.add_parser("sessions")
    s.add_argument("--parallel", type=int, default=50)
    w = sub.add_parser("writes")
    w.add_argument("--parallel", type=int, default=50)
    w.add_argument("--seconds", type=int, default=20)
    sub.add_parser("reports")
    args = ap.parse_args()
    PG.update(container=args.pg_container, db=args.pg_db)

    fn = {"setup": setup, "mixed": mixed, "sessions": sessions,
          "writes": writes, "reports": reports}[args.cmd]
    summ = asyncio.run(fn(args))
    if args.out and summ:
        Path(args.out).write_text(json.dumps(summ, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
