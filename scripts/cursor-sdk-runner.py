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
import hashlib
import json
import os
import sys
from dataclasses import dataclass, field
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
BRIDGE_RETRY_MAX_ATTEMPTS = 3
BRIDGE_RETRY_BACKOFF_SECS = (0.5, 1.5, 3.0)
DEFAULT_BRIDGE_LAUNCH_TIMEOUT_SECS = 60


def _bridge_launch_timeout_secs() -> int:
    raw = os.environ.get("CURSOR_BRIDGE_LAUNCH_TIMEOUT_SECS", "").strip()
    if not raw:
        return DEFAULT_BRIDGE_LAUNCH_TIMEOUT_SECS
    try:
        return max(30, int(raw))
    except ValueError:
        return DEFAULT_BRIDGE_LAUNCH_TIMEOUT_SECS


@dataclass
class _PooledBridge:
    client: Any
    cwd: str
    session_scope: str
    pool_key: str
    state_root: str
    lock: asyncio.Lock = field(default_factory=asyncio.Lock)


_POOL: dict[str, _PooledBridge] = {}
_POOL_GUARD = asyncio.Lock()
# Serialize cold `Client.launch_bridge` across all pools/personas/sessions.
_BRIDGE_LAUNCH_SEM = asyncio.Semaphore(1)


def _api_key() -> str:
    return os.environ.get("CURSOR_API_KEY", "").strip()


def _is_stale_agent_error(err: Exception) -> bool:
    msg = str(getattr(err, "message", err)).lower()
    return "not found" in msg and "agent" in msg


def _is_retryable_bridge_error(err: Exception) -> bool:
    if getattr(err, "is_retryable", False):
        return True
    msg = str(getattr(err, "message", err)).lower()
    needles = (
        "bridge request failed",
        "bridge request timed out",
        "timed out waiting for bridge discovery",
        "bridge discovery",
        "connection refused",
        "errno 111",
        "broken pipe",
        "connection reset",
    )
    return any(needle in msg for needle in needles)


def _bridge_pool_key(cwd: str, session_scope: str = "") -> str:
    persona = hashlib.sha256(os.path.realpath(cwd).encode("utf-8")).hexdigest()[:16]
    scope = session_scope.strip()
    if not scope:
        session = "main"
    else:
        session = hashlib.sha256(scope.encode("utf-8")).hexdigest()[:16]
    return f"{persona}:{session}"


def _bridge_state_root(cwd: str, session_scope: str = "") -> str:
    pool_key = _bridge_pool_key(cwd, session_scope).replace(":", "-")
    root = os.environ.get("CURSOR_SDK_STATE_ROOT", "").strip()
    if not root:
        runtime = os.environ.get("FINALLY_A_VALUE_BOT_RUNTIME_DATA", "").strip()
        if not runtime:
            runtime = os.path.join(
                os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                "workspace",
                "runtime",
            )
        root = os.path.join(runtime, "cursor-sdk-state")
    return os.path.join(root, pool_key)


def _close_agent_only(agent: Any | None) -> None:
    if agent is not None:
        try:
            agent.close()
        except Exception:
            pass


def _close_pooled_bridge(entry: _PooledBridge) -> None:
    try:
        entry.client.close()
    except Exception:
        pass


def _bridge_subprocess_exited(client: Any) -> bool:
    bridge = getattr(client, "_owned_bridge", None)
    if bridge is None:
        return False
    proc = getattr(bridge, "process", None)
    return proc is not None and proc.poll() is not None


def _bridge_is_alive(client: Any) -> bool:
    if _bridge_subprocess_exited(client):
        return False
    try:
        client.ping()
        return True
    except Exception:
        return False


async def _evict_pooled_bridge(pool_key: str) -> None:
    async with _POOL_GUARD:
        entry = _POOL.pop(pool_key, None)
    if entry is not None:
        _close_pooled_bridge(entry)


def _launch_bridge_client(cwd: str, state_root: str, timeout_secs: int) -> Any:
    from cursor_sdk import Client, LocalAgentOptions

    local = LocalAgentOptions(cwd=cwd)
    return Client.launch_bridge(
        workspace=cwd,
        state_root=state_root,
        local=local,
        timeout=timeout_secs,
    )


async def _get_pooled_bridge(cwd: str, session_scope: str = "") -> _PooledBridge:
    pool_key = _bridge_pool_key(cwd, session_scope)
    state_root = _bridge_state_root(cwd, session_scope)
    os.makedirs(state_root, exist_ok=True)

    async with _POOL_GUARD:
        entry = _POOL.get(pool_key)
        if entry is not None and _bridge_is_alive(entry.client):
            return entry

    async with _BRIDGE_LAUNCH_SEM:
        async with _POOL_GUARD:
            entry = _POOL.get(pool_key)
            if entry is not None and _bridge_is_alive(entry.client):
                return entry
            if entry is not None:
                _close_pooled_bridge(entry)
                _POOL.pop(pool_key, None)

            timeout_secs = _bridge_launch_timeout_secs()
            client = await asyncio.to_thread(
                _launch_bridge_client,
                cwd,
                state_root,
                timeout_secs,
            )
            entry = _PooledBridge(
                client=client,
                cwd=cwd,
                session_scope=session_scope,
                pool_key=pool_key,
                state_root=state_root,
            )
            _POOL[pool_key] = entry
            scope_label = session_scope.strip() or "main"
            print(
                "[cursor-sdk-runner] launched bridge for "
                f"persona={cwd} session={scope_label} "
                f"(pool_key={pool_key}, timeout={timeout_secs}s)",
                file=sys.stderr,
            )
            return entry


async def _close_all_pooled_bridges() -> None:
    async with _POOL_GUARD:
        entries = list(_POOL.values())
        _POOL.clear()
    for entry in entries:
        _close_pooled_bridge(entry)
    try:
        from cursor_sdk._client import close_default_client

        close_default_client()
    except Exception:
        pass


def _release_cursor_bridge(client: Any | None = None, agent: Any | None = None) -> None:
    """Legacy helper: close a one-off client/agent and any default singleton."""
    _close_agent_only(agent)
    if client is not None:
        try:
            client.close()
        except Exception:
            pass
    try:
        from cursor_sdk._client import close_default_client

        close_default_client()
    except Exception:
        pass


def _serialize_model_param_values(raw_params: Any) -> list[dict[str, str]]:
    out: list[dict[str, str]] = []
    for item in raw_params or []:
        if isinstance(item, dict):
            param_id = str(item.get("id") or "").strip()
            value = str(item.get("value") or "").strip()
        else:
            param_id = str(getattr(item, "id", "") or "").strip()
            value = str(getattr(item, "value", "") or "").strip()
        if param_id and value:
            out.append({"id": param_id, "value": value})
    return out


def _build_model_selection(model: str, model_params: list[dict[str, str]] | None):
    params = _serialize_model_param_values(model_params)
    if not params:
        return model
    try:
        from cursor_sdk import ModelParameterValue, ModelSelection
    except ImportError:
        return model
    return ModelSelection(
        id=model,
        params=[ModelParameterValue(id=p["id"], value=p["value"]) for p in params],
    )


def _serialize_model_entry(model: Any) -> dict[str, Any] | None:
    model_id = getattr(model, "id", None) or getattr(model, "model", None)
    if not model_id:
        return None
    entry: dict[str, Any] = {"id": str(model_id)}
    display_name = getattr(model, "display_name", None) or getattr(model, "displayName", None)
    if display_name:
        entry["display_name"] = str(display_name)

    parameters: list[dict[str, Any]] = []
    for param in getattr(model, "parameters", None) or []:
        param_id = getattr(param, "id", None)
        if not param_id:
            continue
        values: list[dict[str, str]] = []
        for value in getattr(param, "values", None) or []:
            raw_value = getattr(value, "value", None)
            if raw_value is None:
                continue
            value_entry: dict[str, str] = {"value": str(raw_value)}
            value_label = getattr(value, "display_name", None) or getattr(
                value, "displayName", None
            )
            if value_label:
                value_entry["display_name"] = str(value_label)
            values.append(value_entry)
        if not values:
            continue
        param_entry: dict[str, Any] = {"id": str(param_id), "values": values}
        param_label = getattr(param, "display_name", None) or getattr(param, "displayName", None)
        if param_label:
            param_entry["display_name"] = str(param_label)
        parameters.append(param_entry)
    if parameters:
        entry["parameters"] = parameters

    variants: list[dict[str, Any]] = []
    for variant in getattr(model, "variants", None) or []:
        variant_params = _serialize_model_param_values(getattr(variant, "params", None))
        variant_name = getattr(variant, "display_name", None) or getattr(
            variant, "displayName", None
        )
        if not variant_name:
            continue
        variant_entry: dict[str, Any] = {
            "params": variant_params,
            "display_name": str(variant_name),
            "is_default": bool(getattr(variant, "is_default", False) or getattr(variant, "isDefault", False)),
        }
        description = getattr(variant, "description", None)
        if description:
            variant_entry["description"] = str(description)
        variants.append(variant_entry)
    if variants:
        entry["variants"] = variants
    return entry


def _open_agent(
    client: Any,
    *,
    agent_id: str | None,
    model: str,
    model_params: list[dict[str, str]] | None,
    cwd: str,
    api_key: str,
    opts: Any,
    mcp_servers: dict[str, Any] | None,
):
    from cursor_sdk import AgentOptions, LocalAgentOptions

    model_selection = _build_model_selection(model, model_params)
    agent_opts = AgentOptions(
        api_key=api_key,
        model=model_selection,
        local=LocalAgentOptions(cwd=cwd),
        mcp_servers=mcp_servers or None,
    )
    if not agent_id:
        return client.create_agent(agent_opts)

    try:
        return client.resume_agent(agent_id, opts)
    except Exception as err:
        from cursor_sdk import CursorAgentError

        if isinstance(err, CursorAgentError) and _is_stale_agent_error(err):
            return client.create_agent(agent_opts)
        raise


async def _stream_run(body: dict[str, Any]) -> AsyncIterator[str]:
    prompt = (body.get("prompt") or "").strip()
    cwd = (body.get("cwd") or ".").strip() or "."
    model = (body.get("model") or DEFAULT_MODEL).strip() or DEFAULT_MODEL
    model_params = body.get("model_params")
    agent_id = (body.get("agent_id") or "").strip() or None
    session_scope = (body.get("session_scope") or "").strip()
    mcp_servers = body.get("mcp_servers")
    if not isinstance(mcp_servers, dict):
        mcp_servers = None

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
    pool_key = _bridge_pool_key(cwd, session_scope)

    for attempt in range(BRIDGE_RETRY_MAX_ATTEMPTS):
        agent = None
        pooled: _PooledBridge | None = None
        try:
            pooled = await _get_pooled_bridge(cwd, session_scope)
            async with pooled.lock:
                agent = _open_agent(
                    pooled.client,
                    agent_id=resume_id,
                    model=model,
                    model_params=model_params if isinstance(model_params, list) else None,
                    cwd=cwd,
                    api_key=api_key,
                    opts=opts,
                    mcp_servers=mcp_servers,
                )
                for line in _stream_agent_turn(agent, prompt, resume_id, mcp_servers):
                    yield line
                return
        except CursorAgentError as err:
            if attempt == 0 and resume_id and _is_stale_agent_error(err):
                resume_id = None
                continue
            if (
                attempt + 1 < BRIDGE_RETRY_MAX_ATTEMPTS
                and _is_retryable_bridge_error(err)
            ):
                print(
                    "[cursor-sdk-runner] retryable bridge error "
                    f"(attempt {attempt + 1}/{BRIDGE_RETRY_MAX_ATTEMPTS}): {err}",
                    file=sys.stderr,
                )
                await _evict_pooled_bridge(pool_key)
                await asyncio.sleep(BRIDGE_RETRY_BACKOFF_SECS[attempt])
                continue
            yield json.dumps(
                {
                    "type": "error",
                    "message": (
                        "Cursor SDK startup failed: "
                        f"{getattr(err, 'message', err)}"
                    ),
                }
            ) + "\n"
            return
        except Exception as err:  # pragma: no cover - surfaced to bot logs
            if (
                attempt + 1 < BRIDGE_RETRY_MAX_ATTEMPTS
                and _is_retryable_bridge_error(err)
            ):
                print(
                    "[cursor-sdk-runner] retryable bridge error "
                    f"(attempt {attempt + 1}/{BRIDGE_RETRY_MAX_ATTEMPTS}): {err}",
                    file=sys.stderr,
                )
                await _evict_pooled_bridge(pool_key)
                await asyncio.sleep(BRIDGE_RETRY_BACKOFF_SECS[attempt])
                continue
            yield json.dumps({"type": "error", "message": str(err)}) + "\n"
            return
        finally:
            _close_agent_only(agent)


def _stream_agent_turn(
    agent: Any,
    prompt: str,
    resume_id: str | None,
    mcp_servers: dict[str, Any] | None,
):
    final_text_parts: list[str] = []
    status = "error"
    send_options: dict[str, Any] | None = None
    if mcp_servers:
        send_options = {"mcp_servers": mcp_servers}

    returned_agent_id: str | None = getattr(agent, "agent_id", None) or resume_id
    run = agent.send(prompt, send_options) if send_options else agent.send(prompt)
    for message in run.messages():
        msg_type = getattr(message, "type", None)
        if msg_type != "assistant":
            continue
        payload = getattr(message, "message", None)
        if payload is None:
            continue
        for block in getattr(payload, "content", []) or []:
            block_type = getattr(block, "type", None)
            if block_type == "text":
                text = getattr(block, "text", "") or ""
                if not text:
                    continue
                final_text_parts.append(text)
                yield json.dumps({"type": "text", "text": text}) + "\n"
            elif block_type == "tool_use":
                name = getattr(block, "name", "") or ""
                tool_input = getattr(block, "input", None)
                if not isinstance(tool_input, dict):
                    tool_input = {}
                yield json.dumps(
                    {
                        "type": "tool_use",
                        "name": name,
                        "input": tool_input,
                    }
                ) + "\n"
            elif block_type == "tool_result":
                yield json.dumps(
                    {
                        "type": "tool_result",
                        "name": getattr(block, "name", "") or "",
                        "output": getattr(block, "content", "") or "",
                        "is_error": bool(getattr(block, "is_error", False)),
                    }
                ) + "\n"
            elif block_type == "thinking":
                thinking = getattr(block, "thinking", "") or getattr(block, "text", "") or ""
                if thinking:
                    yield json.dumps({"type": "thinking", "thinking": thinking}) + "\n"

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
            "mcp_supported": True,
            "persona_bridges_active": len(_POOL),
            "session_scoped_bridges": True,
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
            entry = _serialize_model_entry(m)
            if entry:
                payload.append(entry)
        return web.json_response({"ok": True, "models": payload})
    except Exception as err:  # pragma: no cover
        return web.json_response({"ok": False, "error": str(err)}, status=502)


async def _on_cleanup(_app: web.Application) -> None:
    await _close_all_pooled_bridges()


def _run_self_tests() -> None:
    assert _is_retryable_bridge_error(
        RuntimeError("Timed out waiting for bridge discovery")
    )
    assert _is_retryable_bridge_error(RuntimeError("bridge discovery failed"))
    assert not _is_retryable_bridge_error(RuntimeError("prompt required"))
    assert _bridge_launch_timeout_secs() >= 30
    os.environ["CURSOR_BRIDGE_LAUNCH_TIMEOUT_SECS"] = "45"
    assert _bridge_launch_timeout_secs() == 45
    os.environ.pop("CURSOR_BRIDGE_LAUNCH_TIMEOUT_SECS", None)


def main() -> None:
    port = int(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_PORT
    app = web.Application()
    app.on_cleanup.append(_on_cleanup)
    app.router.add_get("/health", handle_health)
    app.router.add_get("/models", handle_models)
    app.router.add_post("/run", handle_run)
    print(f"Cursor SDK runner listening on 0.0.0.0:{port}", file=sys.stderr)
    web.run_app(app, host="0.0.0.0", port=port, print=None)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        _run_self_tests()
        print("cursor-sdk-runner self-tests ok", file=sys.stderr)
    else:
        main()
