"""Bounded adb transport for nl2sh's narrow JSON bridge."""

from __future__ import annotations

import asyncio
import base64
import json
from dataclasses import dataclass


MAX_REPLY = 3 * 1024 * 1024


async def _read_bounded(stream: asyncio.StreamReader) -> bytes:
    chunks = []
    size = 0
    while chunk := await stream.read(min(64 * 1024, MAX_REPLY - size + 1)):
        size += len(chunk)
        if size > MAX_REPLY:
            raise RuntimeError("device reply exceeds size limit")
        chunks.append(chunk)
    return b"".join(chunks)


@dataclass(frozen=True)
class Device:
    serial: str
    binary: str = "/data/local/tmp/nl2sh"
    config: str = "/data/local/tmp/config.toml"

    async def call(self, operation: str, payload: dict | None = None) -> object:
        if operation not in {"inspect", "tools", "ask", "invoke"}:
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
        stdout_task = asyncio.create_task(_read_bounded(process.stdout))
        stderr_task = asyncio.create_task(_read_bounded(process.stderr))
        try:
            async def collect() -> tuple[bytes, bytes]:
                stdout, stderr = await asyncio.gather(stdout_task, stderr_task)
                await process.wait()
                return stdout, stderr

            stdout, stderr = await asyncio.wait_for(
                collect(), timeout=180 if operation in {"ask", "invoke"} else 30
            )
        except BaseException:
            stdout_task.cancel()
            stderr_task.cancel()
            if process.returncode is None:
                try:
                    process.kill()
                except ProcessLookupError:
                    pass
            await process.wait()
            await asyncio.gather(stdout_task, stderr_task, return_exceptions=True)
            raise
        if process.returncode:
            detail = stderr.decode(errors="replace").strip()[:2000]
            raise RuntimeError(f"nl2sh bridge failed: {detail or process.returncode}")
        try:
            return json.loads(stdout)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            # `adb exec-out` can report success after the remote CLI exits with
            # an error and prints its anyhow message instead of JSON.
            detail = stdout.decode(errors="replace").strip()
            if detail.startswith("Error: "):
                raise RuntimeError(f"nl2sh bridge failed: {detail[:2000]}") from error
            raise RuntimeError("nl2sh bridge returned invalid JSON") from error
