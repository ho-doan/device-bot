#!/usr/bin/env python3
"""Package a prebuilt macOS QC app and native tools as a double-clickable app."""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import tarfile
import tempfile

from package_release import package


LAUNCHER = '''#!/bin/sh
set -eu
CONTENTS="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)"
export DEVICE_BOT_QC_APP="$CONTENTS/Resources/mobile_mcp_companion.app"
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
mkdir -p "$HOME/.device-bot"
if ! "$CONTENTS/Resources/bin/device-bot" qc >> "$HOME/.device-bot/qc-launch.log" 2>&1; then
  /usr/bin/osascript -e 'display alert "Device Bot QC could not start" message "See ~/.device-bot/qc-launch.log for details. If another server uses port 3337, stop it and open QC again." as critical'
  exit 1
fi
'''


def package_qc(binary_dir, qc_app, target, version, output):
    arch = {'aarch64-apple-darwin': 'arm64', 'x86_64-apple-darwin': 'x86_64'}[target]
    signed_entitlements = subprocess.check_output(
        ['codesign', '-d', '--entitlements', ':-', str(qc_app)], stderr=subprocess.DEVNULL)
    entitlements = plistlib.loads(signed_entitlements) if signed_entitlements.strip() else {}
    if entitlements.get('com.apple.security.app-sandbox'):
        raise ValueError('Standalone QC must be rebuilt without App Sandbox')
    for binary in [*(binary_dir / name for name in ('device-bot', 'robot-flow', 'robot-report')),
                   qc_app / 'Contents/MacOS/mobile_mcp_companion',
                   qc_app / 'Contents/Frameworks/App.framework/App',
                   qc_app / 'Contents/Frameworks/FlutterMacOS.framework/FlutterMacOS']:
        subprocess.run(['lipo', str(binary), '-verify_arch', arch], check=True)
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        app = root / 'Device Bot QC.app'
        contents = app / 'Contents'
        resources = contents / 'Resources'
        binaries = resources / 'bin'
        binaries.mkdir(parents=True)
        native = package(binary_dir / 'device-bot', target, version, root / 'native')
        with tarfile.open(native) as bundle:
            bundle.extractall(binaries, filter='data')
        shutil.copytree(qc_app, resources / qc_app.name, symlinks=True)
        with (qc_app / 'Contents/Info.plist').open('rb') as source:
            original = plistlib.load(source)
        icon = original.get('CFBundleIconFile') or 'AppIcon'
        if not icon.endswith('.icns'):
            icon += '.icns'
        shutil.copy2(qc_app / 'Contents/Resources' / icon, resources / icon)
        with (contents / 'Info.plist').open('wb') as destination:
            plistlib.dump({
                'CFBundleExecutable': 'DeviceBotQC',
                'CFBundleIdentifier': 'dev.devicebot.qc.launcher',
                'CFBundleName': 'Device Bot QC',
                'CFBundleDisplayName': 'Device Bot QC',
                'CFBundlePackageType': 'APPL',
                'CFBundleShortVersionString': version,
                'CFBundleVersion': original.get('CFBundleVersion', '1'),
                'CFBundleIconFile': icon,
                'LSMinimumSystemVersion': '12.0',
                'LSUIElement': True,
                'NSHighResolutionCapable': True,
            }, destination)
        launcher = contents / 'MacOS/DeviceBotQC'
        launcher.parent.mkdir()
        launcher.write_text(LAUNCHER)
        launcher.chmod(0o755)
        (resources / 'BUILD.json').write_text(json.dumps({
            'version': version, 'target': target,
            'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
            'workingTreeDirty': bool(subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], text=True).strip()),
            'signing': 'ad-hoc; not notarized',
            'appSandbox': False,
        }, indent=2) + '\n')
        subprocess.run(['codesign', '--force', '--sign', '-', str(app)], check=True)
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True)
        archive = output / f'device-bot-qc-v{version}-{target}.zip'
        subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app), str(archive)], check=True)
    with archive.open('rb') as source:
        digest = hashlib.file_digest(source, 'sha256').hexdigest()
    archive.with_suffix('.zip.sha256').write_text(f'{digest}  {archive.name}\n')
    print(archive)
    return archive


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path, required=True)
    parser.add_argument('--qc-app', type=Path, required=True)
    parser.add_argument('--target', choices=['aarch64-apple-darwin', 'x86_64-apple-darwin'], required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    package_qc(args.binary_dir.resolve(), args.qc_app.resolve(), args.target,
               args.version.removeprefix('v'), args.output.resolve())
