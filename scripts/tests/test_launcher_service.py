"""Keep every Android launcher on the native background service interface."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]

WEB_ONLY_LAUNCHERS = ('android-run-linux.sh', 'android-build-run.sh',
                      'android-run-windows.bat', 'android-build-run.ps1')
FORBIDDEN = ('nohup', 'kill -9', '/proc/[0-9]*', 'nl2sh-web.log')
# Per launcher: the guard that skips stopping a healthy service, and the call-site pattern.
STOP_GUARDS = {
    'android-run-linux.sh': ('if [[ "${WEB_ONLY}" == false ]]; then stop_existing_nl2sh',
                             'stop_existing_nl2sh '),
    'android-build-run.sh': ('if [[ "${WEB_ONLY}" == false ]]; then stop_existing_nl2sh',
                             'stop_existing_nl2sh '),
    'android-run-windows.bat': ('if not "!WEB_ONLY!"=="true" (',
                                'call :stop_existing_nl2sh '),
    'android-build-run.ps1': ('if (-not $WebOnly) { Stop-ExistingNl2sh',
                              'Stop-ExistingNl2sh $'),
}


class LauncherServiceTests(unittest.TestCase):
    def test_web_only_launchers_use_the_native_service_interface(self):
        for launcher in WEB_ONLY_LAUNCHERS:
            with self.subTest(launcher=launcher):
                source = (ROOT / launcher).read_text()
                self.assertIn('service start --json', source)
                self.assertIn('service stop --json', source)
                for line in source.splitlines():
                    if 'service start --json' in line or 'service stop --json' in line:
                        # Ownership follows the deployed configuration, not the launcher directory.
                        self.assertIn('--config', line)

    def test_launchers_never_detonate_or_scan_processes(self):
        for launcher in WEB_ONLY_LAUNCHERS:
            with self.subTest(launcher=launcher):
                source = (ROOT / launcher).read_text()
                for pattern in FORBIDDEN:
                    self.assertNotIn(pattern, source, f'{launcher} still contains {pattern!r}')

    def test_web_only_launchers_reuse_a_healthy_service(self):
        """`start` is idempotent, so Web-only mode must not stop the owned service first."""
        for launcher, (guard, call) in STOP_GUARDS.items():
            with self.subTest(launcher=launcher):
                source = (ROOT / launcher).read_text()
                # One call per privilege path: root adbd, Android su, and the adb shell user.
                self.assertEqual(source.count(call), 3, launcher)
                self.assertEqual(source.count(guard), 3, launcher)


if __name__ == '__main__':
    unittest.main()
