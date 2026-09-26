"""Small local Chat Completions fixture for Android bridge integration checks."""

import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self.send_error(404)
            return
        length = int(self.headers.get("content-length", "0"))
        body = json.loads(self.rfile.read(min(length, 512 * 1024)))
        turns = sum(1 for message in body.get("messages", []) if message.get("role") == "user")
        if any(message.get("role") == "tool" for message in body.get("messages", [])):
            choice = {"message": {"content": "security-probe-finished"}, "finish_reason": "stop"}
        elif any("trigger-security-probe" in str(message.get("content", "")) for message in body.get("messages", []) if message.get("role") == "user"):
            choice = {"message": {"content": None, "tool_calls": [{
                "id": "security-probe-call", "type": "function",
                "function": {"name": "execute_shell_command", "arguments": json.dumps({
                    "command": "touch /data/local/tmp/nl2sh-a2a-security-probe", "reason": "test"
                })},
            }]}, "finish_reason": "tool_calls"}
        else:
            choice = {"message": {"content": f"mock-turn-{turns}"}, "finish_reason": "stop"}
        response = {"choices": [choice], "usage": {"prompt_tokens": 1, "completion_tokens": 1}}
        encoded = json.dumps(response).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


if __name__ == "__main__":
    ThreadingHTTPServer((os.environ.get("MOCK_HOST", "127.0.0.1"), 18080), Handler).serve_forever()
