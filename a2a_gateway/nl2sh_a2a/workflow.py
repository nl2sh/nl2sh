"""Explicit host-side build/deploy checkpoints for an A2A development task."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile


TARGETS = {
    "arm64-v8a": "aarch64-linux-android",
    "armeabi-v7a": "armv7-linux-androideabi",
}


def run(*args: str, cwd: pathlib.Path | None = None, env: dict | None = None) -> str:
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, timeout=1800, check=False)
    if result.returncode:
        raise RuntimeError(f"{args[0]} failed ({result.returncode}): {result.stderr[-3000:]}")
    return result.stdout.strip()


def save(path: pathlib.Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temp = tempfile.mkstemp(prefix=".nl2sh-a2a-", dir=path.parent)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "w") as output:
            json.dump(data, output, sort_keys=True)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temp, path)
    finally:
        if os.path.exists(temp):
            os.unlink(temp)


def prepare(repo: pathlib.Path, serial: str, state: pathlib.Path, build_dir: pathlib.Path) -> dict:
    abi = run("adb", "-s", serial, "shell", "getprop", "ro.product.cpu.abi")
    target = TARGETS.get(abi)
    if not target:
        raise ValueError(f"unsupported device ABI: {abi}")
    run("cargo", "fmt", "--all", "--", "--check", cwd=repo)
    run("cargo", "check", "--workspace", "--all-targets", cwd=repo, env={**os.environ, "CARGO_TARGET_DIR": str(build_dir)})
    run("cargo", "test", "--workspace", "--all-targets", cwd=repo, env={**os.environ, "CARGO_TARGET_DIR": str(build_dir)})
    run(str(repo / "cross-compile.sh"), cwd=repo, env={
        **os.environ, "CARGO_TARGET_DIR": str(build_dir), "RUST_TARGET": target,
    })
    binary = build_dir / target / "release" / "nl2sh"
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    data = {"phase": "built", "serial": serial, "abi": abi, "target": target,
            "binary": str(binary), "sha256": digest, "candidate": None}
    save(state, data)
    return data


def deploy(state: pathlib.Path, candidate: str) -> dict:
    data = json.loads(state.read_text())
    if data.get("phase") != "built" or not data.get("serial"):
        raise ValueError("a successful build checkpoint is required")
    if not candidate.startswith("/data/local/tmp/nl2sh-a2a-") or not all(
        char.isalnum() or char in "/_-" for char in candidate
    ):
        raise ValueError("candidate must be a safe path under /data/local/tmp/nl2sh-a2a-")
    binary = pathlib.Path(data["binary"])
    if hashlib.sha256(binary.read_bytes()).hexdigest() != data["sha256"]:
        raise ValueError("candidate binary changed since build")
    serial = data["serial"]
    current_abi = run("adb", "-s", serial, "shell", "getprop", "ro.product.cpu.abi")
    if current_abi != data["abi"]:
        raise ValueError("device ABI changed since build")
    run("adb", "-s", serial, "push", str(binary), candidate)
    run("adb", "-s", serial, "shell", "chmod", "755", candidate)
    version = run("adb", "-s", serial, "exec-out", candidate, "--version")
    data.update(phase="deployed", candidate=candidate, device_version=version)
    save(state, data)
    return data


def main() -> None:
    parser = argparse.ArgumentParser(description="Explicit nl2sh A2A build/deploy checkpoints")
    sub = parser.add_subparsers(dest="command", required=True)
    build = sub.add_parser("prepare")
    build.add_argument("--repo", type=pathlib.Path, required=True)
    build.add_argument("--serial", required=True)
    build.add_argument("--state", type=pathlib.Path, required=True)
    build.add_argument("--build-dir", type=pathlib.Path, required=True)
    install = sub.add_parser("deploy")
    install.add_argument("--state", type=pathlib.Path, required=True)
    install.add_argument("--candidate", default="/data/local/tmp/nl2sh-a2a-candidate")
    status = sub.add_parser("status")
    status.add_argument("--state", type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        result = prepare(args.repo.resolve(), args.serial, args.state, args.build_dir.resolve())
    elif args.command == "deploy":
        result = deploy(args.state, args.candidate)
    else:
        result = json.loads(args.state.read_text())
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
