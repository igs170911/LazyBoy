#!/usr/bin/env python3
"""Export aggregate telemetry only; never export prompts, accounts, or credentials.

Usage: python3 docs/benchmarks/product-latency-audit.py --db PATH --output PATH
Database access is read-only. Timing stages can overlap; their sums are not a
wall-clock decomposition. Samples are fixed historical cases, not an A/B test.
"""
import argparse
import collections
import datetime as dt
import json
import math
import pathlib
import sqlite3
import statistics


CASES = [
    ("A", "f208f42d-387d-4e07-827d-95e56980d020"),
    ("B", "222d1d75-e395-4745-9b51-0dc940aeeee0"),
    ("C", "5553556a-4151-462a-a756-6b6db3bb43ba"),
]


def elapsed(event):
    return event.get("elapsed_ms", event.get("value", 0))


def timestamp(value):
    return dt.datetime.fromisoformat(value.replace("Z", "+00:00"))


def audit(connection, label, task_id):
    task = json.loads(connection.execute(
        "SELECT data FROM tasks WHERE id = ?", (task_id,)
    ).fetchone()[0])
    stages = collections.defaultdict(list)
    tools = collections.defaultdict(list)
    runtime_counts = collections.Counter()
    failures = collections.Counter()
    for kind, raw in connection.execute(
        "SELECT kind, payload FROM events WHERE task = ? ORDER BY id", (task_id,)
    ):
        event = json.loads(raw)
        if kind == "timing":
            stages[event["stage"]].append(event)
            if event["stage"] == "tool_execution":
                tools[event["tool"]].append(elapsed(event))
        elif kind == "runtime":
            runtime_counts[event["type"]] += 1
            if event["type"] == "tool_finished" and not event["success"]:
                failures[event["name"]] += 1
    timing = {}
    for name in ["agent_turn", "model_response", "model_round_including_queue",
                 "request_preparation", "worker_model_queue", "tool_execution"]:
        samples = [elapsed(e) for e in stages[name]]
        timing[name] = {"count": len(samples), "sum_ms": sum(samples)}
        if samples:
            timing[name]["max_ms"] = max(samples)
    responses = sorted(elapsed(e) for e in stages["model_response"])
    timing["model_response"]["median_ms"] = statistics.median(responses)
    timing["model_response"]["mean_ms"] = round(statistics.mean(responses), 2)
    timing["model_response"]["p95_nearest_rank_ms"] = responses[math.ceil(len(responses) * .95) - 1]
    start = timestamp(task["session"]["created_at"])
    end = timestamp(task["session"]["updated_at"])
    wall_ms = round((end - start).total_seconds() * 1000)
    active_ms = timing["agent_turn"]["sum_ms"]
    models = set()
    for message in task["session"]["messages"]:
        source = message.get("model_source", "").split("|")
        if len(source) >= 3:
            models.add((source[0], source[2]))
    return {
        "case": label,
        "started_at_taipei": start.astimezone(dt.timezone(dt.timedelta(hours=8))).isoformat(),
        "ended_at_taipei": end.astimezone(dt.timezone(dt.timedelta(hours=8))).isoformat(),
        "research_mode": task["research"], "state": task["state"],
        "verdict": task["verdict"], "model_requests": task["requests"],
        "http_attempt_events": len(stages["model_http_attempt"]),
        "wall_ms": wall_ms, "timing": timing,
        "model_response_percent_of_active": round(timing["model_response"]["sum_ms"] / active_ms * 100, 2),
        "tool_sum_percent_of_active": round(timing["tool_execution"]["sum_ms"] / active_ms * 100, 2),
        "retained_transcript_models": [{"provider": p, "model": m} for p, m in sorted(models)],
        "runtime_counts": {k: runtime_counts[k] for k in ["tool_started", "tool_finished", "question", "steering", "turn_ended"]},
        "tools": [{"name": name, "calls": len(times), "sum_ms": sum(times), "failed": failures[name]}
                  for name, times in sorted(tools.items(), key=lambda pair: -sum(pair[1]))],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    database = pathlib.Path(args.db).resolve()
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as connection:
        result = {"audited_on": "2026-10-07", "sample_type": "three historical Shopee tasks",
                  "method": "full persisted events, read-only SQLite; task transcript may be compacted",
                  "caveats": ["timing sums may overlap and are not a wall-clock decomposition",
                              "helper spans are nested/background and excluded from tool totals",
                              "cancelled cases are censored observations, not completion times",
                              "retained models describe remaining transcript, not every historical model call",
                              "time outside agent_turn includes intermission and other overhead"],
                  "cases": [audit(connection, label, task_id) for label, task_id in CASES]}
    pathlib.Path(args.output).write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
