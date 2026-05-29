#!/usr/bin/env python3
import json
import re
import sys

PZ_ID_RE = re.compile(r"PZ-\d{8}(?:-[A-Za-z0-9_]+)?")
TERMINAL_KEYWORDS = ("published", "scrapped", "completed", "cancelled", "canceled")


def main() -> int:
    raw = sys.stdin.read()
    try:
        payload = json.loads(raw) if raw.strip() else {}
    except Exception:
        print(json.dumps({"permission": "allow"}))
        return 0

    tool_name = str(payload.get("tool_name") or "")
    tool_input = payload.get("tool_input") or {}
    tool_output = str(payload.get("tool_output") or "")
    tool_is_error = bool(payload.get("tool_is_error"))

    if tool_is_error:
        print(json.dumps({"permission": "allow"}))
        return 0

    input_json = ""
    try:
        input_json = json.dumps(tool_input, ensure_ascii=False)
    except Exception:
        input_json = ""

    combined = f"{tool_name}\n{input_json}\n{tool_output}".lower()
    if not any(k in combined for k in TERMINAL_KEYWORDS):
        print(json.dumps({"permission": "allow"}))
        return 0

    ids = sorted(set(PZ_ID_RE.findall(f"{tool_name}\n{input_json}\n{tool_output}")))
    if not ids:
        print(json.dumps({"permission": "allow"}))
        return 0

    print(
        json.dumps(
            {
                "permission": "allow",
                "effects": {
                    "memory_tier3_prune": {"terminal_pz_post_ids": ids},
                },
            }
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
