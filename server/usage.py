"""Model usage per provider, counted where every model request passes: the product's provider proxy.

Each response's own `usage` block is read as it streams by, in whichever shape the protocol uses (Chat Completions,
Responses, Anthropic Messages), so the counts include work no task records: background workers, context
compression, titles. Totals are kept per day, provider and model; the proxy never waits on this bookkeeping.
"""
import json
from datetime import datetime, timedelta, timezone

BUCKET = "model_usage"
KEEP_DAYS = 400
JSON_LIMIT = 4 * 1024 * 1024
FIELDS = ("input", "output", "cached", "reasoning")


def day(at=None):
    return (at or datetime.now().astimezone()).strftime("%Y-%m-%d")


def normalize(usage):
    """One protocol's usage block as {input, output, cached, reasoning, cost}; input always includes cached input."""
    if not isinstance(usage, dict): return {}
    number = lambda value: value if isinstance(value, (int, float)) and not isinstance(value, bool) and value >= 0 else 0
    details = lambda key: usage.get(key) if isinstance(usage.get(key), dict) else {}
    cache_read, cache_write = number(usage.get("cache_read_input_tokens")), number(usage.get("cache_creation_input_tokens"))
    if "prompt_tokens" in usage:
        result = {"input": number(usage["prompt_tokens"]), "output": number(usage.get("completion_tokens")),
                  "cached": number(details("prompt_tokens_details").get("cached_tokens")) or cache_read,
                  "reasoning": number(details("completion_tokens_details").get("reasoning_tokens"))}
    else:
        # Responses counts cached tokens inside input_tokens; Anthropic Messages counts them beside it.
        result = {"input": number(usage.get("input_tokens")) + cache_read + cache_write, "output": number(usage.get("output_tokens")),
                  "cached": number(details("input_tokens_details").get("cached_tokens")) or cache_read,
                  "reasoning": number(details("output_tokens_details").get("reasoning_tokens"))}
    cost = usage.get("cost")
    if isinstance(cost, (int, float)) and not isinstance(cost, bool) and cost >= 0: result["cost"] = float(cost)
    return result


def usage_in(event):
    """The usage block carried by one response body or stream event, if any."""
    if not isinstance(event, dict): return None
    for holder in (event, event.get("response"), event.get("message")):
        if isinstance(holder, dict) and isinstance(holder.get("usage"), dict): return holder["usage"]
    return None


class Meter:
    """Reads one response as it passes through and keeps the largest count seen for each field.

    Streams report usage once at the end (Chat Completions, Responses) or as running totals (Anthropic), so the
    largest value of each field is the request's total either way.
    """

    def __init__(self, streaming):
        self.streaming, self.buffer, self.size, self.seen = streaming, b"", 0, {}

    def feed(self, chunk):
        if self.streaming:
            self.buffer += chunk
            *lines, self.buffer = self.buffer.split(b"\n")
            for line in lines: self.line(line)
            # response.completed repeats the whole response (instructions, tools, output) on one line, which can run
            # to hundreds of KB; only an absurd line is given up on.
            if len(self.buffer) > JSON_LIMIT: self.buffer = b""
        elif self.size <= JSON_LIMIT:
            self.size += len(chunk)
            self.buffer = self.buffer + chunk if self.size <= JSON_LIMIT else b""

    def line(self, line):
        line = line.strip()
        if not line.startswith(b"data:") or b'"usage"' not in line: return
        try: self.take(json.loads(line[5:]))
        except ValueError: pass

    def take(self, event):
        for key, value in normalize(usage_in(event)).items():
            self.seen[key] = max(self.seen.get(key, 0), value)

    def result(self):
        if self.streaming: self.line(self.buffer)
        elif self.buffer:
            try: self.take(json.loads(self.buffer))
            except ValueError: pass
        self.buffer = b""
        return self.seen


def record(store, provider, model, counts, ok=True, at=None):
    """Add one request to today's row for this provider and model."""
    key = f"{day(at)}|{provider}|{model}"
    with store.lock: _add(store, key, provider, model, counts, ok, at)


def _add(store, key, provider, model, counts, ok, at):
    row = store.get(BUCKET, key) or {"day": day(at), "provider": provider, "model": model, "requests": 0, "errors": 0, **{f: 0 for f in FIELDS}}
    row["requests"] += 1
    if not ok: row["errors"] = row.get("errors", 0) + 1
    for field in FIELDS: row[field] = row.get(field, 0) + int(counts.get(field) or 0)
    if counts.get("cost"): row["cost"] = round(row.get("cost", 0) + counts["cost"], 6)
    store.put(BUCKET, key, row)


def summary(store, days=30, providers=None):
    """Totals per provider (with each model inside) and per day, for the last `days` days."""
    days = max(1, min(int(days), 365))
    since = day(datetime.now().astimezone() - timedelta(days=days - 1))
    rows = [r for r in store.all(BUCKET) if r.get("day", "") >= since]
    labels = {id: (p.get("label") or id) for id, p in (providers or {}).items()}
    out = {}
    for r in rows:
        p = out.setdefault(r["provider"], {"provider": r["provider"], "label": labels.get(r["provider"], r["provider"]), "configured": r["provider"] in labels,
                                           "requests": 0, "errors": 0, **{f: 0 for f in FIELDS}, "models": {}, "days": {}})
        m = p["models"].setdefault(r["model"], {"model": r["model"], "requests": 0, **{f: 0 for f in FIELDS}})
        d = p["days"].setdefault(r["day"], 0)
        for target in (p, m):
            target["requests"] += r.get("requests", 0)
            for f in FIELDS: target[f] += r.get(f, 0)
            if r.get("cost"): target["cost"] = round(target.get("cost", 0) + r["cost"], 6)
        p["errors"] += r.get("errors", 0)
        p["days"][r["day"]] = d + r.get("input", 0) + r.get("output", 0)
    for p in out.values():
        p["models"] = sorted(p["models"].values(), key=lambda m: -(m["input"] + m["output"]))
        p["days"] = [{"day": k, "tokens": v} for k, v in sorted(p["days"].items())]
    return {"days": days, "since": since,
            "providers": sorted(out.values(), key=lambda p: -(p["input"] + p["output"]))}


def prune(store, keep_days=KEEP_DAYS):
    cutoff = day(datetime.now(timezone.utc).astimezone() - timedelta(days=keep_days))
    for r in store.all(BUCKET):
        if r.get("day", "") < cutoff: store.remove(BUCKET, f"{r['day']}|{r['provider']}|{r['model']}")
