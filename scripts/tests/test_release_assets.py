"""Reproduce release uploads with the full downloaded-artifact directory layout."""
from collections import Counter
from pathlib import Path
import re
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ReleaseAssetTests(unittest.TestCase):
    def test_release_uploads_each_asset_once(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        block = re.search(r'^          files: \|\n((?:            .+\n)+)', workflow, re.M)
        self.assertIsNotNone(block)
        patterns = [line.strip() for line in block[1].splitlines()]
        signed = ['nl2sh-runtime.json', 'nl2sh-runtime.json.sig',
                  'nl2sh-runtime-extensions.json', 'nl2sh-runtime-extensions.json.sig',
                  'nl2sh-android-bridge.apk', 'nl2sh-android-bridge.apk.sig',
                  'jadx-helper.jar', 'jadx-helper.jar.sig']
        layout = {
            'release-package': ['nl2sh-android.tar.gz', 'nl2sh-android.zip', 'SHA256SUMS'],
            'termux-debs': [f'nl2sh_1.2.0_{abi}.deb' for abi in ['arm', 'aarch64', 'x86_64']],
            'termux-apt-repository': ['termux-apt-repository.tar.gz'],
            'runtime-extensions': signed.copy(),
        }
        for target, abi in [('aarch64-linux-android', 'arm64-v8a'),
                            ('armv7-linux-androideabi', 'armeabi-v7a'),
                            ('x86_64-linux-android', 'x86_64')]:
            name = f'nl2sh-android-{abi}'
            layout[f'update-{target}'] = [name, name + '.sha256']
            layout[f'binary-{target}'] = ['nl2sh']
            layout[f'termux-{target}'] = ['nl2sh']
            signed.extend([name, name + '.sha256', name + '.sig'])
        layout['signed-runtime'] = signed
        expected = set(signed + layout['release-package'] + layout['termux-debs']
                       + layout['termux-apt-repository'])
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for directory, names in layout.items():
                folder = root / 'artifacts' / directory
                folder.mkdir(parents=True)
                for name in names:
                    (folder / name).touch()
            matches = [path for pattern in patterns for path in root.glob(pattern)]
            counts = Counter(path.name for path in matches)
            self.assertEqual(set(counts), expected)
            self.assertEqual({name: n for name, n in counts.items() if n != 1}, {})
            self.assertEqual(len(matches), 24)
