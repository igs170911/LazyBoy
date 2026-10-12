"""Find out whether a provider model actually answers, and on which wire protocol.

Settings used to ask people to pick Chat Completions / Responses / Anthropic Messages from the
provider's docs. Instead, send one tiny request per protocol (the likely one first) and keep the
one that works; classify failures into a few causes people can act on.
"""
import re
import time
from urllib.parse import urlparse
import uuid

import httpx

PROTOCOLS = ("chat_completions", "responses", "messages")
# Bumped when checking itself was wrong, so stored results from the faulty version are re-run.
CHECK_VERSION = 3

# Hermes's per-model wire rules for the OpenCode relays (hermes_cli/models.py _OPENCODE_API_MODE_PREFIXES).
_OPENCODE_RULES = {
    "opencode-go": ((("gpt-", "grok-", "muse-spark"), "responses"), (("minimax-", "qwen", "union-alpha"), "messages")),
    "opencode-zen": ((("claude-", "union-alpha"), "messages"), (("gpt-", "grok-", "muse-spark"), "responses"), (("qwen",), "messages")),
}


def rule_protocol(provider_id, base_url, model):
    """The protocol Hermes's own rules give this model, or None when they do not decide it."""
    url, provider = str(base_url or "").lower(), str(provider_id or "").lower()
    if provider == "anthropic" or "api.anthropic.com" in url or re.search(r"/anthropic(/v1)?/?$", url): return "messages"
    family = "opencode-go" if provider == "opencode-go" or "opencode.ai/zen/go" in url else \
        "opencode-zen" if provider in {"opencode-zen", "opencode"} or "opencode.ai/zen" in url else None
    if not family: return None
    name = re.sub(r"^opencode-(zen|go)/", "", str(model or "").lower())
    return next((protocol for prefixes, protocol in _OPENCODE_RULES[family] if name.startswith(prefixes)), "chat_completions")
PATHS = {"chat_completions": "chat/completions", "responses": "responses", "messages": "messages"}

# Order matters: a complaint about a request parameter proves the endpoint knows the model.
_RULES = (
    ("ok", re.compile(r"max_(?:output_)?tokens|max_completion_tokens|temperature|unsupported (?:parameter|value)|unknown parameter", re.I)),
    ("protocol", re.compile(r"protocol|not support(?:ed)? (?:on|by|for) this (?:endpoint|api)|unsupported (?:endpoint|api|route)|no route|unknown (?:endpoint|url|path)|invalid url|use the (?:responses|messages|chat)", re.I)),
    ("unavailable", re.compile(r"model[^.]{0,40}(?:unavailable|not (?:found|available|exist|supported))|does not exist|no such model|unknown model|invalid model|model_not_found|not a valid model", re.I)),
    ("auth", re.compile(r"api[ _-]?key|unauthori[sz]ed|authentication|forbidden|permission|credit|balance|quota exceeded|insufficient", re.I)),
    ("busy", re.compile(r"rate.?limit|too many requests|overloaded|endpoint is unavailable|temporarily|try again|timeout|capacity", re.I)),
)

MESSAGES = {
    "ok": "可以使用",
    "protocol": "模型的 API 介面設定不對",
    "unavailable": "這個模型目前無法使用，請換一個模型",
    "auth": "API 金鑰無效或額度不足",
    "busy": "供應商暫時忙碌或無法連線，稍後再試",
    "unknown": "無法確認這個模型能不能用，詳細原因請看提示",
}


def classify(status, text=""):
    """Map an HTTP status and error text to ok / protocol / unavailable / auth / busy."""
    text = str(text or "")
    if status and 200 <= status < 300: return "ok"
    for kind, pattern in _RULES:
        if pattern.search(text): return kind
    if status in (401, 403): return "auth"
    if status == 429 or (status and status >= 500): return "busy"
    if status in (404, 405): return "protocol"
    # An error we cannot read is not proof the model is gone.
    return "unknown"


def friendly(text):
    """A plain-language first line for a failed task's raw provider/Hermes error, or None."""
    raw = str(text or "")
    if not re.search(r"HTTP \d{3}|Provider said|rate-limited|rejected this request", raw, re.I): return None
    status = re.search(r"HTTP (\d{3})", raw)
    kind = classify(int(status.group(1)) if status else None, raw)
    hints = {
        "protocol": "模型的 API 介面設定不對。到「設定 → 模型」選這個模型，會自動檢查並修正。",
        "unavailable": "這個模型目前無法使用，請到 Bot 設定或「設定 → 模型」換一個模型。",
        "auth": "模型供應商拒絕了金鑰，請到「設定 → 模型」確認 API 金鑰與額度。",
        "busy": "模型供應商暫時忙碌或無法連線，請稍後再交辦一次，或換一個模型。",
    }
    return hints.get(kind)


def ordered(guess):
    return [guess] + [p for p in PROTOCOLS if p != guess] if guess in PROTOCOLS else list(PROTOCOLS)


def _request(protocol, model, api_key, base=""):
    headers = {"content-type": "application/json", "User-Agent": "LazyBoy model check"}
    # OpenCode refuses requests without a session id (the task proxy sends the thread's).
    if (urlparse(base).hostname or "").endswith("opencode.ai"): headers["x-opencode-session"] = "lazyboy-check-" + uuid.uuid4().hex
    if api_key: headers["Authorization"] = "Bearer " + api_key
    if protocol == "messages":
        if api_key: headers["x-api-key"] = api_key
        headers["anthropic-version"] = "2023-06-01"
        body = {"model": model, "max_tokens": 16, "messages": [{"role": "user", "content": "hi"}]}
    elif protocol == "responses":
        body = {"model": model, "max_output_tokens": 16, "input": "hi"}
    else:
        body = {"model": model, "max_tokens": 16, "messages": [{"role": "user", "content": "hi"}]}
    return headers, body


def _error_text(response):
    try:
        data = response.json()
        error = data.get("error", data) if isinstance(data, dict) else data
        if isinstance(error, dict): error = error.get("message") or error
        return str(error)[:400]
    except ValueError:
        return response.text[:400]


async def check_model(base_url, api_key, model, guess="chat_completions", *, transport=None, timeout=20):
    """Try each protocol, likely one first. Returns {status, protocol, message, detail, at}."""
    base = str(base_url or "").rstrip("/")
    last = {"status": "unavailable", "detail": ""}
    first_failure = None
    async with httpx.AsyncClient(timeout=httpx.Timeout(timeout, connect=min(10, timeout)), transport=transport) as client:
        for protocol in ordered(guess):
            headers, body = _request(protocol, model, api_key, base)
            try:
                response = await client.post(f"{base}/{PATHS[protocol]}", headers=headers, json=body)
                detail = "" if response.is_success else _error_text(response)
                kind = classify(response.status_code, detail)
            except httpx.HTTPError as exc:
                kind, detail = "busy", str(exc) or type(exc).__name__
            if kind == "ok":
                return {"status": "ok", "protocol": protocol, "message": MESSAGES["ok"], "detail": "", "at": time.time(), "v": CHECK_VERSION}
            last = {"status": kind, "detail": detail}
            # A bad key, a busy provider or an error we cannot read says nothing about the protocol:
            # stop rather than land a working model on another protocol by accident.
            if kind in {"auth", "busy", "unknown"}: break
            if kind == "unavailable" and first_failure is None: first_failure = last
    # Every protocol refused: report what the likely protocol said about the model.
    result = first_failure if last["status"] == "protocol" and first_failure else last
    status = "unavailable" if result["status"] == "protocol" else result["status"]
    return {"status": status, "protocol": None, "message": MESSAGES[status], "detail": result["detail"], "at": time.time(), "v": CHECK_VERSION}
