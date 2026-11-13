#!/usr/bin/env python3
"""Create a native tools archive with the CLI suite and tracked XCTest assets."""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]


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
    expected = re.search(r'^version\s*=\s*"([^"]+)"',
                         (ROOT / 'server/crates/mcp-server/Cargo.toml').read_text(), re.M)[1]
    if version != expected:
        raise ValueError(f'Tag version {version} does not match Cargo version {expected}')
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temp:
        stage = Path(temp)
        suffix = '.exe' if 'windows' in target else ''
        binaries = ['device-bot', 'robot-flow', 'robot-report']
        for tool, crate in [('device-bot', 'mcp-server'), ('robot-flow', 'robot-flow'), ('robot-report', 'robot-report')]:
            tool_version = re.search(r'^version\s*=\s*"([^"]+)"',
                                    (ROOT / f'server/crates/{crate}/Cargo.toml').read_text(), re.M)[1]
            if tool_version != version:
                raise ValueError(f'{tool} version {tool_version} does not match tag {version}')
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
