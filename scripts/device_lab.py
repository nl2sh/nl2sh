#!/usr/bin/env python3
"""Host-side nl2sh experiment client; not a gateway or device execution server."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
LIMIT = 4 * 1024 * 1024
VERSION = "2025-11-25"


class LabError(Exception):
    pass


def read_json(path):
    with open(path, "rb") as file:
        raw = file.read(LIMIT + 1)
    if len(raw) > LIMIT:
        raise LabError("JSON input exceeds 4 MiB")
    return json.loads(raw)


def private_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    raw = json.dumps(value, ensure_ascii=False, indent=2).encode()
    if len(raw) > LIMIT:
        raise LabError("report exceeds 4 MiB")
    fd, temporary = tempfile.mkstemp(dir=path.parent, prefix=".device-lab-")
    try:
        with os.fdopen(fd, "wb") as file:
            file.write(raw)
            file.flush()
            os.fsync(file.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def digest(path):
    hash_ = hashlib.sha256()
    with open(path, "rb") as file:
        for chunk in iter(lambda: file.read(65536), b""):
            hash_.update(chunk)
    return hash_.hexdigest()


def source_snapshot():
    files = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=ROOT
    ).decode().split("\0")
    hash_ = hashlib.sha256()
    for name in sorted(set(files)):
        if not (name in {"Cargo.toml", "Cargo.lock", "build.rs", "cross-compile.sh"}
                or name.startswith(("src/", "crates/", "web/", ".cargo/"))):
            continue
        path = ROOT / name
        if not path.is_file():
            continue
        hash_.update(name.encode() + b"\0" + bytes.fromhex(digest(path)))
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip()
    dirty = bool(subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=normal"], cwd=ROOT
    ).strip())
    return {"source_sha256": hash_.hexdigest(), "git_commit": commit, "git_dirty": dirty}


def build(args):
    before = source_snapshot()
    build_id = uuid.uuid4().hex
    env = dict(os.environ, RUST_TARGET=args.target, NL2SH_BUILD_ID=build_id)
    subprocess.run([str(ROOT / "cross-compile.sh")], cwd=ROOT, env=env, check=True)
    if before != source_snapshot():
        raise LabError("source changed during build; manifest was not created")
    artifact = ROOT / "target" / args.target / "release" / "nl2sh"
    private_json(args.manifest, dict(before, schema_version=1, build_id=build_id,
                 build_target=args.target, build_profile="release",
                 binary_sha256=digest(artifact), artifact=str(artifact)))


def validate_manifest(manifest, artifact=None):
    if manifest.get("schema_version") != 1 or not manifest.get("build_id"):
        raise LabError("unsupported or incomplete build manifest")
    current = source_snapshot()
    for key in ("source_sha256", "git_commit"):
        if manifest.get(key) != current[key]:
            raise LabError("current source differs from build manifest")
    # Dirty state may change solely due to documentation/report additions. The
    # source digest binds build inputs; git_dirty describes the build snapshot.
    if artifact and digest(artifact) != manifest.get("binary_sha256"):
        raise LabError("artifact differs from build manifest")


def identity_gate(environment, manifest):
    identity = environment.get("build_identity", {})
    keys = ("binary_sha256", "build_id", "build_target", "build_profile", "git_commit", "git_dirty")
    mismatches = [key for key in keys if identity.get(key) is None or identity.get(key) != manifest.get(key)]
    return {"status": "fail" if mismatches else "pass", "mismatches": mismatches,
            "expected": {key: manifest.get(key) for key in keys}, "actual": identity}


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        raise LabError("redirect refused; do not forward credentials to another endpoint")


class Mcp:
    def __init__(self, url, token, host=None, timeout=210, allow_http=False):
        parsed = urllib.parse.urlsplit(url)
        if (parsed.scheme not in {"http", "https"} or not parsed.hostname
                or parsed.username or parsed.password or parsed.query or parsed.fragment
                or parsed.path != "/mcp"):
            raise LabError("expected an HTTP(S) /mcp URL without credentials or query")
        if parsed.scheme == "http" and parsed.hostname not in {"localhost", "127.0.0.1", "::1"} and not allow_http:
            raise LabError("remote plaintext requires --allow-insecure-http")
        if not token or not (32 <= len(token) <= 256) or not all(33 <= ord(c) <= 126 for c in token):
            raise LabError("missing or invalid protocol token environment/connection credential")
        self.url, self.token, self.host, self.timeout = url, token, host, timeout
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
        self.counter = 0
        initialized = self.rpc("initialize", {"protocolVersion": VERSION, "capabilities": {},
                                               "clientInfo": {"name": "nl2sh-device-lab", "version": "1"}})
        self.version = initialized.get("protocolVersion")
        if self.version not in {VERSION, "2025-06-18", "2024-11-05"}:
            raise LabError("unsupported negotiated protocol")
        self.rpc("notifications/initialized", {}, notification=True)

    def rpc(self, method, params, notification=False, a2a=False):
        self.counter += 1
        body = {"jsonrpc": "2.0", "method": method, "params": params}
        if not notification:
            body["id"] = self.counter
        headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream",
                   "Authorization": "Bearer " + self.token}
        if hasattr(self, "version"):
            headers["MCP-Protocol-Version"] = self.version
        if self.host:
            headers["Host"] = self.host
        if a2a:
            headers["A2A-Version"] = "1.0"
        endpoint = self.url[:-4] + "/a2a" if a2a else self.url
        request = urllib.request.Request(endpoint, json.dumps(body).encode(), headers)
        try:
            with self.opener.open(request, timeout=self.timeout) as response:
                raw = response.read(LIMIT + 1)
                if len(raw) > LIMIT:
                    raise LabError("MCP response exceeds 4 MiB")
                if notification:
                    return None
                if response.headers.get_content_type() != "application/json":
                    raise LabError("server must return JSON for this bounded lab client")
        except urllib.error.HTTPError as error:
            raise LabError(f"MCP HTTP {error.code}") from None
        except (urllib.error.URLError, TimeoutError, socket.timeout):
            raise LabError("MCP unreachable or timed out; uncertain operations must not be replayed") from None
        reply = json.loads(raw)
        if reply.get("id") != self.counter or "error" in reply:
            raise LabError("MCP JSON-RPC error or mismatched response ID")
        return reply["result"]

    def call(self, name, arguments=None):
        return self.rpc("tools/call", {"name": name, "arguments": arguments or {}})

    def delegate(self, arguments, deadline):
        message = {"messageId": uuid.uuid4().hex, "role": "ROLE_USER",
                   "parts": [{"text": arguments["message"]}]}
        if arguments.get("context_id"):
            message["contextId"] = arguments["context_id"]
        task = self.rpc("SendMessage", {"message": message, "configuration": {"returnImmediately": True}}, a2a=True)["task"]
        task_id = task["id"]
        # Once submitted, only query/cancel the known ID. Never resubmit on loss.
        started = time.monotonic()
        try:
            while task["status"]["state"] in {"TASK_STATE_SUBMITTED", "TASK_STATE_WORKING"}:
                if time.monotonic() - started >= deadline:
                    task = self.rpc("CancelTask", {"id": task_id}, a2a=True)
                    return {"isError": True, "structuredContent": task, "lab_timeout": True}
                time.sleep(0.25)
                task = self.rpc("GetTask", {"id": task_id}, a2a=True)
        except LabError:
            return {"isError": True, "structuredContent": task,
                    "uncertain_task_id": task_id, "lab_disconnect": True}
        return {"isError": task["status"]["state"] != "TASK_STATE_COMPLETED", "structuredContent": task}


def client(args):
    if args.connection:
        path = Path(args.connection)
        if path.is_symlink() or path.stat().st_mode & 0o077:
            raise LabError("connection file must be private and not a symlink")
        record = read_json(path)
        return Mcp(record["url"], record["token"], record.get("host"), args.timeout)
    return Mcp(args.url, os.environ.get(args.token_env), timeout=args.timeout,
               allow_http=args.allow_insecure_http)


def content(reply):
    if "structuredContent" in reply:
        return reply["structuredContent"]
    raise LabError("missing structured MCP evidence")


def check(args):
    manifest = read_json(args.manifest)
    validate_manifest(manifest)
    environment = content(client(args).call("nl2sh_inspect"))
    gate = identity_gate(environment, manifest)
    private_json(args.output, {"schema_version": 1, "identity_gate": gate, "environment": environment})
    return 0 if gate["status"] == "pass" else 1


def pointer(value, path):
    if path == "":
        return value
    if not path.startswith("/"):
        raise LabError("assertion path must be a JSON pointer")
    for key in path[1:].split("/"):
        key = key.replace("~1", "/").replace("~0", "~")
        value = value[int(key)] if isinstance(value, list) else value[key]
    return value


def evaluate_assertion(assertion, replies):
    if assertion.get("manual_review"):
        return dict(assertion, status="manual_review")
    if "equals" not in assertion:
        raise LabError("assertion requires equals or manual_review")
    try:
        actual = pointer(replies[assertion["step"]], assertion["pointer"])
    except (KeyError, IndexError, ValueError, TypeError):
        return dict(assertion, status="inconclusive", reason="missing assertion evidence")
    return dict(assertion, actual=actual, status="pass" if actual == assertion["equals"] else "fail")


def partial(value):
    if isinstance(value, dict):
        if value.get("status") in ("partial", "timed_out", "failed") or value.get("truncated") is True:
            return True
        if value.get("missing_results", 0) or value.get("observations_truncated") or value.get("output_truncated"):
            return True
        return any(partial(item) for item in value.values())
    if isinstance(value, list):
        return any(partial(item) for item in value)
    if isinstance(value, str) and value.startswith(("{", "[")):
        try:
            return partial(json.loads(value))
        except json.JSONDecodeError:
            pass
    if isinstance(value, str) and "NL2SH OUTPUT TRUNCATED" in value:
        return True
    return False


def step_failed(reply):
    if "structuredContent" not in reply:
        return True
    data = reply.get("structuredContent", {})
    if isinstance(data, dict) and "artifacts" in data:
        state = data.get("status", {}).get("state")
        if state != "TASK_STATE_COMPLETED":
            return True
        data = data["artifacts"][0]["parts"][0]["data"]
    if not isinstance(data, dict):
        return True
    return bool(reply.get("isError") or data.get("success") is False or data.get("failed_tools")
                or any(item.get("success") is False for item in data.get("evidence", {}).get("observations", [])))


def redact(value, secrets):
    if isinstance(value, str):
        for secret in secrets:
            if secret:
                value = value.replace(secret, "[REDACTED]")
        return value
    if isinstance(value, list):
        return [redact(item, secrets) for item in value]
    if isinstance(value, dict):
        return {key: redact(item, secrets) for key, item in value.items()}
    return value


def run_case(args):
    case = read_json(args.case)
    manifest = read_json(args.manifest)
    validate_manifest(manifest)
    if case.get("schema_version") != 1 or not case.get("id") or not case.get("assertions"):
        raise LabError("case requires schema_version 1, id and nonempty assertions")
    report = {"schema_version": 1, "case_id": case["id"], "case_sha256": digest(args.case),
              "mode": "delegated" if all(step.get("name") == "nl2sh_ask" for step in case.get("steps", [])) else
                      "mixed" if any(step.get("name") == "nl2sh_ask" for step in case.get("steps", [])) else "deterministic",
              "source": source_snapshot(), "started_at": time.time(), "status": "inconclusive",
              "observations": [], "assertions": [], "hypotheses": [], "recommendations": [],
              "limitations": [], "cleanup": []}
    replies = {}
    mcp = None
    secrets = [value for key, value in os.environ.items() if key.endswith(("API_KEY", "TOKEN"))]
    try:
        mcp = client(args)
        secrets.append(mcp.token)
        environment = content(mcp.call("nl2sh_inspect"))
        report["environment"] = environment
        report["identity_gate"] = identity_gate(environment, manifest)
        if report["identity_gate"]["status"] != "pass":
            raise LabError("running build does not match manifest; case not executed")
        available = content(mcp.call("nl2sh_tools"))["tools"]
        names = {item["name"] for item in available}
        report["tools"] = available
        required = case.get("preconditions", {}).get("required_tools", [])
        if not set(required).issubset(names):
            raise LabError("required device tools unavailable; case not executed")
        minimum = case.get("preconditions", {}).get("android_min_api", 26)
        if int(environment.get("api_level") or 0) < minimum:
            raise LabError("Android API precondition not met")
        unexpected_failure = False
        for step in case.get("steps", []):
            if step["id"] in replies:
                raise LabError("duplicate step ID")
            name = step["name"]
            if name not in {"nl2sh_invoke", "nl2sh_ask", "nl2sh_inspect", "nl2sh_tools", "nl2sh_read_screen"}:
                raise LabError("unsupported case step")
            reply = (mcp.delegate(step.get("arguments", {}), args.task_timeout)
                     if name == "nl2sh_ask" and step.get("async", False)
                     else mcp.call(name, step.get("arguments", {})))
            replies[step["id"]] = reply
            report["observations"].append({"id": step["id"], "source": "protocol_result", "result": reply})
            unexpected_failure |= step_failed(reply) != step.get("expect_error", False)
        report["assertions"] = [evaluate_assertion(item, replies) for item in case["assertions"]]
        final_environment = content(mcp.call("nl2sh_inspect"))
        report["final_identity_gate"] = identity_gate(final_environment, manifest)
        if report["final_identity_gate"]["status"] != "pass":
            raise LabError("running build changed during case")
        states = {item["status"] for item in report["assertions"]}
        if unexpected_failure or "fail" in states:
            report["status"] = "fail"
        elif "manual_review" in states:
            report["status"] = "manual_review"
        elif "inconclusive" in states or any(partial(reply) for reply in replies.values()):
            report["limitations"].append("missing or partial evidence requires review")
        else:
            report["status"] = "pass"
    except (LabError, KeyError, IndexError, ValueError, TypeError) as error:
        report["limitations"].append(str(error))
    finally:
        for step in case.get("cleanup", []):
            if mcp is None or report.get("identity_gate", {}).get("status") != "pass":
                report["cleanup"].append({"id": step["id"], "status": "not_executed"})
                continue
            try:
                reply = mcp.call("nl2sh_invoke", step["arguments"])
                failed = step_failed(reply)
                report["cleanup"].append({"id": step["id"], "status": "fail" if failed else "pass", "result": reply})
                if failed:
                    report["status"] = "fail"
            except LabError as error:
                report["cleanup"].append({"id": step["id"], "status": "inconclusive", "reason": str(error)})
                report["status"] = "inconclusive"
        report["finished_at"] = time.time()
        private_json(args.output, redact(report, secrets))
    return 0 if report["status"] == "pass" else 1


def compare(args):
    before, after = read_json(args.before), read_json(args.after)
    compatible = bool(before.get("case_sha256") and before.get("case_id") and
                      before.get("case_sha256") == after.get("case_sha256") and before.get("case_id") == after.get("case_id"))
    changes = {key: {"before": before.get("environment", {}).get(key), "after": after.get("environment", {}).get(key)}
               for key in ("api_level", "device_abi", "uid", "selinux", "commands")
               if before.get("environment", {}).get(key) != after.get("environment", {}).get(key)}
    gates = all(item.get("identity_gate", {}).get("status") == "pass" and
                item.get("final_identity_gate", {}).get("status") == "pass" for item in (before, after))
    status = ("inconclusive" if not compatible or not gates else "fail" if after.get("status") == "fail"
              else "manual_review" if changes else after.get("status", "inconclusive"))
    private_json(args.output, {"schema_version": 1, "status": status, "same_case": compatible,
                 "environment_changes": changes, "before_status": before.get("status"),
                 "after_status": after.get("status"), "before_identity": before.get("identity_gate"),
                 "after_identity": after.get("identity_gate"), "before_assertions": before.get("assertions"),
                 "after_assertions": after.get("assertions")})
    return 0 if status == "pass" else 1


def annotate(args):
    report, analysis = read_json(args.report), read_json(args.analysis)
    ids = set()
    for observation in report.get("observations", []):
        ids.add(observation["id"])
        data = observation["result"].get("structuredContent", {})
        if "artifacts" in data:
            data = data["artifacts"][0]["parts"][0]["data"]
        for item in data.get("evidence", {}).get("observations", []):
            ids.add(f"{observation['id']}:{item['id']}")
    for hypothesis in analysis.get("hypotheses", []):
        references = hypothesis.get("supporting_evidence", [])
        if (not hypothesis.get("cause") or hypothesis.get("confidence") not in {"low", "medium", "high"}
                or not references or not set(references).issubset(ids)):
            raise LabError("hypothesis has invalid confidence or unsupported evidence references")
    report["hypotheses"] = analysis.get("hypotheses", [])
    report["recommendations"] = analysis.get("recommendations", [])
    report["limitations"].extend(analysis.get("limitations", []))
    report["analysis_source"] = "reviewer_supplied_unverified_analysis"
    private_json(args.output, redact(report, [value for key, value in os.environ.items() if key.endswith(("API_KEY", "TOKEN"))]))


class Adb:
    def __init__(self, serial):
        self.prefix = ["adb", "-s", serial]

    def call(self, *arguments, timeout=90):
        result = subprocess.run(self.prefix + list(arguments), capture_output=True, timeout=timeout)
        if result.returncode:
            raise LabError("ADB operation failed; check selected device and UID")
        if len(result.stdout) > LIMIT:
            raise LabError("ADB result exceeds 4 MiB")
        return result.stdout.decode().strip()

    def shell(self, arguments):
        return self.call("shell", shlex.join(arguments))

    def service(self, binary, config, operation):
        return json.loads(self.shell([binary, "--config", config, "service", operation, "--json"]))


def discover(adb, binary, config):
    """Use verified native status and owner connection details; never parse logs."""
    status = adb.service(binary, config, "status")
    connection = status.get("connections", {})
    if status.get("state") != "ready" or connection.get("state") != "running" or connection.get("transport") != "http":
        raise LabError("managed Web/MCP service not ready; enable protocol_start_with_service explicitly")
    web_forward = adb.call("forward", "tcp:0", f"tcp:{status['port']}")
    mcp_forward = None
    try:
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
        request = urllib.request.Request(f"http://127.0.0.1:{web_forward}/api/connections",
                                         headers={"Host": f"127.0.0.1:{status['port']}"})
        with opener.open(request, timeout=30) as response:
            raw = response.read(16 * 1024 + 1)
        if len(raw) > 16 * 1024:
            raise LabError("connection details oversized")
        details = json.loads(raw)
        # Native endpoint validates the process/lock/config/UID; compare it with
        # independent status before consuming the credential.
        if details.get("mcp_url") != connection.get("mcp_url") or details.get("state") != "running":
            raise LabError("connection changed during discovery")
        remote = urllib.parse.urlsplit(details["mcp_url"])
        port = remote.port or (443 if remote.scheme == "https" else 80)
        if remote.scheme != "http":
            raise LabError("ADB discovery supports native HTTP listeners only")
        mcp_forward = adb.call("forward", "tcp:0", f"tcp:{port}")
        record = {"url": f"http://127.0.0.1:{mcp_forward}/mcp", "host": f"127.0.0.1:{port}",
                  "token": details["token"], "serial": adb.prefix[-1],
                  "local_port": int(mcp_forward), "remote_port": port}
        return record
    except Exception:
        if mcp_forward:
            adb.call("forward", "--remove", "tcp:" + mcp_forward)
        raise
    finally:
        adb.call("forward", "--remove", "tcp:" + web_forward)


def deploy(args):
    manifest = read_json(args.manifest)
    artifact = args.artifact or manifest["artifact"]
    validate_manifest(manifest, artifact)
    if not all(path.startswith("/") and "\n" not in path for path in (args.binary, args.config)):
        raise LabError("device binary and configuration require absolute paths")
    adb = Adb(args.serial)
    report = {"schema_version": 1, "status": "inconclusive", "phase": "preflight", "rollback": "not_needed"}
    suffix = uuid.uuid4().hex
    staged = args.binary + ".device-lab-stage-" + suffix
    backup = args.binary + ".device-lab-previous-" + suffix
    replaced = False
    had_binary = False
    stopped = False
    backup_created = False
    connection = None
    try:
        had_binary = adb.shell(["sh", "-c", 'if [ -f "$1" ]; then echo yes; else echo no; fi', "sh", args.binary]) == "yes"
        # This workflow never rewrites model/safety configuration or changes UID.
        # Package/Helper-managed installations must be updated by their owner.
        if adb.shell(["sh", "-c", 'if [ -e "$1.owner.json" ]; then echo yes; else echo no; fi', "sh", args.binary]) == "yes":
            raise LabError("Helper-owned installation requires its management entry point")
        abi = adb.shell(["getprop", "ro.product.cpu.abilist"])
        mapping = {"x86_64-linux-android": "x86_64", "aarch64-linux-android": "arm64-v8a", "armv7-linux-androideabi": "armeabi-v7a"}
        if mapping[manifest["build_target"]] not in abi.split(","):
            raise LabError("device ABI does not support this artifact")
        adb.shell(["test", "-r", args.config])
        report["phase"] = "stage"
        adb.call("push", str(artifact), staged)
        adb.shell(["chmod", "755", staged])
        staged_hash = adb.shell(["toybox", "sha256sum", staged]).split()[0]
        if staged_hash != manifest["binary_sha256"]:
            raise LabError("device staged digest mismatch")
        if had_binary:
            old_status = adb.service(args.binary, args.config, "status")
            report["before_state"] = old_status["state"]
            report["before_sha256"] = adb.shell(["toybox", "sha256sum", args.binary]).split()[0]
            # An independently started protocol is not owned by service stop.
            if old_status.get("connections", {}).get("state") == "running" and old_status["state"] == "stopped":
                raise LabError("independent protocol must be stopped by its owner before deployment")
            adb.service(args.binary, args.config, "stop")
            stopped = True
            adb.shell(["mv", args.binary, backup])
            backup_created = True
        report["phase"] = "replace"
        adb.shell(["mv", staged, args.binary])
        replaced = True
        report["phase"] = "restart"
        adb.service(args.binary, args.config, "start")
        connection = discover(adb, args.binary, args.config)
        mcp = Mcp(connection["url"], connection["token"], connection["host"])
        environment = content(mcp.call("nl2sh_inspect"))
        report["identity_gate"] = identity_gate(environment, manifest)
        if report["identity_gate"]["status"] != "pass":
            raise LabError("restarted service identity mismatch")
        private_json(args.connection_output, connection)
        report.update(status="pass", phase="verified", environment=environment,
                      backup=backup if had_binary else None,
                      connection_file=str(args.connection_output))
    except Exception as error:
        report["reason"] = str(error) if isinstance(error, LabError) else type(error).__name__
        if replaced:
            report["rollback"] = "failed"
            try:
                # Use previous controller if the candidate cannot execute.
                controller = backup if had_binary else args.binary
                adb.service(controller, args.config, "stop")
                if had_binary:
                    adb.shell(["mv", backup, args.binary])
                    if report.get("before_state") == "ready":
                        adb.service(args.binary, args.config, "start")
                    restored = adb.shell(["toybox", "sha256sum", args.binary]).split()[0]
                    report["rollback"] = "restored" if restored == report["before_sha256"] else "failed"
                else:
                    report["rollback"] = "candidate_stopped"
            except Exception:
                pass
        elif stopped and had_binary:
            # Recover even when stopping succeeded but a rename did not.
            try:
                if backup_created:
                    adb.shell(["mv", backup, args.binary])
                if report.get("before_state") == "ready":
                    adb.service(args.binary, args.config, "start")
                restored = adb.shell(["toybox", "sha256sum", args.binary]).split()[0]
                report["rollback"] = "restored" if restored == report["before_sha256"] else "failed"
            except Exception:
                report["rollback"] = "failed"
    finally:
        if connection and report["status"] != "pass":
            try:
                adb.call("forward", "--remove", f"tcp:{connection['local_port']}")
            except LabError:
                report["forward_cleanup"] = "failed"
        # Only the unique file created by this operation is removed.
        try:
            adb.shell(["rm", "-f", staged])
        except (LabError, subprocess.SubprocessError):
            report["stage_cleanup"] = "inconclusive"
        private_json(args.output, report)
    return 0 if report["status"] == "pass" else 1


def parser():
    parse = argparse.ArgumentParser(description=__doc__)
    commands = parse.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build")
    build_parser.add_argument("--target", required=True, choices=["aarch64-linux-android", "armv7-linux-androideabi", "x86_64-linux-android"])
    build_parser.add_argument("--manifest", required=True)
    build_parser.set_defaults(action=build)
    for name, action in (("check", check), ("run", run_case)):
        command = commands.add_parser(name)
        endpoint = command.add_mutually_exclusive_group(required=True)
        endpoint.add_argument("--url")
        endpoint.add_argument("--connection")
        command.add_argument("--token-env", default="NL2SH_PROTOCOL_TOKEN")
        command.add_argument("--allow-insecure-http", action="store_true")
        command.add_argument("--timeout", type=float, default=210)
        command.add_argument("--manifest", required=True)
        command.add_argument("--output", required=True)
        if name == "run":
            command.add_argument("--case", required=True)
            command.add_argument("--task-timeout", type=float, default=600)
        command.set_defaults(action=action)
    comparison = commands.add_parser("compare")
    for field in ("before", "after", "output"):
        comparison.add_argument("--" + field, required=True)
    comparison.set_defaults(action=compare)
    deployment = commands.add_parser("deploy")
    for field in ("serial", "binary", "config", "manifest", "connection-output", "output"):
        deployment.add_argument("--" + field, required=True)
    deployment.add_argument("--artifact")
    deployment.set_defaults(action=deploy)
    annotation = commands.add_parser("annotate")
    for field in ("report", "analysis", "output"):
        annotation.add_argument("--" + field, required=True)
    annotation.set_defaults(action=annotate)
    return parse


def main():
    args = parser().parse_args()
    try:
        return args.action(args) or 0
    except (LabError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        # Never print request bodies, credentials, or subprocess output here.
        print(f"device-lab: {type(error).__name__}: operation failed; inspect private report or command context", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
