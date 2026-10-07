#!/usr/bin/env python3
"""Generate exact release policy from packaged companion metadata and native assets."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib
import zipfile

ABI_MACHINE = {"arm64-v8a": 183, "armeabi-v7a": 40, "x86_64": 62}


def artifact(path, metadata, version, protocol, native_version):
    data = path.read_bytes()
    maximum = 32_000_000 if path.name.startswith("nl2sh-android-") and path.suffix != ".apk" else 64 * 1024 * 1024
    if not 0 < len(data) <= maximum:
        raise ValueError(f"invalid asset size: {path.name}")
    if metadata.get("protocol") != protocol or metadata.get("min_android_api") != 26:
        raise ValueError(f"unsupported component protocol/API: {path.name}")
    url = f"https://github.com/nl2sh/nl2sh/releases/download/v{native_version}/{path.name}"
    return dict(version=version, protocol=protocol, url=url, signature_url=url + ".sig",
                sha256=hashlib.sha256(data).hexdigest(), size_bytes=len(data),
                min_android_api=26, features=metadata.get("features", []))


def packaged_metadata(path):
    with zipfile.ZipFile(path) as archive:
        entry = archive.getinfo("assets/nl2sh-runtime.json")
        if entry.file_size > 16384:
            raise ValueError("component metadata exceeds limit")
        return json.loads(archive.read(entry))


def generate_extensions(args, policy, version):
    bridge = args.assets / "nl2sh-android-bridge.apk"
    jadx = args.assets / "jadx-helper.jar"
    bridge_meta = packaged_metadata(bridge)
    jadx_meta = packaged_metadata(jadx)
    if bridge_meta.get("app_version") != policy["android_bridge"] or bridge_meta.get("package_name") != "com.nl2sh.bridge":
        raise ValueError("Bridge version/package does not match component policy")
    if jadx_meta.get("helper_version") != policy["jadx_helper"] or "single_class" not in jadx_meta.get("features", []):
        raise ValueError("JADX version/features do not match component policy")
    with zipfile.ZipFile(jadx) as archive:
        dex_entry = archive.getinfo("classes.dex")
        if dex_entry.file_size > 64 * 1024 * 1024 or "classes2.dex" in archive.namelist():
            raise ValueError("JADX must contain a bounded single DEX")
        dex = archive.read(dex_entry)
        if not dex.startswith(b"dex\n") or b"Lcom/nl2sh/jadx/Main;" not in dex:
            raise ValueError("JADX entrypoint is missing")
    verified = subprocess.run([args.apksigner, "verify", "--verbose", "--print-certs", str(bridge)],
                              check=True, capture_output=True, text=True).stdout
    certificates = re.findall(r"Signer #\d+ certificate SHA-256 digest: ([0-9a-fA-F]{64})", verified)
    if len(certificates) != 1:
        raise ValueError("Bridge must have exactly one verified signing certificate")
    bridge_artifact = artifact(bridge, bridge_meta, policy["android_bridge"], 2, version)
    bridge_artifact.update(package_name="com.nl2sh.bridge", certificate_sha256=certificates[0].lower())
    return dict(schema=1, nl2sh=version, binaries={}, android_bridge=bridge_artifact,
                jadx_helper=artifact(jadx, jadx_meta, policy["jadx_helper"], 1, version),
                nl2sh_helper=dict(min_version=policy["nl2sh_helper_min"], service_protocol=1))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["extensions", "complete"])
    parser.add_argument("--assets", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--extensions", type=Path)
    parser.add_argument("--apksigner", default="apksigner")
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    if args.tag != "v" + version:
        raise ValueError("release tag must equal the Cargo package version")
    policy = json.loads((root / "packaging/runtime-components.json").read_text())
    if args.mode == "extensions":
        manifest = generate_extensions(args, policy, version)
    else:
        if args.extensions is None:
            parser.error("complete requires --extensions")
        manifest = json.loads(args.extensions.read_bytes())
        if manifest["nl2sh"] != version or manifest["schema"] != 1 or manifest["binaries"]:
            raise ValueError("invalid extension build policy")
        for abi, machine in ABI_MACHINE.items():
            path = args.assets / f"nl2sh-android-{abi}"
            header = path.read_bytes()[:64]
            if header[:4] != b"\x7fELF" or header[5] != 1 or int.from_bytes(header[18:20], "little") != machine:
                raise ValueError(f"native ELF ABI mismatch: {abi}")
            manifest["binaries"][abi] = artifact(path, {"protocol": 1, "min_android_api": 26}, version, 1, version)
        for name, item in [("nl2sh-android-bridge.apk", manifest["android_bridge"]), ("jadx-helper.jar", manifest["jadx_helper"])]:
            data = (args.assets / name).read_bytes()
            if len(data) != item["size_bytes"] or hashlib.sha256(data).hexdigest() != item["sha256"]:
                raise ValueError("component bytes changed after native build")
    args.output.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
