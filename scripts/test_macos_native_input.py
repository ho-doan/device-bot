"""Opt-in native MCP smoke: python3 scripts/test_macos_native_input.py --run.

Requires an interactive macOS session, Swift, and Accessibility permission for
Device Bot. Creates its own backend and two AppKit windows; never controls an
existing app. Build the backend first with cargo build -p device-bot in server/.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid


def wait_for(check, message):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.1)
    raise AssertionError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--query-only', action='store_true', help='Check AX queries without sending input')
    parser.add_argument('--server-binary', type=Path,
                        default=Path(__file__).resolve().parents[1] / 'server/target/debug/device-bot')
    args = parser.parse_args()
    if not args.run or sys.platform != 'darwin':
        print('SKIP: pass --run on an interactive macOS host')
        return
    with tempfile.TemporaryDirectory(prefix='device-bot-native-smoke-') as directory:
        root = Path(directory)
        fixture = root / 'input-fixture'
        subprocess.run(['swiftc', str(Path(__file__).parent / 'fixtures/macos_input.swift'),
                        '-o', str(fixture)], check=True)
        with socket.socket() as reserved:
            reserved.bind(('127.0.0.1', 0))
            port = reserved.getsockname()[1]
        token = uuid.uuid4().hex
        env = dict(os.environ, DEVICE_BOT_AUTH=token, DEVICE_BOT_FLUTTER_AUTOSTART='0',
                   DEVICE_BOT_ARTIFACT_DIR=str(root / 'artifacts'),
                   DEVICE_BOT_REPORT_DATA_DIR=str(root / 'reports'),
                   DEVICE_BOT_RUNS_DIR=str(root / 'runs'),
                   DEVICE_BOT_IOS_AGENT_BIND='127.0.0.1:0',
                   DEVICE_BOT_IOS_SIM_AGENT_BIND='127.0.0.1:0')
        children = []
        session = None
        sequence = 0
        with (root / 'processes.log').open('wb') as log:
            try:
                backend = subprocess.Popen([str(args.server_binary.resolve()), 'mcp', '--listen',
                                            f'127.0.0.1:{port}'], cwd=root, env=env,
                                           stdin=subprocess.DEVNULL, stdout=log, stderr=log)
                children.append(backend)

                def rpc(method, params=None):
                    nonlocal session, sequence
                    sequence += 1
                    headers = {'Content-Type': 'application/json', 'Authorization': 'Bearer ' + token}
                    if session:
                        headers['mcp-session-id'] = session
                    payload = {'jsonrpc': '2.0', 'method': method, 'params': params or {}}
                    if not method.startswith('notifications/'):
                        payload['id'] = sequence
                    request = urllib.request.Request(f'http://127.0.0.1:{port}/mcp',
                              data=json.dumps(payload).encode(), headers=headers)
                    with urllib.request.urlopen(request, timeout=30) as response:
                        session = response.headers.get('mcp-session-id', session)
                        raw = response.read()
                        if not raw:
                            return {}
                        value = json.loads(raw)
                    assert 'error' not in value, value.get('error')
                    return value['result']

                def initialize():
                    if backend.poll() is not None:
                        raise AssertionError('Fixture backend exited')
                    try:
                        return rpc('initialize', {'protocolVersion': '2025-11-25', 'capabilities': {},
                                   'clientInfo': {'name': 'native-input-smoke', 'version': '1'}})
                    except urllib.error.URLError:
                        return None

                wait_for(initialize, 'Backend did not start')
                rpc('notifications/initialized')

                def tool(name, arguments, expect_error=None):
                    result = rpc('tools/call', {'name': name, 'arguments': arguments})
                    if expect_error is not None:
                        assert result.get('isError') and expect_error in str(result.get('content')), result
                    else:
                        assert not result.get('isError'), result
                    return result.get('structuredContent', {})

                def launch(name, *arguments):
                    path = root / (name + '.json')
                    process = subprocess.Popen([str(fixture), str(path), *arguments],
                                               stdout=log, stderr=log, stdin=subprocess.DEVNULL)
                    children.append(process)
                    wait_for(path.exists, 'AppKit fixture did not start')
                    return process, path

                def query_value(device, text):
                    elements = tool('mobile_list_elements_on_screen', dict(
                        device, source='native', filter=text.upper(), elementType='textField', maxResults=1,
                    ))['elements']
                    assert len(elements) == 1, elements
                    editor = elements[0]
                    assert editor['contentDescription'] == 'Notes input' and editor['value'] == text, editor
                    return editor

                if args.query_only:
                    text = 'Device Bot AX query fixture'
                    first, state_path = launch('query', text)
                    state = json.loads(state_path.read_text())
                    query_value({'deviceId': 'macos-host', 'processId': first.pid}, text)
                    query_value({'deviceId': f"macos:{first.pid}:{state['windowId']}"}, text)
                    print('PASS: native AX host/window queries match case, value, and type before the result limit')
                    return

                first, state_path = launch('first')
                second, other_state = launch('second')
                device = {'deviceId': 'macos-host'}
                tool('mobile_activate_app', dict(device, processId=second.pid))
                elements = tool('mobile_list_elements_on_screen',
                                dict(device, processId=first.pid, source='native'))['elements']
                for path in (state_path, other_state):
                    assert json.loads(path.read_text())['text'] == '', f'{path.stem} input was already populated'
                tool('mobile_type_keys', dict(device, text='must not reach either app'),
                     expect_error='Activate the selected macOS app')
                for path in (state_path, other_state):
                    assert json.loads(path.read_text())['text'] == '', f'{path.stem} input received blocked typing'
                tool('mobile_activate_app', dict(device, processId=first.pid))
                field = next(e for e in elements if e.get('contentDescription') == 'Notes input')
                disabled = next(e for e in elements if e.get('contentDescription') == 'Disabled input')
                assert field['clickable'] and field['value'] == ''
                assert not disabled['enabled'] and not disabled['clickable']

                def tap(element):
                    b = element['bounds']
                    tool('mobile_click_on_screen_at_coordinates',
                         dict(device, x=(b['left'] + b['right']) / 2, y=(b['top'] + b['bottom']) / 2))

                tap(field)
                text = 'Device Bot native controller verified'
                tool('mobile_type_keys', dict(device, text=text))
                wait_for(lambda: json.loads(state_path.read_text())['text'] == text,
                         'Real keyboard input did not reach the app delegate')
                current = query_value(dict(device, processId=first.pid), text)
                assert current['focused'], current
                tap(next(e for e in elements if e.get('text') == 'Commit fixture'))
                wait_for(lambda: json.loads(state_path.read_text())['committed'], 'AXPress did not invoke button action')
                area = next(e for e in elements if e.get('contentDescription') == 'Drag test area')['bounds']
                tool('mobile_swipe_on_screen', dict(device, startX=area['left'] + 100,
                     startY=area['top'] + 60, endX=area['left'] + 400, endY=area['top'] + 170, durationMs=400))
                state = wait_for(lambda: (s if 'dragEnd' in (s := json.loads(state_path.read_text())) else None),
                                 'No native mouseUp callback')
                assert state['dragMoves'] > 0, state
                for actual, expected in [(state['dragStart'], [100, 60]), (state['dragEnd'], [400, 170])]:
                    assert all(abs(a - e) <= 1 for a, e in zip(actual, expected)), state
                print('PASS: native AX focus/value/enabled and typed query; '
                      'real keyboard delegate; button; drag endpoints; PID guard')
            finally:
                for process in reversed(children):
                    if process.poll() is None:
                        process.terminate()
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait(timeout=5)


if __name__ == '__main__':
    main()
