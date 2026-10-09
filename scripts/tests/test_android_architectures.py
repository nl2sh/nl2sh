"""Exercise Android ABI selection and combined packaging without a device/NDK."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]


class AndroidArchitectureTests(unittest.TestCase):
    def test_launcher_prefers_native_x86_64(self):
        for abis, expected in [('x86_64,x86,arm64-v8a,armeabi-v7a', 'x86_64'),
                               ('arm64-v8a,armeabi-v7a', 'arm64-v8a'),
                               ('armeabi-v7a', 'armeabi-v7a')]:
            with self.subTest(abis=abis), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                shutil.copy(ROOT / 'android-run-linux.sh', root)
                shutil.copy(ROOT / 'android-build-run.sh', root)
                binary = root / 'bin' / expected / 'nl2sh'
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b'test binary')
                adb = root / 'adb'
                adb.write_text('''#!/usr/bin/env bash
case "$3" in
  get-state) echo device ;;
  shell) echo "$TEST_ABIS" ;;
  root) exit 0 ;;
  wait-for-device) exit 97 ;;
  *) exit 98 ;;
esac
''')
                adb.chmod(0o755)
                result = subprocess.run(['bash', str(root / 'android-run-linux.sh')],
                                        env={**os.environ, 'PATH': f'{root}:{os.environ["PATH"]}',
                                             'ADB_SERIAL': 'mock', 'TEST_ABIS': abis},
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 97, result.stderr)
                self.assertIn(f'Selected binary: {expected}', result.stdout)
                env = {**os.environ, 'PATH': f'{root}:{os.environ["PATH"]}',
                       'ADB_SERIAL': 'mock', 'TEST_ABIS': abis}
                env.pop('RUST_TARGET', None)
                target = {'x86_64': 'x86_64-linux-android', 'arm64-v8a': 'aarch64-linux-android',
                          'armeabi-v7a': 'armv7-linux-androideabi'}[expected]
                build = subprocess.run(['bash', str(root / 'android-build-run.sh')],
                                       env=env, capture_output=True, text=True)
                self.assertEqual(build.returncode, 97, build.stderr)
                self.assertIn(f'Selected Rust target: {target}', build.stdout)
                env['RUST_TARGET'] = ('aarch64-linux-android' if expected == 'x86_64'
                                      else 'x86_64-linux-android')
                mismatch = subprocess.run(['bash', str(root / 'android-build-run.sh')],
                                          env=env, capture_output=True, text=True)
                self.assertEqual(mismatch.returncode, 1)
                self.assertIn('does not match device ABI', mismatch.stderr)


    def test_combined_archive_contains_all_android_abis(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in ['pack-release.sh', 'cross-compile.sh', 'android-run-linux.sh',
                         'android-run-windows.bat', 'install-android.sh', 'install-android.bat',
                         'install-android.ps1', 'config.toml.example', 'README.md', 'README_EN.md',
                         'LICENSE', 'AGENTS.md']:
                shutil.copy(ROOT / name, root)
            (root / 'docs').mkdir()
            (root / 'docs' / 'test.md').write_text('test docs')
            tools = root / 'tools'
            tools.mkdir()
            rustup = tools / 'rustup'
            rustup.write_text('#!/bin/sh\nprintf "%s\\n" aarch64-linux-android armv7-linux-androideabi x86_64-linux-android\n')
            cargo = tools / 'cargo'
            cargo.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
target = sys.argv[sys.argv.index('--target') + 1]
prefix = target.upper().replace('-', '_')
suffix = target.replace('-', '_')
with open('builds.jsonl', 'a') as log:
    log.write(json.dumps({'target': target, 'linker': os.environ['CARGO_TARGET_' + prefix + '_LINKER'],
                          'cc': os.environ['CC_' + suffix], 'ar': os.environ['AR_' + suffix]}) + '\\n')
binary = pathlib.Path('target') / target / 'release/nl2sh'
binary.parent.mkdir(parents=True, exist_ok=True)
binary.write_text(target)
''')
            for tool in [rustup, cargo]:
                tool.chmod(0o755)
            ndk = root / 'ndk/toolchains/llvm/prebuilt/linux-x86_64/bin'
            ndk.mkdir(parents=True)
            for name in ['aarch64-linux-android26-clang', 'armv7a-linux-androideabi26-clang',
                         'x86_64-linux-android26-clang', 'llvm-ar']:
                path = ndk / name
                path.write_text('#!/bin/sh\nexit 0\n')
                path.chmod(0o755)
            subprocess.run(['bash', str(root / 'pack-release.sh')], cwd=root,
                           env={**os.environ, 'PATH': f'{tools}:{os.environ["PATH"]}',
                                'ANDROID_NDK_HOME': str(root / 'ndk'), 'ANDROID_API_LEVEL': '26'},
                           check=True, capture_output=True)
            with zipfile.ZipFile(root / 'dist/nl2sh-android.zip') as archive:
                for abi, target in [('arm64-v8a', 'aarch64-linux-android'),
                                    ('armeabi-v7a', 'armv7-linux-androideabi'),
                                    ('x86_64', 'x86_64-linux-android')]:
                    self.assertEqual(archive.read(f'nl2sh-android/bin/{abi}/nl2sh').decode(), target)
            builds = [json.loads(line) for line in (root / 'builds.jsonl').read_text().splitlines()]
            self.assertEqual(len(builds), 3)
            x86 = next(build for build in builds if build['target'] == 'x86_64-linux-android')
            self.assertEqual(x86['cc'], str(ndk / 'x86_64-linux-android26-clang'))
            self.assertEqual(x86['linker'], x86['cc'])
            self.assertEqual(x86['ar'], str(ndk / 'llvm-ar'))


if __name__ == '__main__':
    unittest.main()
