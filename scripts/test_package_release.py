"""Release archive/installer contract, using disposable fixture executables."""
import contextlib
import hashlib
import functools
import http.server
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import threading
from unittest.mock import patch
import zipfile

SCRIPTS = Path(__file__).resolve().parent
PWSH = os.environ.get('DEVICE_BOT_TEST_PWSH') or shutil.which('pwsh') or shutil.which('powershell')
spec = importlib.util.spec_from_file_location('package_release', SCRIPTS / 'package_release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class NativeRelease(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
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
--check-install) test -f "$(dirname "$0")/device-bot-assets/{version}/manifest.json" && test "$TEST_FAIL_INSTALLED_CHECK" != 1 ;;
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

    def package(self, target='aarch64-apple-darwin', qc=None, qc_web=None):
        suffix = '.exe' if 'windows' in target else ''
        def tracked(args, **_):
            return args[-1] + 'Runner.swift\n'
        with patch.object(release, 'ROOT', self.repo), patch.object(release.subprocess, 'check_output', tracked), contextlib.redirect_stdout(io.StringIO()):
            return release.package(self.bins / ('device-bot' + suffix), target, self.version, self.out, qc, qc_web)

    def test_release_archives_include_the_web_runtime_beside_the_cli(self):
        web = self.root / 'web'
        web.mkdir()
        for name in ['index.html', 'flutter_bootstrap.js', 'main.dart.js']:
            (web / name).write_text('fixture')
        for target in ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc']:
            archive = self.package(target, qc_web=web)
            if archive.suffix == '.zip':
                with zipfile.ZipFile(archive) as bundle:
                    names = bundle.namelist()
            else:
                with tarfile.open(archive) as bundle:
                    names = bundle.getnames()
            self.assertIn('qc-web/index.html', names)
            self.assertIn('qc-web/flutter_bootstrap.js', names)
            self.assertIn('qc-web/main.dart.js', names)
        (web / 'flutter_bootstrap.js').unlink()
        with self.assertRaisesRegex(ValueError, 'Missing QC web build'):
            self.package(qc_web=web)

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

    def scrcpy_bundle(self, arch='aarch64', version='5.0.1', missing=(), exit_code=0):
        bundle = Path(tempfile.mkdtemp(dir=self.root)) / f'scrcpy-macos-{arch}-v5.0.1'
        bundle.mkdir(exist_ok=True)
        contents = {
            'scrcpy': f'#!/bin/sh\necho "scrcpy {version} <https://github.com/Genymobile/scrcpy>"\nexit {exit_code}\n',
            'adb': '#!/bin/sh\necho "Android Debug Bridge version 1.0.41"\n',
            'scrcpy-server': 'matching server jar',
            'scrcpy.png': 'portable icon',
        }
        for name, content in contents.items():
            path = bundle / name
            if name not in missing:
                path.write_text(content)
                path.chmod(0o755 if name in ('scrcpy', 'adb') else 0o644)
        archive = self.out / (bundle.name + '.tar.gz')
        with tarfile.open(archive, 'w:gz') as output:
            output.add(bundle, arcname=bundle.name)
        return archive

    def install(self, archive, scrcpy=None, bad_scrcpy_checksum=False, arch='arm64', ffmpeg_x264=True):
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        (self.out / 'checksums.txt').write_text(f'{digest}  {archive.name}\n')
        shims = self.root / 'shims'
        shims.mkdir(exist_ok=True)
        (shims / 'uname').write_text(f'#!/bin/sh\ncase "$1" in -s) echo Darwin;; -m) echo {arch};; esac\n')
        (shims / 'ffmpeg').write_text('#!/bin/sh\necho "' + ('V..... libx264' if ffmpeg_x264 else 'V..... mpeg4') + '"\n')
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
        scrcpy = scrcpy or self.scrcpy_bundle('aarch64' if arch == 'arm64' else 'x86_64')
        # Pin fixture checksums in a disposable installer copy. Production pins
        # stay unchanged and no download/install touches the host machine.
        installer = (SCRIPTS / 'install.sh').read_text()
        digest = '0' * 64 if bad_scrcpy_checksum else hashlib.sha256(scrcpy.read_bytes()).hexdigest()
        installer = re.sub(r'SCRCPY_SHA256="[0-9a-f]{64}"', f'SCRCPY_SHA256="{digest}"', installer)
        installer_path = self.root / 'install.sh'
        installer_path.write_text(installer)
        self.install_dir = self.root / 'installed'
        env = dict(os.environ, PATH=str(shims) + os.pathsep + os.environ['PATH'],
                   DEVICE_BOT_VERSION=self.version, DEVICE_BOT_INSTALL_DIR=str(self.install_dir),
                   DEVICE_BOT_FFMPEG_PATH=str(shims / 'ffmpeg'),
                   TEST_DOWNLOAD_ROOT=str(self.out))
        return subprocess.run(['sh', str(installer_path)], env=env, capture_output=True, text=True)

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_installer_copies_the_qc_web_runtime(self):
        web = self.root / 'web'
        web.mkdir()
        for name in ['index.html', 'flutter_bootstrap.js', 'main.dart.js']:
            (web / name).write_text('fixture')
        result = self.install(self.package(qc_web=web))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.install_dir / 'qc-web/index.html').is_file())
        self.assertTrue((self.install_dir / 'qc-web/main.dart.js').is_file())

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_installer_installs_three_verified_tools(self):
        result = self.install(self.package())
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for name in ['device-bot', 'robot-flow', 'robot-report']:
            version = subprocess.check_output([str(self.install_dir / name), '--version'], text=True)
            self.assertEqual(version.strip(), '0.1.4')
        self.assertTrue((self.install_dir / 'robot-report-assets/0.1.4/deploy/README.md').is_file())
        self.assertTrue((self.install_dir / 'scrcpy-server').is_file())
        # Exercise the driver's real discovery contract from a different cwd.
        env = dict(os.environ, PATH=str(self.install_dir) + os.pathsep + os.environ['PATH'])
        version = subprocess.check_output(['scrcpy', '--version'], cwd=self.root, env=env, text=True)
        self.assertTrue(version.startswith('scrcpy 5.0.1 '))
        subprocess.run(['adb', 'version'], cwd=self.root, env=env, check=True, capture_output=True)
        self.assertIn('Installed Android Live dependencies', result.stdout)
        self.assertTrue((self.install_dir / 'ffmpeg/bin/ffmpeg').is_file())

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_installer_rejects_ffmpeg_without_libx264(self):
        result = self.install(self.package(), ffmpeg_x264=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('FFmpeg with libx264', result.stderr)
        self.assertFalse(self.install_dir.exists())

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_installer_rejects_bad_scrcpy_checksum_before_install(self):
        result = self.install(self.package(), bad_scrcpy_checksum=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('scrcpy checksum verification failed', result.stderr)
        self.assertFalse(self.install_dir.exists())

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_installer_rejects_missing_or_incompatible_scrcpy(self):
        archive = self.package()
        for options, error in [({'missing': ('scrcpy-server',)}, 'does not contain scrcpy-server'),
                               ({'version': '9.0.0'}, 'scrcpy version verification failed'),
                               ({'exit_code': 1}, 'scrcpy version verification failed')]:
            with self.subTest(options=options):
                result = self.install(archive, scrcpy=self.scrcpy_bundle(**options))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(error, result.stderr)
                self.assertFalse(self.install_dir.exists())

    @unittest.skipUnless(os.name == 'posix', 'POSIX installer')
    def test_mac_intel_installer_includes_matching_scrcpy(self):
        result = self.install(self.package('x86_64-apple-darwin'), arch='x86_64')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.install_dir / 'scrcpy-server').is_file())

    def install_windows(self, missing=(), bad_checksum=False, version='5.0.1', empty_architecture=False,
                        running_processes=False, bad_ffmpeg_checksum=False, ffmpeg_x264=True,
                        upgrade_caller=False, fail_installed_check=False):
        if os.name == 'nt':
            # Build one tiny native fixture for all CLI names on Windows CI.
            source = self.root / 'Fixture.cs'
            source.write_text('''using System;
using System.IO;
class Fixture {
    static int Main(string[] args) {
        if (args.Length > 0 && args[0] == "--hold") { System.Threading.Thread.Sleep(120000); return 0; }
        string name = Path.GetFileName(Environment.GetCommandLineArgs()[0]);
        if (name == "ffmpeg.exe") Console.WriteLine(args.Length > 1 ? Environment.GetEnvironmentVariable("TEST_FFMPEG_ENCODERS") : "ffmpeg version " + Environment.GetEnvironmentVariable("TEST_FFMPEG_VERSION") + "-fixture");
        else if (name == "scrcpy.exe") Console.WriteLine("scrcpy " + Environment.GetEnvironmentVariable("TEST_SCRCPY_VERSION"));
        else if (name == "adb.exe") Console.WriteLine("Android Debug Bridge version 1.0.41");
        else if (args.Length > 0 && args[0] == "--version") Console.WriteLine("0.1.4");
        else if (args.Length > 0 && args[0] == "--check-install" && Environment.GetEnvironmentVariable("TEST_FAIL_INSTALLED_CHECK") == "1") return 4;
        else if (args.Length == 0 || args[0] != "--check-install") return 1;
        return 0;
    }
}
''')
            compiler = Path(os.environ['WINDIR']) / 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
            fixture = self.root / 'fixture.exe'
            subprocess.run([str(compiler), '/nologo', '/out:' + str(fixture), str(source)], check=True, capture_output=True)
            for name in ['device-bot', 'robot-flow', 'robot-report']:
                shutil.copyfile(fixture, self.bins / (name + '.exe'))
            scrcpy_exe = adb_exe = fixture.read_bytes()
        else:
            scrcpy_exe = f'#!/bin/sh\necho "scrcpy {version} <https://github.com/Genymobile/scrcpy>"\n'.encode()
            adb_exe = b'#!/bin/sh\necho "Android Debug Bridge version 1.0.41"\n'
        archive = self.package('x86_64-pc-windows-msvc')
        (self.out / 'checksums.txt').write_text(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n')
        dependencies = {'scrcpy.exe': scrcpy_exe, 'adb.exe': adb_exe, 'scrcpy-server': b'matching server jar'}
        for name in ['AdbWinApi.dll', 'AdbWinUsbApi.dll', 'SDL3.dll', 'avcodec-63.dll',
                     'avformat-63.dll', 'avutil-61.dll', 'swresample-7.dll', 'libusb-1.0.dll']:
            dependencies[name] = b'runtime fixture'
        scrcpy = self.out / 'scrcpy-win64-v5.0.1.zip'
        with zipfile.ZipFile(scrcpy, 'w') as output:
            for name, content in dependencies.items():
                if name not in missing:
                    output.writestr('scrcpy-win64-v5.0.1/' + name, content)
        digest = '0' * 64 if bad_checksum else hashlib.sha256(scrcpy.read_bytes()).hexdigest()
        installer = (SCRIPTS / 'install.ps1').read_text()
        ffmpeg_version = re.search(r'\$FfmpegVersion = "([^"]+)"', installer)[1]
        ffmpeg_bundle = f'ffmpeg-{ffmpeg_version}-essentials_build'
        ffmpeg_archive = self.out / f'{ffmpeg_bundle}.zip'
        ffmpeg_exe = fixture.read_bytes() if os.name == 'nt' else (
            '#!/bin/sh\nif [ "$1" = "-version" ]; then echo "ffmpeg version $TEST_FFMPEG_VERSION-fixture"; '
            'else echo "$TEST_FFMPEG_ENCODERS"; fi\n').encode()
        with zipfile.ZipFile(ffmpeg_archive, 'w') as output:
            output.writestr(f'{ffmpeg_bundle}/bin/ffmpeg.exe', ffmpeg_exe)
            output.writestr(f'{ffmpeg_bundle}/LICENSE', 'FFmpeg fixture license')
        ffmpeg_digest = '0' * 64 if bad_ffmpeg_checksum else hashlib.sha256(ffmpeg_archive.read_bytes()).hexdigest()

        class QuietHandler(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *_):
                pass
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(QuietHandler, directory=str(self.out)))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        base = f'http://127.0.0.1:{server.server_port}'
        # Exercise the real installer with local HTTP archives, pinning only the
        # fixture checksum. Restore persistent PATH even when installation fails.
        installer = re.sub(r'\$ScrcpySha256 = "[0-9a-f]{64}"', f'$ScrcpySha256 = "{digest}"', installer)
        installer = re.sub(r'\$FfmpegSha256 = "[0-9a-f]{64}"', f'$FfmpegSha256 = "{ffmpeg_digest}"', installer)
        installer = installer.replace('https://github.com/$Repository/releases/download/v$Version', base)
        installer = installer.replace('https://github.com/Genymobile/scrcpy/releases/download/v$ScrcpyVersion', base)
        installer = installer.replace('https://github.com/GyanD/codexffmpeg/releases/download/$FfmpegVersion', base)
        installer_path = self.root / 'install.ps1'
        installer_path.write_text(installer)
        bootstrap = ''
        if os.name != 'nt':
            # PowerShell ZIP extraction drops POSIX executable permissions.
            bootstrap = '''Import-Module Microsoft.PowerShell.Archive
function Expand-Archive {
param($Path, $DestinationPath, [switch]$Force)
Microsoft.PowerShell.Archive\\Expand-Archive @PSBoundParameters
Get-ChildItem $DestinationPath -Recurse -Filter *.exe | ForEach-Object { & /bin/chmod +x $_.FullName }
}
'''
        quoted = str(installer_path).replace("'", "''")
        bootstrap += f'''$savedPath = [Environment]::GetEnvironmentVariable("Path", "User")
try {{ & '{quoted}' }} finally {{ [Environment]::SetEnvironmentVariable("Path", $savedPath, "User") }}
'''
        self.install_dir = self.root / 'installed windows'
        if running_processes:
            if os.name != 'nt':
                # Compile a disposable native process: copying Apple's signed
                # /bin/sleep can be rejected by macOS code-signing enforcement.
                source = self.root / 'hold.c'
                source.write_text('#include <unistd.h>\nint main(void) { sleep(120); return 0; }\n')
                fixture = self.root / 'hold'
                subprocess.run(['cc', str(source), '-o', str(fixture)], check=True, capture_output=True)
            # Real executable paths, including a QC-bundled backend and a
            # similarly named sibling install that must remain untouched.
            self.old_processes = []
            for relative in ['device-bot.exe', 'robot-report.exe', 'scrcpy.exe',
                             'adb.exe', 'qc/device-bot.exe', 'qc/companion.exe', 'ffmpeg/bin/ffmpeg.exe']:
                path = self.install_dir / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(fixture, path)
                process = subprocess.Popen([str(path), '--hold' if os.name == 'nt' else '120'])
                self.old_processes.append(process)
                self.addCleanup(self.stop_fixture, process)
                threading.Thread(target=process.wait, daemon=True).start()
            outside = self.root / 'installed windows-other' / 'device-bot.exe'
            outside.parent.mkdir()
            shutil.copy(fixture, outside)
            self.other_process = subprocess.Popen([str(outside), '--hold' if os.name == 'nt' else '120'])
            self.addCleanup(self.stop_fixture, self.other_process)
        env = {key: value for key, value in os.environ.items() if not key.startswith('DEVICE_BOT_')}
        env.update(DEVICE_BOT_VERSION=self.version, DEVICE_BOT_INSTALL_DIR=str(self.install_dir),
                   PROCESSOR_ARCHITECTURE='' if empty_architecture else 'AMD64', PROCESSOR_ARCHITEW6432='', TEST_SCRCPY_VERSION=version,
                   TEST_FFMPEG_VERSION=ffmpeg_version, TEST_FFMPEG_ENCODERS='V..... libx264' if ffmpeg_x264 else 'V..... mpeg4')
        if upgrade_caller:
            caller = self.other_process if upgrade_caller == 'unrelated' else self.old_processes[0]
            env['DEVICE_BOT_UPGRADE_PID'] = str(caller.pid)
            env['DEVICE_BOT_UPGRADE_LOG'] = str(self.root / 'upgrade.log')
        if fail_installed_check:
            env['TEST_FAIL_INSTALLED_CHECK'] = '1'
        return subprocess.run([PWSH, '-NoProfile', '-NonInteractive', '-Command', bootstrap], env=env, capture_output=True, text=True)

    @staticmethod
    def stop_fixture(process):
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=10)

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_upgrade_stops_only_processes_from_the_replaced_installation(self):
        result = self.install_windows(running_processes=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for process in self.old_processes:
            process.wait(timeout=10)
            self.assertIsNotNone(process.returncode)
        self.assertIsNone(self.other_process.poll())
        self.assertIn('Stopping installed Device Bot process', result.stdout)
        self.assertTrue((self.install_dir / 'device-bot.exe').is_file())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_foreground_upgrade_preserves_waiting_caller_and_reports_result(self):
        result = self.install_windows(running_processes=True, upgrade_caller=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIsNone(self.old_processes[0].poll(), 'installer killed the CLI waiting for its result')
        for process in self.old_processes[1:]:
            process.wait(timeout=10)
        self.assertIsNone(self.other_process.poll())
        self.assertTrue((self.install_dir / 'device-bot.exe').is_file())
        self.assertTrue((self.install_dir / 'device-bot.exe.previous').is_file())
        self.assertIn('Installed device-bot.exe', result.stdout)
        self.assertIn('Installed device-bot.exe', (self.root / 'upgrade.log').read_text(encoding='utf-8-sig'))

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_foreground_upgrade_restores_executable_when_install_verification_fails(self):
        result = self.install_windows(running_processes=True, upgrade_caller=True, fail_installed_check=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Installed Device Bot verification failed', result.stderr)
        self.assertIn('Restored the previous Device Bot executable', result.stdout)
        self.assertIn('Installed Device Bot verification failed', (self.root / 'upgrade.log').read_text(encoding='utf-8-sig'))
        self.assertIsNone(self.old_processes[0].poll())
        self.assertTrue((self.install_dir / 'device-bot.exe').is_file())
        self.assertFalse((self.install_dir / 'device-bot.exe.previous').exists())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_foreground_upgrade_rejects_an_unrelated_caller(self):
        result = self.install_windows(running_processes=True, upgrade_caller='unrelated')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('caller does not belong to this Device Bot installation', result.stderr)
        for process in self.old_processes:
            self.assertIsNone(process.poll())
        self.assertIsNone(self.other_process.poll())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_failed_download_validation_keeps_old_processes_running(self):
        result = self.install_windows(running_processes=True, bad_checksum=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('scrcpy checksum verification failed', result.stderr)
        for process in self.old_processes:
            self.assertIsNone(process.poll())
        self.assertIsNone(self.other_process.poll())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_installer_includes_portable_ffmpeg_and_license(self):
        result = self.install_windows()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.install_dir / 'ffmpeg/bin/ffmpeg.exe').is_file())
        self.assertTrue((self.install_dir / 'ffmpeg/LICENSE').is_file())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_installer_rejects_bad_ffmpeg_before_stopping_old_server(self):
        result = self.install_windows(bad_ffmpeg_checksum=True, running_processes=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('FFmpeg checksum verification failed', result.stderr)
        for process in self.old_processes:
            self.assertIsNone(process.poll())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_installer_requires_ffmpeg_with_libx264(self):
        result = self.install_windows(ffmpeg_x264=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('FFmpeg with libx264 is required', result.stderr)
        self.assertFalse(self.install_dir.exists())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_installer_detects_cpu_without_architecture_environment(self):
        result = self.install_windows(empty_architecture=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.install_dir / 'device-bot.exe').is_file())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_installer_includes_android_live_dependencies(self):
        result = self.install_windows()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for name in ['device-bot.exe', 'robot-flow.exe', 'robot-report.exe', 'scrcpy.exe', 'scrcpy-server', 'adb.exe', 'SDL3.dll']:
            self.assertTrue((self.install_dir / name).is_file(), name)
        self.assertIn('Installed Android Live dependencies', result.stdout)

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_installer_rejects_bad_scrcpy_checksum_before_install(self):
        result = self.install_windows(bad_checksum=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('scrcpy checksum verification failed', result.stderr)
        self.assertFalse(self.install_dir.exists())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_installer_rejects_missing_scrcpy_server_before_install(self):
        result = self.install_windows(missing=('scrcpy-server',))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('does not contain scrcpy-server', result.stderr)
        self.assertFalse(self.install_dir.exists())

    @unittest.skipUnless(PWSH, 'PowerShell installer')
    def test_windows_installer_rejects_incompatible_scrcpy_before_install(self):
        result = self.install_windows(version='9.0.0')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('scrcpy version verification failed', result.stderr)
        self.assertFalse(self.install_dir.exists())

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
