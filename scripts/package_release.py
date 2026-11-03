#!/usr/bin/env python3
"""Create a native release archive including all tracked XCTest source assets."""
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


def package(binary, target, version, output):
    expected = re.search(r'^version\s*=\s*"([^"]+)"',
                         (ROOT / 'server/crates/mcp-server/Cargo.toml').read_text(), re.M)[1]
    if version != expected:
        raise ValueError(f'Tag version {version} does not match Cargo version {expected}')
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temp:
        stage = Path(temp)
        name = 'device-bot.exe' if 'windows' in target else 'device-bot'
        shutil.copy2(binary, stage / name)
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
                bundle.add(stage / name, arcname=name)
                bundle.add(stage / 'device-bot-assets', arcname='device-bot-assets')
        print(archive)
        return archive


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--target', required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    package(args.binary, args.target, args.version.removeprefix('v'), args.output)
