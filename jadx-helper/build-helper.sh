#!/usr/bin/env bash
set -euo pipefail

helper_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
gradle --project-dir "$helper_root" :app:assembleRelease

python3 - "$helper_root" <<'PY'
import hashlib
import json
from pathlib import Path
import shutil
import sys
import zipfile

root = Path(sys.argv[1])
apk = root / 'app/build/outputs/apk/release/app-release-unsigned.apk'
dist = root / 'dist'
dist.mkdir(exist_ok=True)
jar = dist / 'jadx-helper.jar'
shutil.copyfile(apk, jar)
with zipfile.ZipFile(jar) as archive:
    dex = archive.read('classes.dex')
    if not dex.startswith(b'dex\n') or b'Lcom/nl2sh/jadx/Main;' not in dex:
        raise SystemExit('helper APK is missing the DEX entrypoint')
    if any(name.startswith('classes2.dex') for name in archive.namelist()):
        raise SystemExit('multidex helper is unsupported by the app_process loader')
sha = hashlib.sha256(jar.read_bytes()).hexdigest()
(dist / 'jadx-helper.jar.sha256').write_text(f'{sha}  jadx-helper.jar\n')
(dist / 'metadata.json').write_text(json.dumps({
    'helper_version': '0.1.0',
    'jadx_version': '1.5.3',
    'min_android_api': 26,
    'entrypoint': 'com.nl2sh.jadx.Main',
    'sha256': sha,
    'size_bytes': jar.stat().st_size,
}, indent=2) + '\n')
print(jar)
print(sha)
PY
