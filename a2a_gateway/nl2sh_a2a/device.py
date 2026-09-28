"""Bounded adb transport for nl2sh's narrow JSON bridge."""

from __future__ import annotations

import asyncio
import base64
import ipaddress
import json
from dataclasses import dataclass


MAX_REPLY = 3 * 1024 * 1024


def _tcp_serial(serial: str) -> bool:
    host, separator, port = serial.rpartition(":")
    if not separator or not port.isdecimal() or not 1 <= int(port) <= 65535:
        return False
    try:
        return isinstance(ipaddress.ip_address(host), ipaddress.IPv4Address)
    except ValueError:
        return False


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

    async def _connect_tcp(self) -> None:
        process = await asyncio.create_subprocess_exec(
            "adb", "connect", self.serial,
            stdin=asyncio.subprocess.DEVNULL,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            stdout, stderr = await asyncio.wait_for(process.communicate(), timeout=15)
        except BaseException:
            if process.returncode is None:
                try:
                    process.kill()
                except ProcessLookupError:
                    pass
            await process.wait()
            raise
        detail = (stdout + stderr).decode(errors="replace").strip()[:500]
        if process.returncode or any(marker in detail.lower() for marker in (
            "failed to connect", "unable to connect", "cannot connect",
        )):
            raise RuntimeError(f"adb TCP connection failed: {detail or process.returncode}")

    async def call(self, operation: str, payload: dict | None = None) -> object:
        if operation not in {"inspect", "tools", "ask", "invoke"}:
            raise ValueError("unsupported device operation")
        request = json.dumps(payload, ensure_ascii=False).encode() if payload else b""
        if len(request) > 16 * 1024:
            raise ValueError("device request exceeds size limit")
        if _tcp_serial(self.serial):
            # Reconnect before each call, never replay an operation after a lost reply.
            await self._connect_tcp()
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
