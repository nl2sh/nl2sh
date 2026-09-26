"""Local stdio MCP tools backed by the authenticated nl2sh A2A endpoint."""

from __future__ import annotations

import json
import os
import uuid
from dataclasses import dataclass
from typing import Any
from urllib.parse import urlsplit

import httpx
from mcp.server import MCPServer


MAX_REPLY_BYTES = 512 * 1024
MAX_MESSAGE_BYTES = 8192


@dataclass(frozen=True)
class A2ASettings:
    base_url: str
    token: str

    def __post_init__(self) -> None:
        parsed = urlsplit(self.base_url)
        if parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path.rstrip("/"):
            raise ValueError("A2A URL must be an origin without credentials or a path")
        if parsed.scheme != "https" and not (
            parsed.scheme == "http" and parsed.hostname in {"127.0.0.1", "localhost", "::1"}
        ):
            raise ValueError("A2A URL must use HTTPS except on loopback")
        if not parsed.hostname or len(self.token) < 32:
            raise ValueError("A2A URL and a token of at least 32 characters are required")


class A2AClient:
    """Discover one A2A JSON-RPC endpoint and send bounded authenticated calls."""

    def __init__(self, settings: A2ASettings, transport: httpx.AsyncBaseTransport | None = None):
        self.settings = settings
        self.transport = transport

    async def _request(self, client: httpx.AsyncClient, method: str, url: str, **kwargs) -> dict:
        response = await client.request(method, url, **kwargs)
        response.raise_for_status()
        if len(response.content) > MAX_REPLY_BYTES:
            raise ValueError("A2A response exceeds size limit")
        value = response.json()
        if not isinstance(value, dict):
            raise ValueError("A2A response must be an object")
        return value

    async def _rpc(self, method: str, params: dict) -> dict:
        async with httpx.AsyncClient(
            base_url=self.settings.base_url.rstrip("/"),
            transport=self.transport,
            timeout=200,
            follow_redirects=False,
            trust_env=False,
        ) as client:
            card = await self._request(client, "GET", "/.well-known/agent-card.json")
            origin = urlsplit(self.settings.base_url)
            endpoint = next((
                interface.get("url") for interface in card.get("supportedInterfaces", [])
                if isinstance(interface, dict)
                and interface.get("protocolBinding") == "JSONRPC"
                and interface.get("protocolVersion") == "1.0"
            ), None)
            target = urlsplit(endpoint or "")
            if (target.scheme, target.netloc) != (origin.scheme, origin.netloc):
                raise ValueError("Agent Card JSON-RPC URL must use the configured origin")
            if target.path != "/a2a" or target.query or target.fragment:
                raise ValueError("unexpected Agent Card JSON-RPC path")
            envelope = await self._request(
                client, "POST", endpoint,
                headers={
                    "Authorization": f"Bearer {self.settings.token}",
                    "A2A-Version": "1.0",
                },
                json={"jsonrpc": "2.0", "id": str(uuid.uuid4()),
                      "method": method, "params": params},
            )
        if "error" in envelope:
            error = envelope["error"]
            raise RuntimeError(f"A2A {method} failed: {str(error)[:500]}")
        result = envelope.get("result")
        if not isinstance(result, dict):
            raise ValueError("A2A response has no result object")
        return result

    async def send(self, text: str, context_id: str | None = None) -> dict:
        if not text.strip() or len(text.encode()) > MAX_MESSAGE_BYTES:
            raise ValueError("message must contain 1–8192 bytes")
        if context_id is not None and (not context_id or len(context_id) > 256):
            raise ValueError("context_id must contain 1–256 characters")
        message = {"messageId": str(uuid.uuid4()), "role": "ROLE_USER",
                   "parts": [{"text": text}]}
        if context_id:
            message["contextId"] = context_id
        return self._task_result(await self._rpc("SendMessage", {"message": message}))

    async def get_task(self, task_id: str) -> dict:
        if not task_id or len(task_id) > 256:
            raise ValueError("task_id must contain 1–256 characters")
        return self._task_result(await self._rpc("GetTask", {"id": task_id}))

    @staticmethod
    def _task_result(result: dict) -> dict:
        task = result.get("task", result)
        if not isinstance(task, dict) or not isinstance(task.get("id"), str):
            raise ValueError("A2A response has no task")
        state = task.get("status", {}).get("state")
        output = {"task_id": task["id"], "context_id": task.get("contextId"),
                  "state": state}
        artifacts = task.get("artifacts") or []
        if artifacts and isinstance(artifacts[0], dict):
            parts = artifacts[0].get("parts") or []
            if parts and isinstance(parts[0], dict) and isinstance(parts[0].get("text"), str):
                try:
                    output["result"] = json.loads(parts[0]["text"])
                except json.JSONDecodeError:
                    output["result"] = parts[0]["text"]
        if state == "TASK_STATE_FAILED":
            output["error"] = task.get("status", {}).get("message")
        return output


def create_server(client: A2AClient) -> MCPServer:
    server = MCPServer("nl2sh-a2a", instructions=(
        "Use nl2sh_inspect for Android facts and nl2sh_tools for capabilities. "
        "Use nl2sh_ask for device Agent consultation; preserve context_id for follow-ups. "
        "Check result.failed_tools before claiming success. Device writes require local approval "
        "and are rejected over this bridge."
    ))

    @server.tool(structured_output=True)
    async def nl2sh_inspect() -> dict[str, Any]:
        """Inspect bounded, read-only facts about the connected Android device."""
        return await client.send("/inspect")

    @server.tool(structured_output=True)
    async def nl2sh_tools() -> dict[str, Any]:
        """List the tools available to the connected nl2sh Agent."""
        return await client.send("/tools")

    @server.tool(structured_output=True)
    async def nl2sh_ask(message: str, context_id: str | None = None) -> dict[str, Any]:
        """Ask the device Agent; reuse context_id for follow-ups and check failed_tools."""
        return await client.send(message, context_id)

    @server.tool(structured_output=True)
    async def nl2sh_get_task(task_id: str) -> dict[str, Any]:
        """Read a saved A2A task by its task_id, including its current state."""
        return await client.get_task(task_id)

    return server


def main() -> None:
    settings = A2ASettings(
        base_url=os.environ.get("NL2SH_A2A_URL", "http://127.0.0.1:8765"),
        token=os.environ.get("NL2SH_A2A_TOKEN", ""),
    )
    create_server(A2AClient(settings)).run(transport="stdio")


if __name__ == "__main__":
    main()
