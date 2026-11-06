"""Exercise CI bootstrap without network access or changing the runner host."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'ensure_device_bot.sh'
BINARY = '#!/bin/sh\ncase "$1" in --version) echo 0.1.3;; --check-install) exit 0;; *) exit 9;; esac\n'


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.install = self.root / 'install dir'
        self.install.mkdir()
        self.marker = self.root / 'downloaded'
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith('DEVICE_BOT_')}
        self.env.update(PATH=f'{self.bin}:/usr/bin:/bin',
                        DEVICE_BOT_INSTALL_DIR=str(self.install),
                        DEVICE_BOT_VERSION='0.1.3')
        # Fake curl writes an installer to its --output argument.
        installer = ('#!/bin/sh\ncat > "$DEVICE_BOT_INSTALL_DIR/device-bot" <<\'BIN\'\n'
                     + BINARY + 'BIN\nchmod +x "$DEVICE_BOT_INSTALL_DIR/device-bot"\n')
        self.write(self.bin / 'curl', '#!/bin/sh\ntouch "' + str(self.marker)
                   + '"\nwhile [ "$1" != --output ]; do shift; done\nshift\n'
                   + "cat > \"$1\" <<'INSTALLER'\n" + installer + 'INSTALLER\n')

    def write(self, path, content):
        path.write_text(content)
        path.chmod(0o755)

    def run_bootstrap(self):
        return subprocess.run(['bash', '-c', 'source "$1"; "$DEVICE_BOT_BIN" --check-install',
                               'bootstrap', str(SCRIPT)], env=self.env,
                              capture_output=True, text=True)

    def test_healthy_install_skips_download(self):
        self.write(self.install / 'device-bot', BINARY)
        self.assertEqual(self.run_bootstrap().returncode, 0)
        self.assertFalse(self.marker.exists())

    def test_missing_binary_installs_and_exports_path(self):
        result = self.run_bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.marker.exists())

    def test_broken_assets_trigger_reinstall(self):
        self.write(self.install / 'device-bot', BINARY.replace('--check-install) exit 0', '--check-install) exit 1'))
        self.assertEqual(self.run_bootstrap().returncode, 0)
        self.assertTrue(self.marker.exists())

    def test_wrong_version_triggers_reinstall(self):
        self.write(self.install / 'device-bot', BINARY.replace('echo 0.1.3', 'echo 0.1.1'))
        self.assertEqual(self.run_bootstrap().returncode, 0)
        self.assertTrue(self.marker.exists())

    def test_explicit_override_is_respected(self):
        custom = self.root / 'custom bot'
        self.write(custom, BINARY)
        self.env['DEVICE_BOT_BIN'] = str(custom)
        self.assertEqual(self.run_bootstrap().returncode, 0)
        self.assertFalse(self.marker.exists())

    def test_missing_override_fails_without_replacing_it(self):
        self.env['DEVICE_BOT_BIN'] = str(self.root / 'missing')
        self.assertNotEqual(self.run_bootstrap().returncode, 0)
        self.assertFalse(self.marker.exists())

    def test_download_failure_stops_bootstrap(self):
        self.write(self.bin / 'curl', '#!/bin/sh\nexit 22\n')
        self.assertNotEqual(self.run_bootstrap().returncode, 0)

    def test_failed_installer_stops_bootstrap(self):
        self.write(self.bin / 'curl', '#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\nshift\nprintf "exit 1\\n" > "$1"\n')
        self.assertNotEqual(self.run_bootstrap().returncode, 0)

    def test_installer_success_without_valid_binary_still_fails(self):
        self.write(self.bin / 'curl', '#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\nshift\nprintf "exit 0\\n" > "$1"\n')
        self.assertNotEqual(self.run_bootstrap().returncode, 0)
