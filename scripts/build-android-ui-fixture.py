#!/usr/bin/env python3
"""Build the permission-free UI acceptance APK without changing the installed SDK."""
import argparse
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", type=Path, default=os.environ.get("ANDROID_HOME"))
    parser.add_argument("--build-tools", default="36.0.0")
    parser.add_argument("--platform", default="android-36")
    parser.add_argument("--output", type=Path, required=True, help="Private build directory outside source control")
    args = parser.parse_args()
    if args.sdk is None:
        parser.error("set ANDROID_HOME or pass --sdk")
    source = Path(__file__).resolve().parent / "fixtures/android-ui"
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True, mode=0o700)
    tools = args.sdk / "build-tools" / args.build_tools
    android = args.sdk / "platforms" / args.platform / "android.jar"
    classes = output / "classes"
    classes.mkdir(exist_ok=True)
    with (output / "build.log").open("w") as log:
        def run(*command):
            subprocess.run([str(arg) for arg in command], check=True, stdout=log, stderr=log)
        run("javac", "-source", "8", "-target", "8", "-classpath", android, "-d", classes, source / "FixtureActivity.java")
        run(tools / "d8", "--min-api", "26", "--lib", android, "--output", output / "dex.zip", *sorted(classes.rglob("*.class")))
        import zipfile
        with zipfile.ZipFile(output / "dex.zip") as archive:
            (output / "classes.dex").write_bytes(archive.read("classes.dex"))
        run(tools / "aapt", "package", "-f", "-M", source / "AndroidManifest.xml", "-I", android, "-F", output / "unsigned.apk")
        subprocess.run([str(tools / "aapt"), "add", "unsigned.apk", "classes.dex"], cwd=output, check=True, stdout=log, stderr=log)
        run(tools / "zipalign", "-f", "4", output / "unsigned.apk", output / "aligned.apk")
        key = output / "fixture.keystore"
        if not key.exists():
            run("keytool", "-genkeypair", "-keystore", key, "-alias", "fixture", "-storepass", "android", "-keypass", "android",
                "-keyalg", "RSA", "-keysize", "2048", "-validity", "30", "-dname", "CN=Disposable acceptance fixture")
            key.chmod(0o600)
        apk = output / "fixture.apk"
        run(tools / "apksigner", "sign", "--ks", key, "--ks-key-alias", "fixture", "--ks-pass", "pass:android", "--out", apk, output / "aligned.apk")
        run(tools / "apksigner", "verify", apk)
    print(apk)


if __name__ == "__main__":
    main()
