#!/usr/bin/env python3
"""Replay Flutter-exported YAML through the Rust runner against a protocol fixture.

This verifies compiler/transport contracts, not physical typing on a device.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def serve_fixture(observed):
    texts = []
    tools = ["mobile_boot_device", "mobile_list_available_devices", "mobile_open_url",
             "mobile_launch_app", "mobile_list_elements_on_screen",
             "mobile_click_on_screen_at_coordinates", "mobile_type_keys", "mobile_scroll"]
    for line in sys.stdin:
        request = json.loads(line)
        method = request['method']
        if 'id' not in request:
            continue
        if method == 'initialize':
            result = {'protocolVersion': '2025-11-25'}
        elif method == 'tools/list':
            result = {'tools': [{'name': name, 'inputSchema': {}} for name in tools]}
        elif method == 'tools/call':
            call = request['params']
            name, args = call['name'], call['arguments']
            if name == 'mobile_list_available_devices':
                result = {'structuredContent': {'devices': [
                    {'id': 'phone-1', 'platform': 'android', 'state': 'available'},
                    {'id': 'chrome:fresh', 'platform': 'chrome', 'state': 'available'}]}}
            elif name == 'mobile_list_elements_on_screen':
                result = {'structuredContent': {'elements': [{
                    'resourceId': 'probe', 'className': 'input', 'elementType': 'textField',
                    'enabled': True, 'clickable': True, 'focused': True,
                    'bounds': {'left': 0, 'top': 0, 'right': 200, 'bottom': 40}}]}}
            elif name == 'mobile_type_keys':
                texts.append(args['text'])
                result = {'content': []}
            elif name in tools:
                result = {'content': []}
            else:
                raise RuntimeError(f'Unexpected fixture tool: {name}')
        else:
            raise RuntimeError(f'Unexpected method: {method}')
        print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}), flush=True)
    observed.write_text(json.dumps({'texts': texts}, ensure_ascii=False), encoding='utf-8')


def verify(fixtures, runner):
    files = sorted(fixtures.glob('*.yaml'))
    missing = {'android.yaml', 'chrome.yaml', 'literal.yaml'} - {path.name for path in files}
    if missing:
        raise RuntimeError(f'Missing Flutter recording contracts: {sorted(missing)}')
    for source in files:
        expected_path = source.with_suffix('.expected.json')
        expected = json.loads(expected_path.read_text(encoding='utf-8'))
        with tempfile.TemporaryDirectory(prefix='recording-contract-') as directory:
            directory = Path(directory)
            observed = directory / 'observed.json'
            env = os.environ | {'DEVICE_BOT_REPORT_DATA_DIR': str(directory / 'history'),
                                'DEVICE_BOT_RUNS_DIR': str(directory / 'runs'), 'PYTHONUTF8': '1'}
            result = subprocess.run([
                str(runner), '--server', sys.executable,
                '--server-arg', str(Path(__file__).resolve()),
                '--server-arg', '--fake-mcp', '--server-arg', str(observed),
                '--artifacts', str(directory / 'artifacts'), str(source)],
                env=env, capture_output=True, text=True, encoding='utf-8', timeout=30)
            reports = list((directory / 'artifacts').glob('case-*/report.json'))
            report = json.loads(reports[0].read_text(encoding='utf-8')) if reports else {}
            if result.returncode or report.get('status') != 'PASS':
                raise RuntimeError(f"{source.name}: replay failed: {report.get('error', 'missing report')}")
            actual = json.loads(observed.read_text(encoding='utf-8'))['texts']
            wanted = expected.get('texts', [expected.get('text')])
            if actual != wanted:
                raise RuntimeError(f'{source.name}: replay changed recorded text')
            print(f'PASS: exported recording {source.name}', flush=True)


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--fake-mcp':
        serve_fixture(Path(sys.argv[2]))
    else:
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument('--fixtures', type=Path, required=True)
        parser.add_argument('--runner', type=Path, default=ROOT / 'server/target/debug' /
                            ('robot-flow.exe' if os.name == 'nt' else 'robot-flow'))
        args = parser.parse_args()
        verify(args.fixtures.resolve(), args.runner.resolve())
