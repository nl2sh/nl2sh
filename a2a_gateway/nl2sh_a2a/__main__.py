"""CLI for the host-side A2A gateway."""

import argparse
import os
import uvicorn

from .device import Device
from .server import Settings, create_app


def main() -> None:
    parser = argparse.ArgumentParser(description="Serve nl2sh over A2A 1.0")
    parser.add_argument("--serial", required=True, help="exact adb device serial")
    parser.add_argument("--binary", default="/data/local/tmp/nl2sh")
    parser.add_argument("--config", default="/data/local/tmp/config.toml")
    parser.add_argument("--db", required=True, help="persistent SQLite task database")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--advertised-url", help="URL reachable by A2A clients")
    args = parser.parse_args()
    if args.host not in {"127.0.0.1", "localhost"}:
        parser.error("bind to loopback; use an HTTPS reverse proxy for remote clients")
    if args.advertised_url and not (
        args.advertised_url.startswith("https://")
        or args.advertised_url.startswith("http://127.0.0.1:")
        or args.advertised_url.startswith("http://localhost:")
    ):
        parser.error("advertised URL must use HTTPS except on loopback")
    token = os.environ.get("NL2SH_A2A_TOKEN", "")
    settings = Settings(
        device=Device(args.serial, args.binary, args.config),
        token=token,
        db_path=args.db,
        advertised_url=args.advertised_url or f"http://{args.host}:{args.port}",
    )
    uvicorn.run(create_app(settings), host=args.host, port=args.port)


if __name__ == "__main__":
    main()
