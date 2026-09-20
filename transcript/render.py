#!/usr/bin/env python3
"""Превращает журнал сессии Claude Code в читаемый Markdown.

Из сырого JSONL берём только разговор: реплики человека, текст ответов
и однострочные пометки о том, какие инструменты вызывались. Результаты
вызовов (вывод команд, содержимое файлов) выбрасываем — их там 32 МБ,
и читать это невозможно.
"""
import json, re, sys
from datetime import datetime

SRC = sys.argv[1]
DST = sys.argv[2]

def clean(text):
    """Убрать служебные вставки, которых человек не писал."""
    text = re.sub(r"<system-reminder>.*?</system-reminder>", "", text, flags=re.S)
    text = re.sub(r"<command-(name|message|args)>.*?</command-\1>", "", text, flags=re.S)
    text = re.sub(r"<local-command-stdout>.*?</local-command-stdout>", "", text, flags=re.S)
    return text.strip()

def blocks(content):
    """Нормализуем content: он бывает строкой и списком блоков."""
    if isinstance(content, str):
        return [{"type": "text", "text": content}]
    return content if isinstance(content, list) else []

out = []
stats = {"human": 0, "assistant": 0, "tools": 0}
pending_tools = []

def flush_tools():
    """Инструменты сворачиваем в одну строку на серию."""
    if not pending_tools:
        return
    uniq = []
    for name in pending_tools:
        if not uniq or uniq[-1][0] != name:
            uniq.append([name, 1])
        else:
            uniq[-1][1] += 1
    parts = [n if c == 1 else f"{n}×{c}" for n, c in uniq]
    out.append(f"> *инструменты: {', '.join(parts)}*\n")
    pending_tools.clear()

with open(SRC, encoding="utf-8") as f:
    for line in f:
        try:
            rec = json.loads(line)
        except json.JSONDecodeError:
            continue

        kind = rec.get("type")
        msg = rec.get("message") or {}
        if kind not in ("user", "assistant") or not msg:
            continue

        bs = blocks(msg.get("content"))

        if kind == "user":
            # Результаты вызовов инструментов приходят тем же типом —
            # это не реплика человека
            if any(b.get("type") == "tool_result" for b in bs):
                continue
            text = clean("\n".join(b.get("text", "") for b in bs if b.get("type") == "text"))
            # Системные пометки об отмене — не реплики человека
            if not text or text.startswith("[Request interrupted"):
                continue
            flush_tools()
            ts = rec.get("timestamp", "")
            when = ""
            if ts:
                try:
                    when = datetime.fromisoformat(ts.replace("Z", "+00:00")).strftime("%d.%m %H:%M")
                except ValueError:
                    pass
            out.append(f"\n---\n\n## 🧑 Денис  <sub>{when}</sub>\n\n{text}\n")
            stats["human"] += 1

        else:
            said = []
            for b in bs:
                t = b.get("type")
                if t == "text" and b.get("text", "").strip():
                    said.append(b["text"].strip())
                elif t == "tool_use":
                    pending_tools.append(b.get("name", "?"))
                    stats["tools"] += 1
            if said:
                flush_tools()
                out.append("\n### 🤖 Claude\n\n" + "\n\n".join(said) + "\n")
                stats["assistant"] += 1

flush_tools()

head = f"""# Разработка тренажёра оператора ДДС — рабочий диалог

Хакатон ЛЦТ, задача №9 Департамента ГОЧСиПБ Москвы.
Полный журнал сессии: `session.jsonl` (32 МБ, включая вывод всех команд).

Здесь только разговор: **{stats['human']}** реплик человека,
**{stats['assistant']}** ответов, **{stats['tools']}** вызовов инструментов
(свёрнуты в пометки, их вывод выброшен).

Внутренние рассуждения модели не включены.
"""

with open(DST, "w", encoding="utf-8") as f:
    f.write(head + "".join(out))

print(f"реплик человека: {stats['human']}, ответов: {stats['assistant']}, "
      f"вызовов инструментов: {stats['tools']}")
