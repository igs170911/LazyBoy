"""Product-owned model routing; Hermes retains the native model/tool loop."""
import asyncio
import copy
import math
import time
from urllib.parse import urlparse

import httpx

DEFAULT_ROUTING = {
    "enabled": True, "tool_groups": "auto", "chat": None, "work": None, "fallbacks": [],
    "chat_timeout_ms": 15000, "work_timeout_ms": 180000,
    "semantic": False, "laya": False, "classifier_shadow": True,
    "classifier_url": "http://127.0.0.1:8778", "classifier_timeout_ms": 250,
    "min_confidence": 0.9, "gateway_provider": None, "gateway_chat_pool": [], "gateway_work_pool": [],
}
INTENTS = {"answer", "native", "browser", "desktop", "work"}
EFFORTS = {"", "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"}


def model_row(provider, model):
    return next((r if isinstance(r, dict) else {"id": r} for r in provider.get("models", [])
                 if (r.get("id") if isinstance(r, dict) else r) == model), {})


def validate_routing(value, config):
    if not isinstance(value, dict) or set(value) - set(DEFAULT_ROUTING):
        raise ValueError("無效的分流設定欄位")
    result = {**copy.deepcopy(DEFAULT_ROUTING), **value}
    for key in ("enabled", "semantic", "laya", "classifier_shadow"):
        if type(result[key]) is not bool: raise ValueError("分流開關須為布林值")
    if result["tool_groups"] not in {"0", "1", "auto"}: raise ValueError("無效的工具清單模式")
    for key, low, high in (("chat_timeout_ms", 100, 60000), ("work_timeout_ms", 1000, 300000), ("classifier_timeout_ms", 20, 1000)):
        if type(result[key]) is not int or not low <= result[key] <= high:
            raise ValueError(f"{key} 須為 {low}–{high} 毫秒")
    confidence = result["min_confidence"]
    if type(confidence) not in (int, float) or not math.isfinite(confidence) or not .5 <= confidence <= 1:
        raise ValueError("分類接受門檻須為 0.5–1")
    url = urlparse(result["classifier_url"] if isinstance(result["classifier_url"], str) else "")
    if url.scheme not in {"http", "https"} or url.hostname not in {"localhost", "127.0.0.1", "::1"} or url.username or url.password or url.query or url.fragment:
        raise ValueError("分類器須使用沒有憑證的本機 HTTP 網址")
    targets = []
    for key in ("chat", "work", "fallbacks", "gateway_chat_pool", "gateway_work_pool"):
        rows = result[key]
        if key in {"chat", "work"}: rows = [] if rows is None else [rows]
        elif not isinstance(rows, list) or len(rows) > 3: raise ValueError("每組最多三個備援或部署模型")
        for row in rows:
            if not isinstance(row, dict) or set(row) - {"provider", "model", "reasoning_effort"}:
                raise ValueError("無效的分流模型")
            provider = config["providers"].get(row.get("provider") or config["active"], {})
            if not isinstance(row.get("model"), str) or not model_row(provider, row["model"]):
                raise ValueError("分流模型須在已設定供應商的模型清單中")
            endpoint = urlparse(provider.get("base_url", ""))
            if endpoint.scheme not in {"http", "https"} or not endpoint.hostname or endpoint.username or endpoint.password:
                raise ValueError("分流供應商須設定 HTTP 服務網址")
            if row.get("reasoning_effort") is not None and (not isinstance(row["reasoning_effort"], str) or row["reasoning_effort"] not in EFFORTS):
                raise ValueError("無效的分流推理強度")
            targets.append((provider, row["model"]))
    gateway = result["gateway_provider"]
    if gateway is not None:
        if not isinstance(gateway, str): raise ValueError("無效的模型閘道")
        provider = config["providers"].get(gateway, {})
        if not all(model_row(provider, model) for model in ("lazyboy-chat", "lazyboy-work")):
            raise ValueError("閘道須提供 lazyboy-chat 與 lazyboy-work")
        targets.extend((provider, model) for model in ("lazyboy-chat", "lazyboy-work"))
    # A running native SDK has one wire transport. Never silently rewrite another
    # transport's messages or opaque reasoning to approximate that protocol.
    if result["enabled"] and targets:
        protocols = {model_row(p, m).get("protocol", "chat_completions") for p, m in targets}
        if result["chat"] is None or result["work"] is None:
            active = config["providers"].get(config["active"], {})
            if config.get("model"): protocols.add(model_row(active, config["model"]).get("protocol", "chat_completions"))
        if len(protocols) > 1:
            raise ValueError("同一組聊天、工作與備援模型須使用相同 API 介面；Bot 仍可各自選擇不同介面")
    return result


def deterministic_intent(text):
    text = text.strip().lower()
    groups = (
        ("desktop", ("桌面", "螢幕", "屏幕", "desktop", "computer action")),
        ("work", ("完整實作", "實作完成", "重構", "深入研究", "完整研究", "refactor", "implement", "comprehensive research")),
        ("browser", ("登入", "登录", "瀏覽器", "browser", "log in", "login")),
        ("native", ("今天", "最新", "天氣", "查詢", "搜尋", "本機", "讀取", "執行", "today", "latest", "weather", "search", "read file", "local file", "run command", "http://", "https://")),
    )
    for intent, words in groups:
        if any(word in text for word in words): return intent
    if text.rstrip("!！.。") in {"你好", "嗨", "哈囉", "謝謝", "谢谢", "hello", "hi", "thanks"}: return "answer"
    return None


class TurnRoute:
    def __init__(self, settings, agent, config, intent, role, enabled, vision=False):
        self.settings, self.config = settings, config
        self.routing = {**DEFAULT_ROUTING, **config.get("routing", {})}
        provider = agent.get("provider") or config["active"]
        model = agent.get("model") or config.get("model", "")
        self.base = provider, model, copy.deepcopy(config["providers"].get(provider, {}))
        self.intent, self.role, self.enabled, self.vision = intent, role, enabled, vision
        self.last = None
        self.promoted = False
        self.input_binding = self.primary()
        self.input_provider = self.input_binding[0]
        self.prompt = ""

    def executed_tools(self, payload):
        """Tool results also prove promotion when WS notifications lag HTTP."""
        rows = payload.get("messages", payload.get("input", []))
        if not self.prompt or not isinstance(rows, list): return False
        def text(message):
            content = message.get("content", "")
            return content if isinstance(content, str) else "\n".join(p.get("text", "") for p in content if isinstance(p, dict)) if isinstance(content, list) else ""
        start = max((i for i,m in enumerate(rows) if isinstance(m, dict) and m.get("role") == "user" and text(m).startswith(self.prompt)), default=-1)
        if start < 0: return False
        calls = {}
        outputs = []
        for message in rows[start+1:]:
            if not isinstance(message, dict): continue
            for call in message.get("tool_calls", []): calls[call.get("id")] = call.get("function", {}).get("name")
            if message.get("role") == "tool": outputs.append(message.get("tool_call_id"))
            if message.get("type") == "function_call": calls[message.get("call_id")] = message.get("name")
            if message.get("type") == "function_call_output": outputs.append(message.get("call_id"))
            if isinstance(message.get("content"), list):
                for part in message["content"]:
                    if not isinstance(part, dict): continue
                    if part.get("type") == "tool_use": calls[part.get("id")] = part.get("name")
                    if part.get("type") == "tool_result": outputs.append(part.get("tool_use_id"))
        return any(calls.get(key) and calls[key] not in {"clarify", "todo", "send_message", "report_progress"} for key in outputs)

    def resolve(self, target):
        if not target: return copy.deepcopy(self.base)
        provider = target.get("provider") or self.base[0]
        model = target["model"]
        selected = copy.deepcopy(self.config["providers"][provider])
        if target.get("reasoning_effort") is not None:
            for row in selected.get("models", []):
                if isinstance(row, dict) and row.get("id") == model: row["reasoning_effort"] = target["reasoning_effort"]
        return provider, model, selected

    def primary(self):
        if not self.enabled: return copy.deepcopy(self.base)
        gateway = self.routing.get("gateway_provider")
        if gateway: return self.resolve({"provider": gateway, "model": "lazyboy-" + self.role})
        return self.resolve(self.routing[self.role])

    def promote(self):
        if self.enabled and self.role == "chat":
            self.role, self.promoted, self.last = "work", True, None

    def candidates(self):
        rows = [self.last or self.primary()]
        if self.enabled:
            if self.role == "chat": rows.append(self.resolve(self.routing["work"]))
            rows.extend(self.resolve(t) for t in self.routing["fallbacks"])
        seen, result = set(), []
        for binding in rows:
            p, m, selected = binding
            key = p, m, self.settings.reasoning(selected, m)
            if key in seen or (self.vision and not model_row(selected, m).get("vision")): continue
            seen.add(key); result.append(binding)
        return result

    def deadline(self, binding):
        if not self.enabled: return 300
        role = "chat" if self.role == "chat" and binding[:2] == self.primary()[:2] and self.routing["chat"] else "work"
        return self.routing[role + "_timeout_ms"] / 1000


class Router:
    def __init__(self, settings):
        self.settings = settings
        self.cooldowns = {}
        self.observers = set()

    async def classify(self, routing, text):
        url = routing["classifier_url"].rstrip("/")
        if self.cooldowns.get(url, 0) > time.monotonic(): return None
        timeout = routing["classifier_timeout_ms"] / 1000
        try:
            async with asyncio.timeout(timeout):
                async with httpx.AsyncClient(timeout=timeout) as client:
                    response = await client.post(url + "/v1/route", json={"text": text[:2000], "semantic": routing["semantic"], "laya": routing["laya"], "min_confidence": routing["min_confidence"], "deadline_ms": routing["classifier_timeout_ms"]})
                    response.raise_for_status(); body = response.json()
            confidence = body.get("confidence")
            if body.get("abstained") or type(confidence) not in (int, float) or not math.isfinite(confidence) or confidence < routing["min_confidence"]:
                return None
            return body.get("route") if body.get("route") in INTENTS else None
        except (httpx.HTTPError, TimeoutError, ValueError, TypeError, AttributeError):
            self.cooldowns[url] = time.monotonic() + 30
            return None

    async def select(self, agent, text, *, routine=False, vision=False, emit=None):
        config = self.settings.read()
        routing = {**DEFAULT_ROUTING, **config.get("routing", {})}
        enabled = routing["enabled"] and not (agent.get("provider") or agent.get("model"))
        hint = deterministic_intent(text)
        continuation = text.strip().lower().rstrip("!！.。") in {"繼續", "继续", "接著做", "continue", "resume", "go on"}
        forced_work = bool(routine or continuation or vision)
        suggestion = None
        if enabled and not forced_work and hint is None and (routing["semantic"] or routing["laya"]):
            if routing["classifier_shadow"]:
                async def observe():
                    started = time.monotonic()
                    suggested = await self.classify(routing, text)
                    if emit: emit("classifier_shadow", {"suggested": suggested, "elapsed_ms": round((time.monotonic() - started)*1000)})
                task = asyncio.create_task(observe())
                self.observers.add(task); task.add_done_callback(self.observers.discard)
            else: suggestion = await self.classify(routing, text)
        intent = "work" if forced_work else hint or suggestion or "native"
        # With an active classifier, uncertainty belongs to the work model.
        uncertain = hint is None and suggestion is None and not routing["classifier_shadow"] and (routing["semantic"] or routing["laya"])
        role = "work" if forced_work or intent in {"work", "desktop"} or uncertain else "chat"
        route = TurnRoute(self.settings, agent, config, intent, role, enabled, vision)
        route.prompt = text
        if emit: emit("routing", {"intent": intent, "role": role, "enabled": enabled, "shadow": routing["classifier_shadow"]})
        return route

    async def health(self):
        routing = {**DEFAULT_ROUTING, **self.settings.read().get("routing", {})}
        result = {"ready": True, "supported": True, "backend": "hermes", "engines": {"semantic": "disabled", "laya": "disabled"}}
        if routing["semantic"] or routing["laya"]:
            try:
                async with asyncio.timeout(.25):
                    async with httpx.AsyncClient(timeout=.25) as client:
                        response = await client.get(routing["classifier_url"].rstrip("/") + "/health")
                        body = response.json()
                result["engines"] = {key: body.get("engines", {}).get(key, "unavailable") if routing[key] else "disabled" for key in ("semantic", "laya")}
            except (httpx.HTTPError, TimeoutError, ValueError, AttributeError):
                result["engines"] = {key: "unavailable" if routing[key] else "disabled" for key in ("semantic", "laya")}
        result["ready"] = all(status in {"ready", "disabled"} for status in result["engines"].values())
        return result

    async def close(self):
        for task in self.observers: task.cancel()
        await asyncio.gather(*self.observers, return_exceptions=True)


def routed_payload(payload, binding, protocol, settings, *, other_provider=False):
    """Keep tool IDs/results; discard opaque reasoning when changing models."""
    result = copy.deepcopy(payload)
    _, model, provider = binding
    changed = result.get("model") != model or other_provider
    result["model"] = model
    if changed:
        for message in result.get("messages", []):
            if not isinstance(message, dict): continue
            message.pop("reasoning_content", None); message.pop("reasoning", None)
            if isinstance(message.get("content"), list):
                message["content"] = [p for p in message["content"] if not isinstance(p, dict) or p.get("type") not in {"thinking", "redacted_thinking"}]
        if isinstance(result.get("messages"), list):
            result["messages"] = [m for m in result["messages"] if not isinstance(m, dict) or m.get("content") != [] or m.get("tool_calls")]
        if isinstance(result.get("input"), list):
            result["input"] = [p for p in result["input"] if not isinstance(p, dict) or p.get("type") not in {"reasoning", "compaction"}]
    row = model_row(provider, model)
    effort = row.get("reasoning_effort")
    if changed and protocol == "chat_completions":
        result.pop("reasoning_effort", None); result.pop("reasoning", None)
    if changed and protocol == "responses":
        result.pop("reasoning", None)
    if effort is not None:
        if protocol == "chat_completions":
            result.pop("reasoning", None)
            if effort: result["reasoning_effort"] = effort
            else: result.pop("reasoning_effort", None)
        elif protocol == "responses":
            if effort: result["reasoning"] = {**result.get("reasoning", {}), "effort": effort}
            else: result.pop("reasoning", None)
        elif protocol == "messages":
            messages_reasoning(result, model, effort)
    elif changed and protocol == "messages":
        messages_reasoning(result, model, settings.reasoning(provider, model))
    return result


def messages_reasoning(body, model, effort):
    """The pinned Hermes adapter's thinking policy, applied to routed requests."""
    name = model.lower().replace(".", "-")
    legacy = ("claude-3", "claude-opus-4-0", "claude-opus-4-1", "claude-sonnet-4-0", "claude-opus-4-2025", "claude-sonnet-4-2025", "claude-opus-4-5", "claude-sonnet-4-5", "claude-haiku-4-5")
    adaptive = ("claude" in name and not any(v in name for v in legacy)) or "kimi" in name or "moonshot" in name
    for key in ("thinking", "output_config", "reasoning", "reasoning_effort"): body.pop(key, None)
    if "claude" in name and adaptive and not any(v in name for v in ("claude-opus-4-6", "claude-sonnet-4-6")):
        for key in ("temperature", "top_p", "top_k"): body.pop(key, None)
    if "haiku" in name or not effort: return
    if effort == "none":
        if "claude-sonnet-5-5" in name: body["thinking"] = {"type": "between_tools"}
        elif adaptive and "claude" in name and not any(v in name for v in ("claude-fable", "claude-opus-5-5")):
            body["thinking"] = {"type": "disabled"}
    elif adaptive:
        level = {"minimal":"low", "ultra":"max"}.get(effort, effort)
        if level == "xhigh" and any(v in name for v in ("claude-opus-4-6", "claude-sonnet-4-6")): level = "max"
        body["thinking"] = {"type":"adaptive", "display":"summarized"}
        body["output_config"] = {"effort":level}
    else:
        budget = {"xhigh":32000,"high":16000,"medium":8000,"low":4000}.get(effort, 8000)
        body.update(thinking={"type":"enabled","budget_tokens":budget}, temperature=1, max_tokens=max(body.get("max_tokens", 0), budget+4096))
