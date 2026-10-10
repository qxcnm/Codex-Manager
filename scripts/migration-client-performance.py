#!/usr/bin/env python3
"""Isolated real-binary acceptance and comparable local-fixture load measurements.

Dependencies: python -m pip install -r scripts/migration-probe-requirements.txt
Creates a new temporary database and copies supplied binaries beside it. It never
uses an existing database or inherited CODEXMANAGER_* / database credentials.
Results are local-fixture observations, not production performance guarantees.
"""
import argparse
import asyncio
import hashlib
import json
import os
import platform
import shutil
import socket
import sqlite3
import statistics
import subprocess
import tempfile
import time
from collections import Counter
from pathlib import Path

import aiohttp
from aiohttp import web
import psutil

KEY = "fixture-migration-platform-key"
RPC_TOKEN = "fixture-migration-rpc-token"
MODEL = "gpt-6-luna"


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def child_environment(folder, port, provider_port):
    env = {k: v for k, v in os.environ.items()
           if not k.upper().startswith("CODEXMANAGER_") and k.upper() != "DATABASE_URL"}
    env.update({
        "CODEXMANAGER_DB_PATH": str(folder / "fixture.sqlite"),
        "CODEXMANAGER_RPC_TOKEN": RPC_TOKEN,
        "CODEXMANAGER_RPC_TOKEN_FILE": str(folder / "rpc-token"),
        "CODEXMANAGER_SERVICE_ADDR": f"127.0.0.1:{port}",
        # Keep the deterministic provider loopback-only while matching the
        # official ChatGPT backend path used by the gateway's WebSocket guard.
        # The fixture server accepts every path, so this does not contact an
        # external host or imply production provider acceptance.
        "CODEXMANAGER_UPSTREAM_BASE_URL": f"http://127.0.0.1:{provider_port}/chatgpt.com/backend-api/codex",
        "CODEXMANAGER_UPSTREAM_PROXY_URL": "",
        "CODEXMANAGER_PROXY_LIST": "",
        "CODEXMANAGER_DISABLE_POLLING": "1",
        "CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED": "false",
        "CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED": "false",
        "CODEXMANAGER_WARMUP_CRON_ENABLED": "false",
        "CODEXMANAGER_USAGE_POLLING_ENABLED": "false",
        "CODEXMANAGER_WEB_NO_OPEN": "1",
        "CODEXMANAGER_WEB_NO_SPAWN_SERVICE": "1",
        "NO_PROXY": "*",
        "RUST_LOG": "warn",
    })
    return env


def launch(binary, folder, env, name):
    copied = folder / Path(binary).name
    if not copied.exists():
        shutil.copy2(binary, copied)
    log = (folder / f"{name}.log").open("ab")
    process = subprocess.Popen([str(copied)], cwd=folder, env=env,
                               stdout=log, stderr=log,
                               creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    log.close()
    return process


async def wait_ready(session, base, process):
    until = time.monotonic() + 40
    while time.monotonic() < until:
        if process.poll() is not None:
            raise AssertionError(f"process exited before readiness: {process.returncode}")
        try:
            async with session.get(base + "/health", timeout=aiohttp.ClientTimeout(total=1)) as response:
                if response.status == 200 and await response.text() == "ok":
                    return
        except (aiohttp.ClientError, asyncio.TimeoutError):
            pass
        await asyncio.sleep(.05)
    raise AssertionError("listener readiness deadline exceeded")


async def stop(session, process, base, endpoint="/__shutdown"):
    if process is None:
        return {"started": False}
    result = {"started": True, "forced_termination": False}
    if process.poll() is not None:
        return {**result, "already_exited": True, "exit_code": process.returncode}
    try:
        headers = {"X-CodexManager-Rpc-Token": RPC_TOKEN}
        if endpoint == "/__quit":
            # Web control routes require the fixture admin's session. Do not
            # follow an anonymous redirect and mistake its login HTML for quit.
            async with session.post(base + "/__login", data={"username": "fixture-admin", "password": "fixture-admin-password"}) as login:
                if "Set-Cookie" in login.headers:
                    headers["Cookie"] = login.headers["Set-Cookie"].split(";", 1)[0]
        async with session.get(base + endpoint, headers=headers, allow_redirects=False,
                               timeout=aiohttp.ClientTimeout(total=3)) as response:
            await response.read()
            result["shutdown_http_status"] = response.status
    except (aiohttp.ClientError, asyncio.TimeoutError) as error:
        result["shutdown_error"] = type(error).__name__
    for _ in range(200):
        if process.poll() is not None:
            return {**result, "exit_code": process.returncode}
        await asyncio.sleep(.05)
    result["forced_termination"] = True
    process.terminate()
    await asyncio.to_thread(process.wait, 10)
    return {**result, "exit_code": process.returncode}


def load_key(index):
    return f"{KEY}-load-{index}"


def seed_database(path, accounts_mode, independent_key_count=0):
    now = int(time.time())
    profile = path.parent / "fixture-codex-profile"
    profile.mkdir()
    with sqlite3.connect(path) as db:
        db.execute("INSERT OR REPLACE INTO app_settings(key,value,updated_at) VALUES(?,?,?)",
                   ("codex_profile.codex_home", str(profile), now))
        db.execute("INSERT INTO accounts(id,label,issuer,workspace_id,sort,status,created_at,updated_at) "
                   "VALUES(?,?,?,?,?,?,?,?)", ("fixture-account", "Migration local fixture", "https://auth.openai.com",
                                              "fixture-workspace", 0, "force_enabled", now, now))
        db.execute("INSERT INTO tokens(account_id,id_token,access_token,refresh_token,last_refresh) VALUES(?,?,?,?,?)",
                   ("fixture-account", "", "fixture-upstream-access", "", now))
        db.execute("INSERT INTO api_keys(id,name,key_hash,status,created_at) VALUES(?,?,?,?,?)",
                   ("fixture-key", "Migration fixture", hashlib.sha256(KEY.encode()).hexdigest(), "active", now))
        db.execute("INSERT INTO api_key_profiles(key_id,client_type,protocol_type,auth_scheme,created_at,updated_at) "
                   "VALUES(?,?,?,?,?,?)", ("fixture-key", "codex", "openai_compat", "authorization_bearer", now, now))
        for index in range(independent_key_count):
            key_id = f"fixture-load-{index}"
            db.execute("INSERT INTO api_keys(id,name,key_hash,status,created_at) VALUES(?,?,?,?,?)",
                       (key_id, "Independent load fixture", hashlib.sha256(load_key(index).encode()).hexdigest(), "active", now))
            db.execute("INSERT INTO api_key_profiles(key_id,client_type,protocol_type,auth_scheme,created_at,updated_at) "
                       "VALUES(?,?,?,?,?,?)", (key_id, "codex", "openai_compat", "authorization_bearer", now, now))
        if accounts_mode:
            db.execute("INSERT OR REPLACE INTO app_settings(key,value,updated_at) VALUES(?,?,?)",
                       ("web.auth.mode", "accounts", now))


def completed_event(response_id="fixture-response"):
    return {"type": "response.completed", "response": {
        "id": response_id, "object": "response", "status": "completed", "model": MODEL,
        "output": [{"id": "fixture-message", "type": "message", "role": "assistant", "status": "completed",
                    "content": [{"type": "output_text", "text": "fixture hello", "annotations": []}]}],
        "usage": {"input_tokens": 2, "output_tokens": 1, "total_tokens": 3}}}


async def provider(request):
    if request.headers.get("Upgrade", "").lower() == "websocket":
        ws = web.WebSocketResponse()
        await ws.prepare(request)
        async for message in ws:
            if message.type == aiohttp.WSMsgType.TEXT:
                incoming = json.loads(message.data)
                if incoming.get("type") == "response.create":
                    await ws.send_json({"type": "response.output_text.delta", "delta": "fixture hello"})
                    await ws.send_json(completed_event())
        return ws
    incoming = await request.json()
    long_stream = "long-fixture" in json.dumps(incoming)
    response = web.StreamResponse(headers={"Content-Type": "text/event-stream", "Cache-Control": "no-cache"})
    await response.prepare(request)
    try:
        await response.write(b'data: {"type":"response.output_text.delta","delta":"fixture hello"}\n\n')
        if long_stream:
            for _ in range(15):
                await asyncio.sleep(.1)
                await response.write(b": fixture heartbeat\n\n")
        await response.write(("data: " + json.dumps(completed_event()) + "\n\ndata: [DONE]\n\n").encode())
        await response.write_eof()
    except (ConnectionResetError, asyncio.CancelledError):
        pass
    return response


async def rpc(session, base, method, params=None, cookie=None, direct=False):
    headers = {"Content-Type": "application/json"}
    if direct:
        headers["X-CodexManager-Rpc-Token"] = RPC_TOKEN
    if cookie:
        headers["Cookie"] = cookie
    async with session.post(base + ("/rpc" if direct else "/api/rpc"), headers=headers,
                            json={"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}) as response:
        # The Axum RPC compatibility response is JSON-shaped but intentionally
        # keeps the legacy text/plain content type. Parse the body explicitly
        # so this probe validates the payload rather than an incidental MIME
        # header difference between the old and new transports.
        return response.status, json.loads(await response.text())


async def protocol_checks(session, base):
    result = []
    for path in ("/v1/responses", "/v1/chat/completions"):
        for stream in (False, True):
            payload = {"model": MODEL, "stream": stream}
            payload.update({"messages": [{"role": "user", "content": "hello"}]} if "chat" in path else {"input": "hello"})
            async with session.post(base + path, headers={"Authorization": "Bearer " + KEY}, json=payload) as response:
                body = await response.text()
                assert response.status == 200, f"{path}/{stream}: status={response.status} body={body[:200]}"
                assert "fixture hello" in body, f"missing fixture payload: {path}/{stream}"
                if stream:
                    assert "text/event-stream" in response.headers.get("Content-Type", "")
                    assert ("finish_reason" if "chat" in path else "response.completed") in body
                else:
                    assert "usage" in json.loads(body)
                result.append({"path": path, "stream": stream, "status": response.status, "payload_verified": True})
    websocket_base = base.replace("http://", "ws://", 1).replace("https://", "wss://", 1)
    async with session.ws_connect(websocket_base + "/v1/responses", headers={"Authorization": "Bearer " + KEY}, timeout=10) as ws:
        await ws.send_json({"type": "response.create", "model": MODEL, "input": "hello"})
        received = []
        while True:
            message = await asyncio.wait_for(ws.receive(), 10)
            assert message.type == aiohttp.WSMsgType.TEXT, f"unexpected websocket event: {message.type}"
            event = json.loads(message.data)
            received.append(event.get("type"))
            if event.get("type") == "response.completed":
                assert "fixture hello" in message.data
                break
        await ws.close(code=1000)
        result.append({"path": "/v1/responses", "transport": "websocket", "events": received, "close_code": ws.close_code})
    return result


async def web_checks(session, base, database):
    checks = []
    status, _ = await rpc(session, base, "accountManager/status")
    assert status == 401, f"unauthenticated RPC status={status}"
    checks.append("unauthenticated_rpc_401")
    async with session.get(base + "/", allow_redirects=False) as response:
        assert response.status == 303 and response.headers.get("Location") == "/__login", \
            f"unauthenticated UI redirect status={response.status} location={response.headers.get('Location')}"
    checks.append("unauthenticated_ui_login_redirect")
    async with session.post(base + "/__login", data={"username": "fixture-admin", "password": "fixture-admin-password"}) as response:
        assert response.status == 200, f"admin login status={response.status}"
        raw = response.headers["Set-Cookie"]
        assert "HttpOnly" in raw and "SameSite=Lax" in raw and "Path=/" in raw, f"admin cookie attributes={raw}"
        admin = raw.split(";", 1)[0]
        login_body = await response.text()
        assert "sessionStorage.setItem" in login_body, "admin login bootstrap missing sessionStorage"
    checks.append("http_bootstrap_cookie_and_tab_session")
    async with session.get(base + "/", headers={"Cookie": admin}) as response:
        body = await response.text()
        assert response.status == 200 and "_next/" in body, f"authenticated UI status={response.status}"
    checks.append("authenticated_exported_ui")
    async with session.get(base + "/__auth_status", headers={"Cookie": admin}) as response:
        auth_status = await response.json()
        assert auth_status.get("role") == "admin", f"admin auth status={auth_status}"
    checks.append("cookie_resolves_admin_session")
    status, created = await rpc(session, base, "accountManager/users/create",
                                {"username": "fixture-member", "password": "fixture-member-password", "role": "member"}, admin)
    assert status == 200 and "id" in created.get("result", {}), f"member create status={status} payload={created}"
    member_id = created["result"]["id"]
    async with session.post(base + "/__login", data={"username": "fixture-member", "password": "wrong-password"}) as response:
        assert response.status == 401 and "Set-Cookie" not in response.headers, f"invalid login status={response.status}"
    checks.append("invalid_password_401")
    async with session.post(base + "/__login", data={"username": "fixture-member", "password": "fixture-member-password"}) as response:
        assert response.status == 200, f"member login status={response.status}"
        member = response.headers["Set-Cookie"].split(";", 1)[0]
    status, denied = await rpc(session, base, "accountManager/users/list", cookie=member)
    assert status == 200 and denied.get("result", {}).get("errorCode") == "permission_denied", \
        f"member privilege check status={status} payload={denied}"
    checks.append("member_privileged_rpc_denied")
    status, allowed = await rpc(session, base, "accountManager/profile/update", {"displayName": "Fixture verified member"}, member)
    assert status == 200 and allowed.get("result", {}).get("displayName") == "Fixture verified member", \
        f"member profile update status={status} payload={allowed}"
    checks.append("member_scoped_rpc_success")
    for method in ("apikey/disable", "apikey/enable", "apikey/delete"):
        status, denied = await rpc(session, base, method, {"id": "fixture-key"}, member)
        assert status == 200 and denied.get("result", {}).get("errorCode") == "permission_denied", \
            f"member foreign-key mutation was not rejected: {method}"
    status, created_key = await rpc(session, base, "apikey/create", {"name": "member mutation fixture"}, member)
    assert status == 200 and "id" in created_key.get("result", {}), "member fixture key creation failed"
    member_key_id = created_key["result"]["id"]
    for method, expected in (("apikey/disable", "disabled"), ("apikey/enable", "active")):
        status, mutated = await rpc(session, base, method, {"id": member_key_id}, member)
        assert status == 200 and mutated.get("result", {}).get("ok") is True, f"member {method} failed"
        with sqlite3.connect(database) as db:
            assert db.execute("SELECT status FROM api_keys WHERE id=?", (member_key_id,)).fetchone()[0] == expected
            assert db.execute("SELECT status FROM api_keys WHERE id='fixture-key'").fetchone()[0] == "active"
    status, deleted = await rpc(session, base, "apikey/delete", {"id": member_key_id}, member)
    assert status == 200 and deleted.get("result", {}).get("ok") is True, "member key delete failed"
    with sqlite3.connect(database) as db:
        assert db.execute("SELECT COUNT(*) FROM api_keys WHERE id=?", (member_key_id,)).fetchone()[0] == 0
        assert db.execute("SELECT COUNT(*) FROM api_key_owners WHERE key_id=?", (member_key_id,)).fetchone()[0] == 0
    checks.append("member_key_mutations_ownership_and_sqlite_readback")
    async with session.post(base + "/__logout", headers={"Cookie": member}) as response:
        assert response.status == 200 and "Max-Age=0" in response.headers.get("Set-Cookie", ""), \
            f"logout status={response.status}"
    assert (await rpc(session, base, "accountManager/status", cookie=member))[0] == 401, "logged-out member cookie remained valid"
    checks.append("logout_cookie_clear_and_old_cookie_revocation")
    async with session.post(base + "/__login", data={"username": "fixture-member", "password": "fixture-member-password"}) as response:
        member = response.headers["Set-Cookie"].split(";", 1)[0]
    status, updated = await rpc(session, base, "accountManager/users/update", {
        "id": member_id, "status": "disabled", "displayName": "Fixture verified member"
    }, admin)
    assert status == 200 and updated.get("result", {}).get("status") == "disabled", \
        f"member disable status={status} payload={updated}"
    assert (await rpc(session, base, "accountManager/status", cookie=member))[0] == 401, "disabled member cookie remained valid"
    checks.append("disabled_user_session_revocation")
    with sqlite3.connect(database) as db:
        revoked = db.execute("SELECT COUNT(*) FROM app_user_sessions WHERE revoked_at IS NOT NULL").fetchone()[0]
        assert revoked >= 1, f"revoked session rows={revoked}"
        display_name = db.execute("SELECT display_name FROM app_users WHERE id=?", (member_id,)).fetchone()[0]
        assert display_name == "Fixture verified member", f"profile readback={display_name!r}"
    checks.append("sqlite_session_and_profile_readback")
    return {"checks": checks, "gateway": await protocol_checks(session, base)}


def summarize(samples, elapsed):
    durations = sorted(item[0] for item in samples)
    failed = sum(not item[1] for item in samples)
    def percentile(p):
        return round(durations[min(len(durations) - 1, int((len(durations) - 1) * p))], 3) if durations else None
    return {"requests": len(samples), "elapsed_seconds": round(elapsed, 3),
            "throughput_rps": round(len(samples) / elapsed, 3),
            "p50_ms": percentile(.50), "p95_ms": percentile(.95), "p99_ms": percentile(.99),
            "failures": failed, "failure_rate": round(failed / len(samples), 6) if samples else None,
            "status_counts": dict(Counter(str(item[2]) for item in samples))}


async def measure(session, base, process, duration, concurrency, mixed, independent_keys=False):
    start = time.monotonic()
    deadline = start + duration
    samples, long_samples, health_samples, resources = [], [], [], []
    process_info = psutil.Process(process.pid)
    async def request_once(kind, destination, key=KEY):
        begin = time.perf_counter()
        try:
            if kind == "health":
                request = session.get(base + "/health")
            else:
                request = session.post(base + "/v1/responses", headers={"Authorization": "Bearer " + key},
                                       json={"model": MODEL, "input": "long-fixture" if kind == "long" else "hello", "stream": kind == "long"})
            async with request as response:
                body = await response.read()
                valid = response.status == 200 and (b"ok" == body if kind == "health" else b"fixture hello" in body)
                destination.append(((time.perf_counter() - begin) * 1000, valid, response.status))
        except (aiohttp.ClientError, asyncio.TimeoutError) as error:
            destination.append(((time.perf_counter() - begin) * 1000, False, type(error).__name__))
    async def worker(kind, destination, index):
        while time.monotonic() < deadline:
            await request_once(kind, destination, load_key(index) if independent_keys else KEY)
    async def sample_resources():
        while time.monotonic() < deadline:
            def read():
                with process_info.oneshot():
                    memory = process_info.memory_info()
                    return {"rss_bytes": memory.rss, "private_bytes": getattr(memory, "private", memory.vms),
                            "threads": process_info.num_threads(), "tcp_connections": len(process_info.net_connections(kind="tcp"))}
            resources.append(await asyncio.to_thread(read))
            await asyncio.sleep(.2)
    async def health_worker():
        while time.monotonic() < deadline:
            await request_once("health", health_samples)
            await asyncio.sleep(.05)
    jobs = [worker("short", samples, index) for index in range(concurrency)] + [sample_resources(), health_worker()]
    if mixed:
        jobs += [worker("long", long_samples, concurrency + index) for index in range(4)]
    await asyncio.gather(*jobs)
    elapsed = time.monotonic() - start
    return {"model": {"short_concurrency": concurrency, "long_concurrency": 4 if mixed else 0,
                       "provider_long_stream_seconds": 1.5, "duration_seconds": duration,
                       "request_gate_scope": "one key per worker" if independent_keys else "shared key and model",
                       "request": "Responses JSON, fixed 2 input / 1 output tokens, loopback SSE fixture",
                       "health_probe_interval_ms": 50},
            "short": summarize(samples, elapsed), "long": summarize(long_samples, elapsed),
            "health": summarize(health_samples, elapsed),
            "resources": {"samples": len(resources), "interval_ms": 200,
                          "peak": {key: max(row[key] for row in resources) for key in resources[0]},
                          "rss_mean_bytes": round(statistics.mean(row["rss_bytes"] for row in resources))}}


async def run(args):
    folder = Path(tempfile.mkdtemp(prefix="codexmanager-client-acceptance-"))
    port, provider_port, web_port = free_port(), free_port(), free_port()
    base, web_base = f"http://127.0.0.1:{port}", f"http://127.0.0.1:{web_port}"
    env = child_environment(folder, port, provider_port)
    provider_app = web.Application()
    provider_app.router.add_route("*", "/{path:.*}", provider)
    runner = web.AppRunner(provider_app)
    await runner.setup()
    await web.TCPSite(runner, "127.0.0.1", provider_port).start()
    report = {"label": args.label, "scope": "isolated local SQLite, deterministic loopback provider; not production",
              "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "fixture_directory": str(folder),
              "service_binary_sha256": sha256(args.service_bin), "source_revision": args.source_revision,
              "probe_script_sha256": sha256(__file__), "build_profile": args.build_profile,
              "hardware": {"os": platform.platform(), "processor": platform.processor(),
                           "physical_cores": psutil.cpu_count(logical=False), "logical_cpus": psutil.cpu_count(),
                           "memory_total_bytes": psutil.virtual_memory().total}, "result": "FAIL"}
    process = web_process = None
    async with aiohttp.ClientSession(cookie_jar=aiohttp.DummyCookieJar(), trust_env=False,
                                    timeout=aiohttp.ClientTimeout(total=30), connector=aiohttp.TCPConnector(limit=256)) as session:
        try:
            process = launch(args.service_bin, folder, env, "service-init")
            await wait_ready(session, base, process)
            report["initialization_shutdown"] = await stop(session, process, base)
            seed_database(folder / "fixture.sqlite", bool(args.web_bin),
                          args.concurrency + 4 if args.independent_keys else 0)
            process = launch(args.service_bin, folder, env, "service")
            await wait_ready(session, base, process)
            report["service_protocol"] = await protocol_checks(session, base)
            for _ in range(10):
                await rpc(session, base, "accountManager/status", direct=True)
            if args.web_bin:
                env.update({"CODEXMANAGER_WEB_ADDR": f"127.0.0.1:{web_port}", "CODEXMANAGER_WEB_ROOT": str(Path(args.web_root).resolve())})
                web_process = launch(args.web_bin, folder, env, "web")
                await wait_ready(session, web_base, web_process)
                report["web_binary_sha256"] = sha256(args.web_bin)
                report["web_acceptance"] = await web_checks(session, web_base, folder / "fixture.sqlite")
                if args.browser:
                    browser_script = Path(__file__).with_name("migration-browser-acceptance.cjs")
                    report["browser_probe_script_sha256"] = sha256(browser_script)
                    browser = await asyncio.create_subprocess_exec(
                        "node", str(browser_script), web_base, str(folder),
                        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
                    _, browser_errors = await browser.communicate()
                    browser_report = folder / "browser-report.json"
                    report["browser_acceptance"] = json.loads(browser_report.read_text(encoding="utf-8")) \
                        if browser_report.exists() else {"result": "FAIL", "failure": browser_errors.decode(errors="replace")}
                    assert browser.returncode == 0, "browser acceptance failed; see browser_acceptance report"
            if args.duration:
                report["short_only"] = await measure(session, base, process, args.duration, args.concurrency, False)
                report["mixed_short_long"] = await measure(session, base, process, args.duration, args.concurrency, True)
                scenarios = ["short_only", "mixed_short_long"]
                if args.independent_keys:
                    report["mixed_independent_keys"] = await measure(session, base, process, args.duration, args.concurrency, True, True)
                    scenarios.append("mixed_independent_keys")
                assert all(report[scenario][kind]["failures"] == 0 for scenario in scenarios
                           for kind in ("short", "long", "health")), "nonzero load-test failure rate"
            # Retain load measurements even when an older baseline has an abnormal
            # close, but never label that protocol failure as a passing acceptance.
            assert all(item.get("close_code") == 1000 for item in report["service_protocol"]
                       if item.get("transport") == "websocket"), "websocket normal close was not acknowledged with code 1000"
            if "web_acceptance" in report:
                assert all(item.get("close_code") == 1000 for item in report["web_acceptance"]["gateway"]
                           if item.get("transport") == "websocket"), "web proxy websocket close was not acknowledged with code 1000"
            report["result"] = "PASS"
        except Exception as error:
            report["failure"] = f"{type(error).__name__}: {error}"
        finally:
            report["web_shutdown"] = await stop(session, web_process, web_base, "/__quit")
            report["service_shutdown"] = await stop(session, process, base)
            shutdowns = [report.get("initialization_shutdown", {}), report["web_shutdown"], report["service_shutdown"]]
            if any(item.get("forced_termination") or item.get("exit_code", 0) != 0 for item in shutdowns):
                report["result"] = "FAIL"
                report.setdefault("failure", "fixture process did not shut down gracefully")
            await runner.cleanup()
            report["processes_exited"] = all(p is None or p.poll() is not None for p in (process, web_process))
            if (folder / "fixture.sqlite").exists():
                with sqlite3.connect(folder / "fixture.sqlite") as db:
                    report["sqlite_integrity"] = db.execute("PRAGMA integrity_check").fetchone()[0]
                    report["request_logs_readback"] = db.execute("SELECT COUNT(*) FROM request_logs").fetchone()[0]
    Path(args.output).write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"result": report["result"], "report": str(Path(args.output).resolve()),
                      "failure": report.get("failure"), "fixture_directory": str(folder)}, ensure_ascii=False))
    return 0 if report["result"] == "PASS" else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--service-bin", required=True)
    parser.add_argument("--web-bin")
    parser.add_argument("--browser", action="store_true", help="Run installed Playwright Chromium against the real isolated Web UI")
    parser.add_argument("--web-root", default="apps/out")
    parser.add_argument("--label", required=True)
    parser.add_argument("--source-revision", required=True, help="Actual checkout/build revision description, not an inferred label")
    parser.add_argument("--build-profile", default="unspecified", help="Verified build profile for baseline comparison")
    parser.add_argument("--output", required=True)
    parser.add_argument("--duration", type=int, default=15)
    parser.add_argument("--concurrency", type=int, default=8)
    parser.add_argument("--independent-keys", action="store_true", help="Also measure mixed load with a distinct fixture key per worker")
    args = parser.parse_args()
    if args.duration < 0 or args.concurrency < 1:
        parser.error("duration must be nonnegative and concurrency positive")
    if args.browser and not args.web_bin:
        parser.error("--browser requires --web-bin")
    raise SystemExit(asyncio.run(run(args)))
