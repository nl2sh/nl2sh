"""Run Bash launcher device selection against mocked ADB output."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class AdbDeviceTests(unittest.TestCase):
    def test_bash_device_selection(self):
        for launcher in ('android-build-run.sh', 'android-run-linux.sh',
                         'android-build-tmux-run.sh'):
            source = (ROOT / launcher).read_text()
            functions = '\n'.join(re.search(
                rf'^{name}\(\) \{{.*?^\}}', source, re.M | re.S).group()
                for name in ('collect_devices', 'select_device'))
            for serials in (['usb-123'], ['192.0.2.1:5555'],
                            ['adb-test (3)._adb-tls-connect._tcp'],
                            ['usb-123', 'adb-test (3)._adb-tls-connect._tcp']):
                with self.subTest(launcher=launcher, serials=serials), tempfile.TemporaryDirectory() as tmp:
                    fixture = Path(tmp) / 'devices'
                    fixture.write_text('List of devices attached\r\n'
                                       'offline-usb\toffline\r\nlocked-usb\tunauthorized\r\n' +
                                       ''.join(f'{serial}\tdevice\r\n' for serial in serials))
                    env = {**os.environ, 'DEVICE_FIXTURE': str(fixture)}
                    env.pop('ADB_SERIAL', None)
                    script = '''set -euo pipefail
die() { echo "$*" >&2; exit 1; }
adb() {
  if [[ "$1" == devices ]]; then cat "$DEVICE_FIXTURE";
  elif [[ "$1" == -s && "$3" == shell ]]; then echo package:/mock/termux.apk;
  else exit 98; fi
}
TERMUX_PACKAGE=com.termux
''' + functions + '\nselect_device\nprintf "SELECTED=%s\\n" "$SELECTED_SERIAL"\n'
                    result = subprocess.run(['bash', '-c', script], env=env,
                                            input='2\n' if len(serials) > 1 else '',
                                            capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(f'SELECTED={serials[-1]}\n', result.stdout)
                    self.assertNotIn('No connected ADB device', result.stdout)


if __name__ == '__main__':
    unittest.main()
