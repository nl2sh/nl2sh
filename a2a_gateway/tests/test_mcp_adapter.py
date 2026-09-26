"""MCP tool calls must traverse the real A2A JSON-RPC boundary."""

import tempfile
import unittest
from pathlib import Path

import httpx
from mcp import Client

from nl2sh_a2a.mcp_adapter import A2AClient, A2ASettings, create_server
from nl2sh_a2a.server import Settings, create_app


class FakeDevice:
    def __init__(self):
        self.sessions = []

    async def call(self, operation, payload=None):
        if operation == "inspect":
            return {"kind": "android_environment", "status": "complete"}
        if operation == "tools":
            return [{"name": "inspect_android_environment"}]
        self.sessions.append(payload["session"])
        return {"answer": "device-ready", "failed_tools": []}


class AdapterTests(unittest.IsolatedAsyncioTestCase):
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
                    "nl2sh_inspect", "nl2sh_tools", "nl2sh_ask", "nl2sh_get_task",
                })
                environment = await client.call_tool("nl2sh_inspect")
                self.assertEqual(environment.structured_content["result"]["kind"], "android_environment")
                tools = await client.call_tool("nl2sh_tools")
                self.assertEqual(tools.structured_content["result"][0]["name"], "inspect_android_environment")
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


if __name__ == "__main__":
    unittest.main()
