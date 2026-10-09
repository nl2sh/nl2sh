import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from types import SimpleNamespace
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("device_lab", Path(__file__).parents[1] / "device_lab.py")
lab = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(lab)


class DeviceLabTests(unittest.TestCase):
    def test_identity_requires_exact_running_image_and_all_fields(self):
        manifest = dict(binary_sha256="a" * 64, build_id="test", build_target="android",
                        build_profile="release", git_commit="abc", git_dirty=True)
        self.assertEqual(lab.identity_gate({"build_identity": manifest}, manifest)["status"], "pass")
        for field in manifest:
            changed = dict(manifest)
            changed.pop(field)
            self.assertEqual(lab.identity_gate({"build_identity": changed}, manifest)["status"], "fail")
        changed = dict(manifest, binary_sha256="b" * 64)
        self.assertEqual(lab.identity_gate({"build_identity": changed}, manifest)["status"], "fail")

    def test_missing_assertion_is_not_pass_and_manual_requires_review(self):
        assertion = dict(step="s", pointer="/success", equals=True)
        self.assertEqual(lab.evaluate_assertion(assertion, {})["status"], "inconclusive")
        self.assertEqual(lab.evaluate_assertion(assertion, {"s": {"success": False}})["status"], "fail")
        self.assertEqual(lab.evaluate_assertion({"manual_review": True}, {})["status"], "manual_review")
        self.assertEqual(lab.pointer({"a/b": {"~": [4]}}, "/a~1b/~0/0"), 4)

    def test_completed_agent_with_internal_failure_is_failed(self):
        data = {"failed_tools": ["denied"]}
        reply = {"isError": False, "structuredContent": {"status": {"state": "TASK_STATE_COMPLETED"},
                "artifacts": [{"parts": [{"data": data}]}]}}
        self.assertTrue(lab.step_failed(reply))
        self.assertTrue(lab.step_failed({"isError": False, "content": []}))
        data.clear()
        data["evidence"] = {"observations": [{"success": False}]}
        self.assertTrue(lab.step_failed(reply))

    def test_partial_and_bounded_outputs_are_not_complete(self):
        for value in ({"status": "partial"}, {"output_truncated": True},
                      {"missing_results": 1}, {"output": '{"truncated":true}'},
                      "NL2SH OUTPUT TRUNCATED: omitted"):
            self.assertTrue(lab.partial(value))
        self.assertFalse(lab.partial({"success": True, "output": "complete"}))
        self.assertFalse(lab.partial({"status": {"state": "TASK_STATE_COMPLETED"}}))

    def test_private_report_and_recursive_redaction(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "report.json"
            lab.private_json(path, lab.redact({"a": ["secret token"]}, ["secret", "token"]))
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(lab.read_json(path), {"a": ["[REDACTED] [REDACTED]"]})

    def test_manifest_rejects_stale_sources_and_artifacts(self):
        manifest = dict(schema_version=1, build_id="x", source_sha256="abc", git_commit="def")
        with patch.object(lab, "source_snapshot", return_value={"source_sha256": "bad", "git_commit": "def"}):
            with self.assertRaises(lab.LabError):
                lab.validate_manifest(manifest)
        with patch.object(lab, "source_snapshot", return_value=manifest), patch.object(lab, "digest", return_value="bad"):
            with self.assertRaises(lab.LabError):
                lab.validate_manifest(manifest, "artifact")

    def test_connection_file_rejects_public_permissions(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "connection.json"
            path.write_text("{}")
            path.chmod(0o644)
            with self.assertRaises(lab.LabError):
                lab.client(SimpleNamespace(connection=str(path)))

    def test_case_comparison_requires_same_case_and_both_version_gates(self):
        with tempfile.TemporaryDirectory() as root:
            args = SimpleNamespace(before=Path(root) / "before", after=Path(root) / "after", output=Path(root) / "out")
            report = dict(case_id="x", case_sha256="abc", status="pass", environment={"uid": 2000},
                          identity_gate={"status": "pass"}, final_identity_gate={"status": "pass"})
            lab.private_json(args.before, report)
            lab.private_json(args.after, dict(report, case_sha256="changed"))
            self.assertEqual(lab.compare(args), 1)
            lab.private_json(args.after, dict(report, environment={"uid": 0}))
            lab.compare(args)
            self.assertEqual(lab.read_json(args.output)["status"], "manual_review")
            lab.private_json(args.after, report)
            self.assertEqual(lab.compare(args), 0)

    def test_hypotheses_require_real_evidence_links(self):
        with tempfile.TemporaryDirectory() as root:
            args = SimpleNamespace(report=Path(root) / "report", analysis=Path(root) / "analysis", output=Path(root) / "out")
            lab.private_json(args.report, {"observations": [{"id": "one", "result": {"structuredContent": {}}}], "limitations": []})
            hypothesis = dict(cause="permission", confidence="medium", supporting_evidence=["invented"])
            lab.private_json(args.analysis, {"hypotheses": [hypothesis]})
            with self.assertRaises(lab.LabError):
                lab.annotate(args)
            hypothesis["supporting_evidence"] = ["one"]
            lab.private_json(args.analysis, {"hypotheses": [hypothesis]})
            lab.annotate(args)
            self.assertEqual(lab.read_json(args.output)["analysis_source"], "reviewer_supplied_unverified_analysis")

    def test_async_timeout_cancels_known_id_without_resubmission(self):
        client = object.__new__(lab.Mcp)
        calls = []
        def rpc(method, params, **kwargs):
            calls.append((method, params))
            if method == "SendMessage":
                return {"task": {"id": "known", "status": {"state": "TASK_STATE_WORKING"}}}
            return {"id": "known", "status": {"state": "TASK_STATE_CANCELED"}}
        client.rpc = rpc
        result = client.delegate({"message": "investigate"}, 0)
        self.assertTrue(result["lab_timeout"])
        self.assertEqual([call[0] for call in calls], ["SendMessage", "CancelTask"])
        self.assertEqual(calls[1][1]["id"], "known")

    def test_deployment_failure_restores_binary_and_owned_service(self):
        with tempfile.TemporaryDirectory() as root:
            binary = "/data/local/tmp/lab/nl2sh"
            files = {binary: "old"}
            operations = []
            class FakeAdb:
                def __init__(self, serial):
                    pass
                def shell(self, argv):
                    operations.append(argv)
                    if argv[0] == "sh":
                        return "no" if ".owner.json" in argv[2] else "yes"
                    if argv[:2] == ["toybox", "sha256sum"]:
                        return files[argv[2]] + " file"
                    if argv[0] == "mv":
                        files[argv[2]] = files.pop(argv[1])
                    if argv[0] == "rm":
                        files.pop(argv[-1], None)
                    return ""
                def call(self, *argv):
                    if argv[0] == "push":
                        files[argv[2]] = "new"
                    return ""
                def service(self, path, config, action):
                    operations.append(["service", action, files[path]])
                    if action == "status":
                        return {"state": "ready", "connections": {"state": "running"}}
                    if action == "start" and files[path] == "new":
                        raise lab.LabError("candidate startup failed")
                    return {"state": "ready" if action == "start" else "stopped"}
            args = SimpleNamespace(manifest=Path(root) / "manifest", artifact="candidate", binary=binary,
                                   config="/data/local/tmp/lab/config.toml", serial="test", output=Path(root) / "report",
                                   connection_output=Path(root) / "connection")
            lab.private_json(args.manifest, {"binary_sha256": "new", "build_target": "x86_64-linux-android"})
            original_shell = FakeAdb.shell
            def shell(self, argv):
                return "x86_64" if argv[0] == "getprop" else original_shell(self, argv)
            FakeAdb.shell = shell
            with patch.object(lab, "Adb", FakeAdb), patch.object(lab, "validate_manifest"):
                self.assertEqual(lab.deploy(args), 1)
            self.assertEqual(files[binary], "old")
            self.assertEqual(lab.read_json(args.output)["rollback"], "restored")
            self.assertIn(["service", "start", "old"], operations)
            self.assertFalse(args.connection_output.exists())
            self.assertFalse(any(argv[0] in {"su", "root"} for argv in operations))

    def test_offline_deployment_records_inconclusive_and_cleanup_failure(self):
        class OfflineAdb:
            def __init__(self, serial):
                pass
            def shell(self, argv):
                raise lab.LabError("device unreachable")
        with tempfile.TemporaryDirectory() as root:
            args = SimpleNamespace(manifest=Path(root) / "manifest", artifact="candidate",
                                   binary="/data/local/tmp/lab/nl2sh", config="/data/local/tmp/lab/config.toml",
                                   serial="offline", output=Path(root) / "report", connection_output=Path(root) / "connection")
            lab.private_json(args.manifest, {})
            with patch.object(lab, "Adb", OfflineAdb), patch.object(lab, "validate_manifest"):
                self.assertEqual(lab.deploy(args), 1)
            report = lab.read_json(args.output)
            self.assertEqual(report["status"], "inconclusive")
            self.assertEqual(report["stage_cleanup"], "inconclusive")

    def test_unreachable_mcp_is_explicit(self):
        # Reserve then release a local ephemeral endpoint; it has no HTTP server.
        import socket
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        with self.assertRaises(lab.LabError):
            lab.Mcp(f"http://127.0.0.1:{port}/mcp", "t" * 32, timeout=1)

    def test_native_json_handshake_and_wrong_token(self):
        methods = []
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass
            def do_POST(self):
                if self.headers.get("Authorization") != "Bearer " + "t" * 32:
                    self.send_response(401)
                    self.end_headers()
                    return
                request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                methods.append(request["method"])
                if "id" not in request:
                    self.send_response(202)
                    self.end_headers()
                    return
                result = {"protocolVersion": lab.VERSION} if request["method"] == "initialize" else {"structuredContent": {"success": True}}
                raw = json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(raw)
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            url = f"http://127.0.0.1:{server.server_port}/mcp"
            self.assertTrue(lab.content(lab.Mcp(url, "t" * 32).call("nl2sh_inspect"))["success"])
            self.assertEqual(methods[:2], ["initialize", "notifications/initialized"])
            with self.assertRaises(lab.LabError):
                lab.Mcp(url, "x" * 32)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == "__main__":
    unittest.main()
