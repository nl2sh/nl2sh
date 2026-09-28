"""The adb transport preserves diagnostics and bounds its subprocess output."""

import asyncio
import sys
import unittest
from unittest.mock import AsyncMock, Mock, patch

from nl2sh_a2a.device import MAX_REPLY, Device, _tcp_serial


class DeviceTests(unittest.IsolatedAsyncioTestCase):
    async def test_tcp_serial_connects_before_bridge_without_replaying(self):
        connect = AsyncMock()
        connect.communicate.return_value = (b"connected to 192.168.1.50:5555\n", b"")
        connect.returncode = 0
        bridge = AsyncMock()
        bridge.stdout = asyncio.StreamReader()
        bridge.stderr = asyncio.StreamReader()
        bridge.stdout.feed_data(b'{"ready":true}')
        bridge.stdout.feed_eof()
        bridge.stderr.feed_eof()
        bridge.returncode = 0
        bridge.wait.return_value = 0
        with patch("nl2sh_a2a.device.asyncio.create_subprocess_exec",
                   side_effect=[connect, bridge]) as spawn:
            result = await Device("192.168.1.50:5555").call("inspect")
        self.assertEqual(result, {"ready": True})
        self.assertEqual(spawn.call_args_list[0].args[:3], ("adb", "connect", "192.168.1.50:5555"))
        self.assertEqual(spawn.call_args_list[1].args[:4],
                         ("adb", "-s", "192.168.1.50:5555", "exec-out"))

    async def test_failed_tcp_connect_does_not_run_bridge(self):
        connect = AsyncMock()
        connect.communicate.return_value = (b"failed to connect to 192.168.1.50:5555", b"")
        connect.returncode = 0
        with patch("nl2sh_a2a.device.asyncio.create_subprocess_exec",
                   return_value=connect) as spawn:
            with self.assertRaisesRegex(RuntimeError, "adb TCP connection failed"):
                await Device("192.168.1.50:5555").call("invoke", {
                    "tool": "android.press_back", "arguments": {},
                })
        spawn.assert_called_once()

    async def test_only_ipv4_tcp_serial_triggers_connect(self):
        self.assertTrue(_tcp_serial("192.168.1.50:5555"))
        for serial in ("emulator-5554", "USB123", "192.168.1.50:0",
                       "192.168.1.50:65536", "device.example:5555"):
            self.assertFalse(_tcp_serial(serial))

    async def test_remote_cli_error_is_not_reported_as_invalid_json(self):
        process = AsyncMock()
        process.stdout = asyncio.StreamReader()
        process.stderr = asyncio.StreamReader()
        process.stdout.feed_data(b"Error: Android UI node not found\n")
        process.stdout.feed_eof()
        process.stderr.feed_eof()
        process.returncode = 0
        process.wait.return_value = 0
        with patch("nl2sh_a2a.device.asyncio.create_subprocess_exec", return_value=process):
            with self.assertRaisesRegex(RuntimeError, "Android UI node not found"):
                await Device("emulator-5554").call("invoke", {
                    "tool": "android.tap_text", "arguments": {"text": "missing"},
                })

    async def test_oversized_stdout_kills_and_reaps_device_process(self):
        process = AsyncMock()
        process.stdout = asyncio.StreamReader()
        process.stderr = asyncio.StreamReader()
        process.stdout.feed_data(b"x" * (MAX_REPLY + 1))
        process.returncode = None
        process.kill = Mock(side_effect=lambda: setattr(process, "returncode", -9))
        process.wait.return_value = -9
        with patch("nl2sh_a2a.device.asyncio.create_subprocess_exec", return_value=process):
            with self.assertRaisesRegex(RuntimeError, "device reply exceeds size limit"):
                await asyncio.wait_for(Device("emulator-5554").call("invoke", {
                    "tool": "android.screen_dump", "arguments": {},
                }), timeout=2)
        process.kill.assert_called_once()
        process.wait.assert_awaited()

    async def test_live_pipe_stops_at_reply_limit(self):
        spawn = asyncio.create_subprocess_exec

        async def fake_adb(*_args, **kwargs):
            return await spawn(
                sys.executable, "-c",
                "import sys,time; sys.stdout.buffer.write(b'x'*1025); "
                "sys.stdout.flush(); time.sleep(30)",
                **kwargs,
            )

        with patch("nl2sh_a2a.device.MAX_REPLY", 1024), patch(
            "nl2sh_a2a.device.asyncio.create_subprocess_exec", side_effect=fake_adb
        ):
            with self.assertRaisesRegex(RuntimeError, "device reply exceeds size limit"):
                await asyncio.wait_for(Device("emulator-5554").call("inspect"), timeout=3)


if __name__ == "__main__":
    unittest.main()
