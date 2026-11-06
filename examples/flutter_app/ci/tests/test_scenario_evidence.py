"""Exercise the real shell helper with a fake MCP and no device access."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

HELPER = Path(__file__).resolve().parents[2] / 'automation_test/support/mcp.sh'


@unittest.skipUnless(shutil.which('jq'), 'shell evidence requires jq')
class ScenarioEvidenceTests(unittest.TestCase):
    def run_scenario(self, commands, missing_video=False):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            fake = root / 'device-bot'
            fake.write_text(f'#!{sys.executable}\n' + '''
import json, os, pathlib, sys
for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request:
        continue
    result = {'content': []}
    if request['method'] == 'tools/call':
        name = request['params']['name']
        with open(os.environ['CALLS'], 'a') as log:
            log.write(name + '\\n')
        if name == 'mobile_list_elements_on_screen':
            result['structuredContent'] = {'elements': [{
                'text': 'Welcome back.', 'className': 'staticText',
                'bounds': {'left': 0, 'top': 0, 'right': 100, 'bottom': 50}
            }]}
        elif name == 'mobile_take_screenshot':
            result['content'] = [{'type': 'image', 'mimeType': 'image/png', 'data': 'eA=='}]
        elif name == 'mobile_stop_screen_recording':
            video = pathlib.Path(os.environ['CALLS']).with_suffix('.mp4')
            if os.environ['MISSING_VIDEO'] != 'true':
                video.write_bytes(b'fake video; validation belongs to run_suite')
            result['content'] = [{'type': 'text', 'text': 'Screen recording saved: ' + str(video)}]
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}), flush=True)
''')
            fake.chmod(0o700)
            xcrun = root / 'xcrun'
            xcrun.write_text('#!/bin/sh\nexit 0\n')
            xcrun.chmod(0o700)
            evidence = root / 'evidence'
            env = dict(os.environ, PLATFORM='ios-sim', DEVICE_ID='fake-sim',
                       APP_ID='test.app', DEVICE_BOT_BIN=str(fake),
                       XCRUN_PATH=str(xcrun), EVIDENCE_DIR=str(evidence),
                       SCENARIO_RECORDS_VIDEO='true', RPC_TIMEOUT='5',
                       CALLS=str(root / 'calls'),
                       MISSING_VIDEO=str(missing_video).lower())
            env.pop('APP_PATH', None)
            env.pop('APK_PATH', None)
            script = 'CASE_ID=IT-01\nsource "$1"\n' + commands
            process = subprocess.run(['bash', '-c', script, 'scenario', str(HELPER)],
                                     env=env, capture_output=True, text=True, timeout=15)
            result = evidence / 'result.json'
            return (process, json.loads(result.read_text()) if result.exists() else None,
                    (root / 'calls').read_text().splitlines(),
                    (evidence / 'video.mp4').exists())

    def test_partial_result_and_video_use_ci_evidence_directory(self):
        process, result, calls, video = self.run_scenario('note_limitation "Busy state"\nfinish_flow\n')
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(result['status'], 'PARTIAL')
        self.assertEqual(len(result['limitations']), 1)
        self.assertTrue(video)
        self.assertEqual(calls.count('mobile_start_screen_recording'), 1)
        self.assertEqual(calls[-1], 'mobile_stop_screen_recording')

    def test_failure_still_saves_video_and_preserves_exit_code(self):
        process, result, calls, video = self.run_scenario('exit 7\n')
        self.assertEqual(process.returncode, 7, process.stderr)
        self.assertIsNone(result)
        self.assertTrue(video)
        self.assertEqual(calls[-1], 'mobile_stop_screen_recording')

    def test_missing_recording_fails_successful_scenario(self):
        process, result, calls, video = self.run_scenario('finish_flow\n', missing_video=True)
        self.assertNotEqual(process.returncode, 0)
        self.assertEqual(result['status'], 'PASS')
        self.assertFalse(video)
        self.assertEqual(calls[-1], 'mobile_stop_screen_recording')
