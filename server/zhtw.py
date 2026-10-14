"""Replies in Traditional Chinese as written in Taiwan, even when a model slips into Simplified mid-conversation.

Only text that clearly contains Simplified characters is converted (OpenCC s2twp: characters and Taiwan phrasing,
by context, so 佣金 and 公里 stay as they are). Characters that are just as correct in Traditional text never count
as evidence on their own, and 台 is kept rather than turned into the formal 臺. Code blocks and inline code are left
exactly as written, since a converted string or identifier would break the code.
"""
import re
from functools import lru_cache

from opencc import OpenCC

# Correct in Traditional text too, so seeing them proves nothing (台灣, 佣金, 公里, 划算, 社群, 高峰…).
AMBIGUOUS = set("佣台里划吃群峰着干后面周余只制系借准克卷尸")
EVIDENCE = 3
# s2twp's computing vocabulary is wrong for everyday writing: a parameter is 參數 to most people, not 引數.
KEEP = {"参数": "參數", "函数": "函數", "数据": "數據"}
KEEP_PATTERN = re.compile("|".join(KEEP))
CODE = re.compile(r"(```[\s\S]*?(?:```|$)|`[^`\n]+`)")


@lru_cache(maxsize=1)
def phrases():
    return OpenCC("s2twp")


@lru_cache(maxsize=1)
def characters():
    return OpenCC("s2tw")


@lru_cache(maxsize=4096)
def is_simplified(ch):
    return ch not in AMBIGUOUS and characters().convert(ch) != ch


def simplified_count(text):
    return sum(1 for ch in text if "一" <= ch <= "鿿" and is_simplified(ch))


def convert(part):
    out = phrases().convert(part)
    if len(out) != len(part): return out
    # Same length, so positions line up: put back each 台 (Taiwan writes 台灣, not 臺灣) and the everyday words.
    chars = [original if original == "台" else new for original, new in zip(part, out)]
    for match in KEEP_PATTERN.finditer(part):
        chars[match.start():match.end()] = KEEP[match.group(0)]
    return "".join(chars)


def to_traditional(text):
    """`text` with any Simplified Chinese turned into Taiwan Traditional; unchanged when there is none."""
    if not text or not isinstance(text, str): return text
    pieces = CODE.split(text)
    prose = "".join(piece for i, piece in enumerate(pieces) if i % 2 == 0)
    found = simplified_count(prose)
    if not found: return text
    # A stray Simplified character or two: fix just those characters, leaving the wording alone.
    fix = convert if found >= EVIDENCE else lambda part: "".join(characters().convert(ch) if is_simplified(ch) else ch for ch in part)
    return "".join(fix(piece) if i % 2 == 0 else piece for i, piece in enumerate(pieces))
