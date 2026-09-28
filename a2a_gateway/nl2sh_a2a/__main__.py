"""CLI for the host-side A2A gateway."""

import argparse
import os
from urllib.parse import urlsplit
import uvicorn

from .device import Device
from .server import Settings, create_app


def validate_listener(host: str, advertised_url: str, allow_insecure_http: bool) -> None:
    if host not in {"127.0.0.1", "localhost", "0.0.0.0"}:
        raise ValueError("host must be loopback or 0.0.0.0")
    parsed = urlsplit(advertised_url)
    if (not parsed.hostname or parsed.username or parsed.password or parsed.query
            or parsed.fragment or parsed.path.rstrip("/") or parsed.scheme not in {"http", "https"}):
        raise ValueError("advertised URL must be an HTTP(S) origin without credentials or a path")
    loopback = parsed.hostname in {"127.0.0.1", "localhost", "::1"}
    if parsed.scheme == "http" and not loopback and not allow_insecure_http:
        raise ValueError("remote advertised URL requires HTTPS or explicit insecure HTTP opt-in")
    if host == "0.0.0.0" and not allow_insecure_http:
        raise ValueError("network bind requires explicit insecure HTTP opt-in")


def main() -> None:
    parser = argparse.ArgumentParser(description="Serve nl2sh over A2A 1.0")
    parser.add_argument("--serial", required=True, help="exact adb device serial")
    parser.add_argument("--binary", default="/data/local/tmp/nl2sh")
    parser.add_argument("--config", default="/data/local/tmp/config.toml")
    parser.add_argument("--db", required=True, help="persistent SQLite task database")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--advertised-url", help="URL reachable by A2A clients")
    parser.add_argument("--allow-insecure-http", action="store_true",
                        help="allow network binding and HTTP on a trusted private network")
    args = parser.parse_args()
    if args.host == "0.0.0.0" and not args.advertised_url:
        parser.error("--advertised-url is required for a network bind")
    advertised_url = args.advertised_url or f"http://{args.host}:{args.port}"
    allow_insecure_http = args.allow_insecure_http or os.environ.get("NL2SH_A2A_ALLOW_INSECURE_HTTP") == "1"
    try:
        validate_listener(args.host, advertised_url, allow_insecure_http)
    except ValueError as error:
        parser.error(str(error))
    token = os.environ.get("NL2SH_A2A_TOKEN", "")
    settings = Settings(
        device=Device(args.serial, args.binary, args.config),
        token=token,
        db_path=args.db,
        advertised_url=advertised_url,
    )
    uvicorn.run(create_app(settings), host=args.host, port=args.port)


if __name__ == "__main__":
    main()
