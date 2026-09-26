"""Bounded adb transport for nl2sh's narrow JSON bridge."""

from __future__ import annotations

import asyncio
import base64
import json
from dataclasses import dataclass


MAX_REPLY = 256 * 1024


@dataclass(frozen=True)
class Device:
    serial: str
    binary: str = "/data/local/tmp/nl2sh"
    config: str = "/data/local/tmp/config.toml"

    async def call(self, operation: str, payload: dict | None = None) -> object:
        if operation not in {"inspect", "tools", "ask"}:
            raise ValueError("unsupported device operation")
        request = json.dumps(payload, ensure_ascii=False).encode() if payload else b""
        if len(request) > 16 * 1024:
            raise ValueError("device request exceeds size limit")
        extra = ["--payload-base64", base64.urlsafe_b64encode(request).rstrip(b"=").decode()] if payload else []
        process = await asyncio.create_subprocess_exec(
            "adb", "-s", self.serial, "exec-out", self.binary,
            "--config", self.config, "bridge", operation, *extra,
            stdin=asyncio.subprocess.DEVNULL,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            stdout, stderr = await asyncio.wait_for(
                process.communicate(), timeout=180 if operation == "ask" else 30
            )
        except (asyncio.TimeoutError, asyncio.CancelledError):
            process.kill()
            await process.wait()
            raise
        if len(stdout) > MAX_REPLY or len(stderr) > MAX_REPLY:
            raise RuntimeError("device reply exceeds size limit")
        if process.returncode:
            detail = stderr.decode(errors="replace").strip()[:2000]
            raise RuntimeError(f"nl2sh bridge failed: {detail or process.returncode}")
        try:
            return json.loads(stdout)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise RuntimeError("nl2sh bridge returned invalid JSON") from error
