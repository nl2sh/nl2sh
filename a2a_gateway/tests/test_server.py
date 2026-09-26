"""Protocol boundary checks using the official A2A HTTP routes."""

import json
import tempfile
import unittest
from pathlib import Path

import httpx

from nl2sh_a2a.server import Settings, create_app


class FakeDevice:
    sessions = []

    async def call(self, operation, payload=None):
        if operation == "inspect":
            return {"kind": "android_environment", "status": "complete"}
        if operation == "tools":
            return [{"name": "inspect_android_environment"}]
        self.sessions.append(payload["session"])
        return {"session": payload["session"], "answer": "ready"}


class GatewayTests(unittest.IsolatedAsyncioTestCase):
    async def test_card_auth_and_task(self):
        with tempfile.TemporaryDirectory() as directory:
            app = create_app(Settings(
                FakeDevice(), "x" * 32, str(Path(directory) / "tasks.db"),
                "http://127.0.0.1:8765",
            ))
            transport = httpx.ASGITransport(app=app)
            async with httpx.AsyncClient(transport=transport, base_url="http://test") as client:
                card = await client.get("/.well-known/agent-card.json")
                self.assertEqual(card.status_code, 200)
                self.assertEqual(card.json()["supportedInterfaces"][0]["protocolVersion"], "1.0")
                request = {
                    "jsonrpc": "2.0", "id": 1, "method": "SendMessage",
                    "params": {"message": {
                        "messageId": "test-message-1", "role": "ROLE_USER",
                        "parts": [{"text": "/inspect"}],
                    }},
                }
                rejected = await client.post("/a2a", json=request)
                self.assertEqual(rejected.status_code, 401)
                response = await client.post(
                    "/a2a", json=request,
                    headers={"Authorization": "Bearer " + "x" * 32, "A2A-Version": "1.0"},
                )
                self.assertEqual(response.status_code, 200, response.text)
                self.assertNotIn("error", response.json())
                self.assertIn("android_environment", json.dumps(response.json()))

    async def test_followup_and_restart_keep_task_and_device_session(self):
        with tempfile.TemporaryDirectory() as directory:
            db = str(Path(directory) / "tasks.db")
            device = FakeDevice()
            device.sessions = []
            token = "y" * 32
            settings = Settings(device, token, db, "http://127.0.0.1:8765")
            headers = {"Authorization": "Bearer " + token, "A2A-Version": "1.0"}
            transport = httpx.ASGITransport(app=create_app(settings))
            async with httpx.AsyncClient(transport=transport, base_url="http://test") as client:
                first = await client.post("/a2a", headers=headers, json={
                    "jsonrpc": "2.0", "id": 1, "method": "SendMessage",
                    "params": {"message": {"messageId": "follow-1", "role": "ROLE_USER", "parts": [{"text": "question one"}]}}
                })
                task = first.json()["result"]["task"]
                self.assertEqual(task["status"]["state"], "TASK_STATE_COMPLETED")
                context = task["contextId"]
                task_id = task["id"]
            # A new app simulates restarting the gateway while retaining SQLite tasks.
            transport = httpx.ASGITransport(app=create_app(settings))
            async with httpx.AsyncClient(transport=transport, base_url="http://test") as client:
                saved = await client.post("/a2a", headers=headers, json={
                    "jsonrpc": "2.0", "id": 2, "method": "GetTask", "params": {"id": task_id}
                })
                self.assertEqual(saved.json()["result"]["status"]["state"], "TASK_STATE_COMPLETED")
                second = await client.post("/a2a", headers=headers, json={
                    "jsonrpc": "2.0", "id": 3, "method": "SendMessage",
                    "params": {"message": {"messageId": "follow-2", "contextId": context,
                                           "role": "ROLE_USER", "parts": [{"text": "question two"}]}}
                })
                self.assertEqual(second.json()["result"]["task"]["contextId"], context)
                self.assertEqual(device.sessions[0], device.sessions[1])


if __name__ == "__main__":
    unittest.main()
