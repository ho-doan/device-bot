#!/usr/bin/env python3
"""Create a native tools archive with the CLI suite and tracked XCTest assets."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def validate_release_versions(version, binary_directory=None):
    """Fail before packaging/building Flutter when any CLI version is stale."""
    for tool, crate in [('device-bot', 'mcp-server'), ('robot-flow', 'robot-flow'), ('robot-report', 'robot-report')]:
        manifest = tomllib.loads((ROOT / f'server/crates/{crate}/Cargo.toml').read_text())
        declared = manifest.get('package', {}).get('version')
        if isinstance(declared, dict) and declared.get('workspace') is True:
            workspace = tomllib.loads((ROOT / 'server/Cargo.toml').read_text())
            declared = workspace.get('workspace', {}).get('package', {}).get('version')
        if declared != version:
            raise ValueError(f'{tool} version {declared} does not match tag {version}')
        if binary_directory is not None:
            suffix = '.exe' if os.name == 'nt' else ''
            binary = Path(binary_directory) / (tool + suffix)
            result = subprocess.run([str(binary), '--version'], check=True,
                                    capture_output=True, text=True, timeout=10)
            match = re.search(r'\b\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?\b', result.stdout)
            actual = match[0] if match else result.stdout.strip()
            if actual != version:
                raise ValueError(f'{tool} binary version {actual} does not match tag {version}')
            print(f'{tool}: {actual}')


def stage_qc(stage, target, qc):
    if qc is None:
        return
    if not qc.exists():
        raise ValueError(f'Missing QC app: {qc}')
    destination = stage / 'qc'
    destination.mkdir()
    if 'windows' in target:
        executable = qc / 'mobile_mcp_companion.exe'
        if not executable.is_file():
            raise ValueError(f'Missing QC executable: {executable}')
        shutil.copytree(qc, destination, dirs_exist_ok=True, symlinks=True)
        return
    app = qc if qc.suffix == '.app' else qc / 'mobile_mcp_companion.app'
    if not app.is_dir():
        raise ValueError(f'Missing QC app bundle: {app}')
    shutil.copytree(app, destination / 'mobile_mcp_companion.app', symlinks=True)


def package(binary, target, version, output, qc=None, qc_web=None):
    validate_release_versions(version)
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temp:
        stage = Path(temp)
        suffix = '.exe' if 'windows' in target else ''
        binaries = ['device-bot', 'robot-flow', 'robot-report']
        for tool in binaries:
            source = binary if tool == 'device-bot' else binary.parent / (tool + suffix)
            if not source.is_file():
                raise ValueError(f'Missing release binary: {source}')
            shutil.copy2(source, stage / (tool + suffix))
        shutil.copytree(ROOT / 'server/crates/robot-report/deploy',
                        stage / 'robot-report-assets' / version / 'deploy')
        (stage / 'bundle.json').write_text(json.dumps({
            'version': version, 'binaries': [name + suffix for name in binaries],
        }, indent=2) + '\n')
        assets = stage / 'device-bot-assets' / version
        files = []
        for platform in ('ios-sim', 'ios-real'):
            prefix = f'server/crates/robot-{platform}/xctest-runner/'
            tracked = subprocess.check_output(['git', 'ls-files', prefix], cwd=ROOT, text=True)
            for source in tracked.splitlines():
                relative = Path(platform) / 'xctest-runner' / source[len(prefix):]
                destination = assets / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(ROOT / source, destination)
                files.append(relative.as_posix())
        if not files:
            raise ValueError('No tracked XCTest assets were found')
        # Optional DOM inspection has a pinned, separately installed Node runtime
        # dependency. Ship its setup contract and scripts in every native archive.
        helper = ROOT / 'server/crates/robot-ios-real/webview-helper'
        destination = stage / 'webview-helper'
        destination.mkdir()
        for name in ('webview.mjs', 'dom-references.js', 'dom-tap.js', 'dom-scroll.js', 'dom-edit.js', 'package.json', 'package-lock.json', 'README.md'):
            shutil.copy2(helper / name, destination / name)
        stage_qc(stage, target, qc)
        if qc_web is not None:
            if not (qc_web / 'index.html').is_file() or not (qc_web / 'flutter_bootstrap.js').is_file():
                raise ValueError(f'Missing QC web build: {qc_web}')
            shutil.copytree(qc_web, stage / 'qc-web', symlinks=True)
        (assets / 'manifest.json').write_text(json.dumps({
            'version': version, 'files': sorted(files),
        }, indent=2) + '\n')
        stem = f'device-bot-v{version}-{target}'
        if 'windows' in target:
            archive = output / (stem + '.zip')
            with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as bundle:
                for path in sorted(stage.rglob('*')):
                    if path.is_file():
                        bundle.write(path, path.relative_to(stage))
        else:
            archive = output / (stem + '.tar.gz')
            with tarfile.open(archive, 'w:gz') as bundle:
                for path in sorted(stage.iterdir()):
                    bundle.add(path, arcname=path.name)
        print(archive)
        return archive


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--target', required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--qc', type=Path)
    parser.add_argument('--qc-web', type=Path)
    args = parser.parse_args()
    package(args.binary, args.target, args.version.removeprefix('v'), args.output, args.qc, args.qc_web)
