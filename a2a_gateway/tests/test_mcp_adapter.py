"""MCP tool calls must traverse the real A2A JSON-RPC boundary."""

import tempfile
import unittest
import base64
import asyncio
import os
import socket
import sys
from pathlib import Path

import httpx
from mcp import Client
from mcp.client.stdio import StdioServerParameters
import uvicorn

from nl2sh_a2a.mcp_adapter import A2AClient, A2ASettings, MAX_REPLY_BYTES, create_server
from nl2sh_a2a.server import Settings, create_app


class FakeDevice:
    def __init__(self):
        self.sessions = []

    async def call(self, operation, payload=None):
        if operation == "inspect":
            return {"kind": "android_environment", "status": "complete"}
        if operation == "tools":
            return [{"name": "inspect_android_environment"}]
        if operation == "invoke":
            if payload["tool"] == "android.read_screen":
                return {"tool": payload["tool"], "success": True, "output": "screen",
                        "attachments": [{"media_type": "image/png",
                                         "base64_data": base64.b64encode(b"\x89PNG\r\n\x1a\n").decode()}]}
            return {"tool": payload["tool"], "success": True, "output": "direct"}
        self.sessions.append(payload["session"])
        return {"answer": "device-ready", "failed_tools": []}


class AdapterTests(unittest.IsolatedAsyncioTestCase):
    async def test_installed_stdio_entrypoint_calls_live_a2a_gateway(self):
        entrypoint = Path(sys.executable).with_name("nl2sh-a2a-mcp")
        self.assertTrue(entrypoint.is_file(), "install the gateway package in this Python environment")
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        listener.bind(("127.0.0.1", 0))
        listener.listen(128)
        port = listener.getsockname()[1]
        url = f"http://127.0.0.1:{port}"
        with tempfile.TemporaryDirectory() as directory:
            device = FakeDevice()
            app = create_app(Settings(device, "t" * 32, str(Path(directory) / "tasks.db"), url))
            server = uvicorn.Server(uvicorn.Config(app, log_level="error", lifespan="off"))
            serving = asyncio.create_task(server.serve(sockets=[listener]))
            try:
                for _ in range(100):
                    if server.started:
                        break
                    await asyncio.sleep(0.01)
                self.assertTrue(server.started, "A2A gateway did not start")
                environment = os.environ.copy()
                environment.update({"NL2SH_A2A_URL": url, "NL2SH_A2A_TOKEN": "t" * 32})
                async with Client(StdioServerParameters(
                    command=str(entrypoint), env=environment,
                ), read_timeout_seconds=10) as client:
                    tools = await client.list_tools()
                    self.assertIn("nl2sh_invoke", {tool.name for tool in tools.tools})
                    direct = await client.call_tool("nl2sh_invoke", {
                        "tool": "inspect_android_ui", "arguments": {},
                    })
                    self.assertEqual(direct.structured_content["result"]["output"], "direct")
                    screen = await client.call_tool("nl2sh_read_screen")
                    self.assertEqual(screen.content[1].type, "image")
                    self.assertEqual(screen.content[1].mime_type, "image/png")
                    self.assertEqual(device.sessions, [])
            finally:
                server.should_exit = True
                await serving

    async def test_mcp_discovery_a2a_auth_and_context(self):
        with tempfile.TemporaryDirectory() as directory:
            device = FakeDevice()
            app = create_app(Settings(
                device, "t" * 32, str(Path(directory) / "tasks.db"),
                "http://127.0.0.1:8765",
            ))
            transport = httpx.ASGITransport(app=app)
            settings = A2ASettings("http://127.0.0.1:8765", "t" * 32)
            async with Client(create_server(A2AClient(settings, transport))) as client:
                listed = await client.list_tools()
                self.assertEqual({tool.name for tool in listed.tools}, {
                    "nl2sh_inspect", "nl2sh_tools", "nl2sh_ask", "nl2sh_invoke", "nl2sh_read_screen", "nl2sh_get_task",
                })
                invoke_tool = next(tool for tool in listed.tools if tool.name == "nl2sh_invoke")
                self.assertIn("without a device LLM", invoke_tool.description)
                environment = await client.call_tool("nl2sh_inspect")
                self.assertEqual(environment.structured_content["result"]["kind"], "android_environment")
                tools = await client.call_tool("nl2sh_tools")
                self.assertEqual(tools.structured_content["result"][0]["name"], "inspect_android_environment")
                direct = (await client.call_tool("nl2sh_invoke", {
                    "tool": "inspect_android_ui", "arguments": {},
                })).structured_content
                self.assertEqual(direct["result"]["output"], "direct")
                self.assertEqual(device.sessions, [])
                screen = await client.call_tool("nl2sh_read_screen")
                self.assertEqual(screen.content[1].type, "image")
                self.assertEqual(screen.content[1].mime_type, "image/png")
                first = (await client.call_tool("nl2sh_ask", {"message": "first"})).structured_content
                second = (await client.call_tool("nl2sh_ask", {
                    "message": "second", "context_id": first["context_id"],
                })).structured_content
                self.assertEqual(first["result"]["answer"], "device-ready")
                self.assertEqual(first["context_id"], second["context_id"])
                self.assertEqual(device.sessions[0], device.sessions[1])
                saved = (await client.call_tool("nl2sh_get_task", {
                    "task_id": first["task_id"],
                })).structured_content
                self.assertEqual(saved["state"], "TASK_STATE_COMPLETED")

    async def test_rejects_unsafe_urls_and_wrong_token(self):
        with self.assertRaises(ValueError):
            A2ASettings("http://example.com", "t" * 32)
        self.assertEqual(A2ASettings("http://192.168.1.10:8765", "t" * 32,
                                     allow_insecure_http=True).base_url,
                         "http://192.168.1.10:8765")
        with self.assertRaises(ValueError):
            A2ASettings("https://example.com/path", "t" * 32)
        with self.assertRaises(ValueError):
            A2ASettings("http://127.0.0.1:8765", "short")
        with tempfile.TemporaryDirectory() as directory:
            app = create_app(Settings(
                FakeDevice(), "t" * 32, str(Path(directory) / "tasks.db"),
                "http://127.0.0.1:8765",
            ))
            wrong = A2AClient(
                A2ASettings("http://127.0.0.1:8765", "x" * 32),
                httpx.ASGITransport(app=app),
            )
            with self.assertRaises(httpx.HTTPStatusError) as caught:
                await wrong.send("/inspect")
            self.assertEqual(caught.exception.response.status_code, 401)

    async def test_remote_ip_gateway_origin_matches_agent_card(self):
        with tempfile.TemporaryDirectory() as directory:
            url = "http://192.168.1.10:8765"
            app = create_app(Settings(
                FakeDevice(), "t" * 32, str(Path(directory) / "tasks.db"), url,
            ))
            client = A2AClient(
                A2ASettings(url, "t" * 32, allow_insecure_http=True),
                httpx.ASGITransport(app=app),
            )
            result = await client.send("/inspect")
            self.assertEqual(result["result"]["kind"], "android_environment")

    async def test_agent_card_cannot_redirect_bearer_token(self):
        requests = []

        def respond(request):
            requests.append(request)
            return httpx.Response(200, json={"supportedInterfaces": [{
                "protocolBinding": "JSONRPC", "protocolVersion": "1.0",
                "url": "https://another-host.example/a2a",
            }]})

        client = A2AClient(
            A2ASettings("https://gateway.example", "t" * 32),
            httpx.MockTransport(respond),
        )
        with self.assertRaisesRegex(ValueError, "configured origin"):
            await client.send("/inspect")
        self.assertEqual([request.method for request in requests], ["GET"])

    async def test_a2a_response_stops_reading_at_limit(self):
        class OversizedStream(httpx.AsyncByteStream):
            async def __aiter__(self):
                yield b"x" * (MAX_REPLY_BYTES + 1)
                raise AssertionError("response continued reading after the limit")

        transport = httpx.MockTransport(
            lambda _request: httpx.Response(200, stream=OversizedStream())
        )
        adapter = A2AClient(A2ASettings("https://gateway.example", "t" * 32), transport)
        async with httpx.AsyncClient(transport=transport) as http_client:
            with self.assertRaisesRegex(ValueError, "A2A response exceeds size limit"):
                await adapter._request(http_client, "GET", "https://gateway.example/a2a")


if __name__ == "__main__":
    unittest.main()
