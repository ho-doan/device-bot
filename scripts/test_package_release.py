"""Release archive/installer contract, using disposable fixture executables."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SCRIPTS = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('package_release', SCRIPTS / 'package_release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class NativeRelease(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / 'repo'
        self.bins = self.root / 'binaries'
        self.bins.mkdir()
        self.out = self.root / 'dist'
        self.setup_version('0.1.4')

    def setup_version(self, version):
        self.version = version
        for tool, crate in [('device-bot', 'mcp-server'), ('robot-flow', 'robot-flow'), ('robot-report', 'robot-report')]:
            path = self.repo / f'server/crates/{crate}/Cargo.toml'
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f'[package]\nname = "{tool}"\nversion = "{version}"\n')
            for suffix in ['', '.exe']:
                binary = self.bins / (tool + suffix)
                binary.write_text(f'''#!/bin/sh
case "$1" in
--version) echo {version} ;;
--check-install) test -f "$(dirname "$0")/device-bot-assets/{version}/manifest.json" ;;
*) exit 1 ;;
esac
''')
                binary.chmod(0o755)
        for platform in ['ios-sim', 'ios-real']:
            path = self.repo / f'server/crates/robot-{platform}/xctest-runner/Runner.swift'
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('// tracked XCTest fixture\n')
        deploy = self.repo / 'server/crates/robot-report/deploy'
        deploy.mkdir(exist_ok=True)
        (deploy / 'README.md').write_text('Production deployment guide\n')

    def package(self, target='aarch64-apple-darwin', qc=None):
        suffix = '.exe' if 'windows' in target else ''
        def tracked(args, **_):
            return args[-1] + 'Runner.swift\n'
        with patch.object(release, 'ROOT', self.repo), patch.object(release.subprocess, 'check_output', tracked), contextlib.redirect_stdout(io.StringIO()):
            return release.package(self.bins / ('device-bot' + suffix), target, self.version, self.out, qc)

    def test_mac_bundle_contains_all_tools_and_versioned_assets(self):
        archive = self.package()
        with tarfile.open(archive) as bundle:
            for name in ['device-bot', 'robot-flow', 'robot-report', 'bundle.json',
                         'robot-report-assets/0.1.4/deploy/README.md',
                         'device-bot-assets/0.1.4/manifest.json']:
                self.assertIn(name, bundle.getnames())
            manifest = json.load(bundle.extractfile('bundle.json'))
            self.assertEqual(manifest['binaries'], ['device-bot', 'robot-flow', 'robot-report'])

    def test_windows_bundle_contains_all_executables(self):
        with zipfile.ZipFile(self.package('x86_64-pc-windows-msvc')) as bundle:
            for name in ['device-bot.exe', 'robot-flow.exe', 'robot-report.exe', 'bundle.json']:
                self.assertIn(name, bundle.namelist())

    def test_release_archive_includes_the_qc_bundle(self):
        app = self.root / 'mobile_mcp_companion.app'
        (app / 'Contents').mkdir(parents=True)
        (app / 'Contents' / 'Info.plist').write_text('qc')
        with tarfile.open(self.package(qc=app)) as bundle:
            self.assertIn('qc/mobile_mcp_companion.app/Contents/Info.plist', bundle.getnames())
        windows = self.root / 'Release'
        (windows / 'data').mkdir(parents=True)
        (windows / 'mobile_mcp_companion.exe').write_bytes(b'MZ')
        (windows / 'data' / 'icudtl.dat').write_bytes(b'icu')
        with zipfile.ZipFile(self.package('x86_64-pc-windows-msvc', windows)) as bundle:
            self.assertIn('qc/mobile_mcp_companion.exe', bundle.namelist())
            self.assertIn('qc/data/icudtl.dat', bundle.namelist())

    def test_missing_companion_binary_and_mismatched_version_fail_packaging(self):
        (self.bins / 'robot-report').unlink()
        with self.assertRaisesRegex(ValueError, 'Missing release binary'):
            self.package()
        self.setup_version('0.1.4')
        (self.repo / 'server/crates/robot-flow/Cargo.toml').write_text('version = "0.1.0"\n')
        with self.assertRaisesRegex(ValueError, 'does not match tag'):
            self.package()

    def drop_members(self, archive, names):
        replacement = archive.with_suffix('.replacement')
        with tarfile.open(archive) as source, tarfile.open(replacement, 'w:gz') as destination:
            for member in source:
                if member.name not in names:
                    destination.addfile(member, source.extractfile(member) if member.isfile() else None)
        replacement.replace(archive)

    def install(self, archive):
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        (self.out / 'checksums.txt').write_text(f'{digest}  {archive.name}\n')
        shims = self.root / 'shims'
        shims.mkdir(exist_ok=True)
        (shims / 'uname').write_text('#!/bin/sh\ncase "$1" in -s) echo Darwin;; -m) echo arm64;; esac\n')
        (shims / 'curl').write_text('''#!/bin/sh
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = --output ]; then shift; output="$1"; else url="$1"; fi
  shift
done
cp "$TEST_DOWNLOAD_ROOT/${url##*/}" "$output"
''')
        for path in shims.iterdir():
            path.chmod(0o755)
        self.install_dir = self.root / 'installed'
        env = dict(os.environ, PATH=str(shims) + os.pathsep + os.environ['PATH'],
                   DEVICE_BOT_VERSION=self.version, DEVICE_BOT_INSTALL_DIR=str(self.install_dir),
                   TEST_DOWNLOAD_ROOT=str(self.out))
        return subprocess.run(['sh', str(SCRIPTS / 'install.sh')], env=env, capture_output=True, text=True)

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_installer_installs_three_verified_tools(self):
        result = self.install(self.package())
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for name in ['device-bot', 'robot-flow', 'robot-report']:
            version = subprocess.check_output([str(self.install_dir / name), '--version'], text=True)
            self.assertEqual(version.strip(), '0.1.4')
        self.assertTrue((self.install_dir / 'robot-report-assets/0.1.4/deploy/README.md').is_file())

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_installer_installs_qc_next_to_the_cli(self):
        app = self.root / 'mobile_mcp_companion.app'
        (app / 'Contents').mkdir(parents=True)
        (app / 'Contents' / 'Info.plist').write_text('qc')
        result = self.install(self.package(qc=app))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.install_dir / 'qc/mobile_mcp_companion.app/Contents/Info.plist').is_file())
        self.assertIn('Installed QC', result.stdout)

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_installer_rejects_incomplete_new_bundle_before_install(self):
        archive = self.package()
        self.drop_members(archive, {'robot-report'})
        result = self.install(archive)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('does not contain robot-report', result.stderr)
        self.assertFalse(self.install_dir.exists())

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_installer_keeps_support_for_legacy_device_bot_only_archives(self):
        self.setup_version('0.1.3')
        archive = self.package()
        self.drop_members(archive, {'robot-report', 'robot-flow'})
        result = self.install(archive)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.install_dir / 'device-bot').is_file())


if __name__ == '__main__':
    unittest.main()
