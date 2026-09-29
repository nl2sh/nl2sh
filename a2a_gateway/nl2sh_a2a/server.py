"""A2A 1.0 gateway backed by the official Python SDK."""

from __future__ import annotations

import asyncio
import hashlib
import json
from contextlib import asynccontextmanager
from dataclasses import dataclass

import httpx

from a2a.server.agent_execution import AgentExecutor, RequestContext
from a2a.server.events import EventQueue
from a2a.server.request_handlers import DefaultRequestHandler
from a2a.server.routes import create_agent_card_routes, create_jsonrpc_routes
from a2a.server.tasks import TaskUpdater
from a2a.server.tasks.database_task_store import DatabaseTaskStore
from a2a.types import (
    AgentCapabilities, AgentCard, AgentInterface, AgentSkill, HTTPAuthSecurityScheme,
    Part, SecurityRequirement, SecurityScheme, StringList, Task, TaskState, TaskStatus,
)
from sqlalchemy.ext.asyncio import create_async_engine
from starlette.applications import Starlette
from starlette.authentication import SimpleUser
from starlette.responses import PlainTextResponse

from .device import Device
from .mcp_adapter import A2AClient, A2ASettings, create_server


@dataclass(frozen=True)
class Settings:
    device: Device
    token: str
    db_path: str
    advertised_url: str


class BearerAuth:
    """Authenticate every A2A operation while leaving the public card discoverable."""

    def __init__(self, app, token: str):
        self.app = app
        self.token = token

    async def __call__(self, scope, receive, send):
        if scope["type"] != "http" or scope["path"] == "/.well-known/agent-card.json":
            await self.app(scope, receive, send)
            return
        from hmac import compare_digest
        headers = dict(scope.get("headers", []))
        authorization = headers.get(b"authorization", b"").decode(errors="ignore")
        expected = f"Bearer {self.token}"
        if not compare_digest(authorization, expected):
            response = PlainTextResponse("unauthorized", status_code=401)
            await response(scope, receive, send)
            return
        scope["user"] = SimpleUser("gateway-owner")
        await self.app(scope, receive, send)


class DeviceAgent(AgentExecutor):
    """Translate A2A messages into bounded nl2sh bridge operations."""

    def __init__(self, device: Device):
        self.device = device
        self._session_locks: dict[str, asyncio.Lock] = {}
        self._active: dict[str, asyncio.Task] = {}

    async def execute(self, context: RequestContext, event_queue: EventQueue) -> None:
        if not context.task_id or not context.context_id or not context.message:
            raise ValueError("missing A2A task context")
        current = asyncio.current_task()
        if current:
            self._active[context.task_id] = current
        await event_queue.enqueue_event(Task(
            id=context.task_id,
            context_id=context.context_id,
            status=TaskStatus(state=TaskState.TASK_STATE_SUBMITTED),
            history=[context.message],
        ))
        updater = TaskUpdater(event_queue, context.task_id, context.context_id)
        await updater.start_work()
        prompt = context.get_user_input().strip()
        try:
            if not prompt or len(prompt.encode()) > 8192:
                raise ValueError("message must contain 1–8192 bytes")
            if prompt == "/inspect":
                answer = await self.device.call("inspect")
            elif prompt == "/tools":
                answer = await self.device.call("tools")
            elif prompt.startswith("/invoke "):
                request = json.loads(prompt[len("/invoke "):])
                if not isinstance(request, dict) or set(request) != {"tool", "arguments"}:
                    raise ValueError("tool request must contain tool and arguments")
                if not isinstance(request["tool"], str) or not isinstance(request["arguments"], dict):
                    raise ValueError("invalid tool request")
                answer = await self.device.call("invoke", request)
            else:
                session = "a2a-" + hashlib.sha256(context.context_id.encode()).hexdigest()[:32]
                lock = self._session_locks.setdefault(session, asyncio.Lock())
                async with lock:
                    answer = await self.device.call("ask", {
                        "session": session, "message": prompt,
                    })
            await updater.add_artifact(
                parts=[Part(text=json.dumps(answer, ensure_ascii=False))],
                name="nl2sh_result",
                last_chunk=True,
            )
            await updater.complete()
        except Exception as error:
            await updater.failed(updater.new_agent_message(
                parts=[Part(text=str(error)[:2000])]
            ))
        finally:
            self._active.pop(context.task_id, None)

    async def cancel(self, context: RequestContext, event_queue: EventQueue) -> None:
        if context.task_id and context.context_id:
            active = self._active.get(context.task_id)
            if active:
                active.cancel()
            await TaskUpdater(event_queue, context.task_id, context.context_id).cancel()


def create_app(settings: Settings) -> Starlette:
    if len(settings.token) < 32:
        raise ValueError("A2A token must contain at least 32 characters")
    url = settings.advertised_url.rstrip("/")
    card = AgentCard(
        name="nl2sh Android Device Runtime",
        description="Invoke registered Android device tools directly without a device LLM; optionally consult nl2sh's built-in Agent.",
        version="0.1.0",
        supported_interfaces=[AgentInterface(
            url=f"{url}/a2a", protocol_binding="JSONRPC", protocol_version="1.0"
        )],
        capabilities=AgentCapabilities(streaming=False, push_notifications=False),
        default_input_modes=["text/plain"],
        default_output_modes=["text/plain"],
        skills=[
            AgentSkill(id="invoke", name="Invoke device tool", description="Send /invoke followed by {tool,arguments} JSON to call the device Tool Runtime directly without a model request. Device approval rules still apply.", tags=["android", "tools"]),
            AgentSkill(id="inspect", name="Inspect device", description="Send /inspect for bounded read-only Android facts.", tags=["android", "diagnostics"]),
            AgentSkill(id="tools", name="List capabilities", description="Send /tools for the device Tool Runtime catalog and argument schemas.", tags=["android", "tools"]),
            AgentSkill(id="consult", name="Consult nl2sh Agent", description="Optionally send text to the built-in device Agent; this requires a device model provider. Use the same A2A context for follow-up questions. Unattended writes are rejected.", tags=["android", "agent"]),
        ],
        security_schemes={"bearer": SecurityScheme(http_auth_security_scheme=HTTPAuthSecurityScheme(scheme="bearer"))},
        security_requirements=[SecurityRequirement(schemes={"bearer": StringList(list=[])})],
    )
    engine = create_async_engine(f"sqlite+aiosqlite:///{settings.db_path}")
    handler = DefaultRequestHandler(
        agent_executor=DeviceAgent(settings.device),
        task_store=DatabaseTaskStore(engine),
        agent_card=card,
    )
    a2a_routes = [
        *create_agent_card_routes(card),
        *create_jsonrpc_routes(handler, rpc_url="/a2a"),
    ]
    a2a_app = BearerAuth(Starlette(routes=a2a_routes), settings.token)
    # Keep Agent Card discovery and JSON-RPC on the same configured origin without
    # making an outbound network request from the HTTP MCP endpoint.
    mcp_client = A2AClient(
        A2ASettings(url, settings.token, allow_insecure_http=True),
        httpx.ASGITransport(app=a2a_app),
    )
    mcp_app = create_server(mcp_client).streamable_http_app(host="0.0.0.0")

    @asynccontextmanager
    async def lifespan(_app):
        async with mcp_app.router.lifespan_context(mcp_app):
            yield

    app = Starlette(
        routes=[*a2a_routes, *mcp_app.routes],
        lifespan=lifespan,
    )
    return BearerAuth(app, settings.token)
