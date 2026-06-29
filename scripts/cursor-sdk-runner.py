#!/usr/bin/env python3
"""Cursor SDK sidecar for FinallyAValueBot's Cursor agent engine.

Requires on the host:
  pip install cursor-sdk aiohttp

Environment:
  CURSOR_API_KEY   User API key from Cursor Dashboard → Integrations

API:
  GET  /health
  POST /run
    Body: {"prompt": "...", "cwd": "...", "model": "composer-2.5", "agent_id": "..."}
    Response: NDJSON stream
      {"type":"text","text":"..."}
      {"type":"done","status":"finished","agent_id":"...","result":"..."}
      {"type":"error","message":"..."}
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
from typing import Any, AsyncIterator

try:
    from aiohttp import web
except ImportError as exc:  # pragma: no cover
    print(
        "cursor-sdk-runner requires aiohttp: pip install aiohttp cursor-sdk",
        file=sys.stderr,
    )
    raise SystemExit(1) from exc

DEFAULT_PORT = 3848
DEFAULT_MODEL = "composer-2.5"


def _api_key() -> str:
    return os.environ.get("CURSOR_API_KEY", "").strip()


def _is_stale_agent_error(err: Exception) -> bool:
    msg = str(getattr(err, "message", err)).lower()
    return "not found" in msg and "agent" in msg


def _open_agent(
    *,
    agent_id: str | None,
    model: str,
    cwd: str,
    api_key: str,
    opts: Any,
):
    from cursor_sdk import Agent, LocalAgentOptions

    if not agent_id:
        return Agent.create(
            model=model,
            api_key=api_key,
            local=LocalAgentOptions(cwd=cwd),
        )

    try:
        return Agent.resume(agent_id, opts)
    except Exception as err:
        from cursor_sdk import CursorAgentError

        if isinstance(err, CursorAgentError) and _is_stale_agent_error(err):
            return Agent.create(
                model=model,
                api_key=api_key,
                local=LocalAgentOptions(cwd=cwd),
            )
        raise


async def _stream_run(body: dict[str, Any]) -> AsyncIterator[str]:
    prompt = (body.get("prompt") or "").strip()
    cwd = (body.get("cwd") or ".").strip() or "."
    model = (body.get("model") or DEFAULT_MODEL).strip() or DEFAULT_MODEL
    agent_id = (body.get("agent_id") or "").strip() or None

    if not prompt:
        yield json.dumps({"type": "error", "message": "prompt required"}) + "\n"
        return

    api_key = _api_key()
    if not api_key:
        yield json.dumps(
            {"type": "error", "message": "CURSOR_API_KEY is not set on the sidecar host"}
        ) + "\n"
        return

    try:
        from cursor_sdk import AgentOptions, CursorAgentError
    except ImportError:
        yield json.dumps(
            {
                "type": "error",
                "message": "cursor-sdk is not installed (pip install cursor-sdk)",
            }
        ) + "\n"
        return

    opts = AgentOptions(api_key=api_key)
    resume_id = agent_id

    for attempt in (0, 1):
        try:
            agent_ctx = _open_agent(
                agent_id=resume_id,
                model=model,
                cwd=cwd,
                api_key=api_key,
                opts=opts,
            )
            for line in _stream_agent_turn(agent_ctx, prompt, resume_id):
                yield line
            return
        except CursorAgentError as err:
            if attempt == 0 and resume_id and _is_stale_agent_error(err):
                resume_id = None
                continue
            yield json.dumps(
                {
                    "type": "error",
                    "message": f"Cursor SDK startup failed: {getattr(err, 'message', err)}",
                }
            ) + "\n"
            return
        except Exception as err:  # pragma: no cover - surfaced to bot logs
            yield json.dumps({"type": "error", "message": str(err)}) + "\n"
            return


def _stream_agent_turn(
    agent_ctx: Any,
    prompt: str,
    resume_id: str | None,
):
    final_text_parts: list[str] = []
    returned_agent_id: str | None = resume_id
    status = "error"

    with agent_ctx as agent:
        returned_agent_id = getattr(agent, "agent_id", None) or resume_id
        run = agent.send(prompt)
        for message in run.messages():
            msg_type = getattr(message, "type", None)
            if msg_type != "assistant":
                continue
            payload = getattr(message, "message", None)
            if payload is None:
                continue
            for block in getattr(payload, "content", []) or []:
                if getattr(block, "type", None) != "text":
                    continue
                text = getattr(block, "text", "") or ""
                if not text:
                    continue
                final_text_parts.append(text)
                yield json.dumps({"type": "text", "text": text}) + "\n"

        result = run.wait()
        status = getattr(result, "status", None) or "finished"
        result_text = getattr(result, "result", None)
        if isinstance(result_text, str) and result_text.strip():
            if not final_text_parts:
                final_text_parts.append(result_text)
        returned_agent_id = getattr(agent, "agent_id", None) or returned_agent_id

    yield json.dumps(
        {
            "type": "done",
            "status": status,
            "agent_id": returned_agent_id,
            "result": "".join(final_text_parts),
        }
    ) + "\n"


async def handle_health(_request: web.Request) -> web.Response:
    cursor_sdk_installed = False
    try:
        import cursor_sdk  # noqa: F401

        cursor_sdk_installed = True
    except ImportError:
        pass
    return web.json_response(
        {
            "ok": True,
            "service": "cursor-sdk-runner",
            "api_key_configured": bool(_api_key()),
            "cursor_sdk_installed": cursor_sdk_installed,
        }
    )


async def handle_run(request: web.Request) -> web.StreamResponse:
    try:
        body = await request.json()
    except json.JSONDecodeError as exc:
        return web.json_response(
            {"type": "error", "message": f"invalid JSON: {exc}"},
            status=400,
        )

    response = web.StreamResponse(
        status=200,
        reason="OK",
        headers={"Content-Type": "application/x-ndjson"},
    )
    await response.prepare(request)

    async for line in _stream_run(body if isinstance(body, dict) else {}):
        await response.write(line.encode("utf-8"))

    await response.write_eof()
    return response


async def handle_models(_request: web.Request) -> web.Response:
    api_key = _api_key()
    if not api_key:
        return web.json_response(
            {"ok": False, "error": "CURSOR_API_KEY is not set on the sidecar host"},
            status=503,
        )
    try:
        from cursor_sdk import Cursor
    except ImportError:
        return web.json_response(
            {"ok": False, "error": "cursor-sdk is not installed (pip install cursor-sdk)"},
            status=503,
        )
    try:
        models = Cursor.models.list(api_key=api_key)
        payload = []
        for m in models or []:
            model_id = getattr(m, "id", None) or getattr(m, "model", None)
            if model_id:
                payload.append({"id": str(model_id)})
        return web.json_response({"ok": True, "models": payload})
    except Exception as err:  # pragma: no cover
        return web.json_response({"ok": False, "error": str(err)}, status=502)


def main() -> None:
    port = int(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_PORT
    app = web.Application()
    app.router.add_get("/health", handle_health)
    app.router.add_get("/models", handle_models)
    app.router.add_post("/run", handle_run)
    print(f"Cursor SDK runner listening on 0.0.0.0:{port}", file=sys.stderr)
    web.run_app(app, host="0.0.0.0", port=port, print=None)


if __name__ == "__main__":
    main()
