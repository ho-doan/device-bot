import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import run_suite as runner


class FakeMCP:
    calls = []
    def __init__(self, directory):
        pass
    def tool(self, name, args):
        self.calls.append(name)
        return {'content': []}
    def close(self):
        pass


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.script = self.root / 'case.sh'
        self.script.write_text('#!/usr/bin/env bash\nexit 0\n')
        FakeMCP.calls = []
        for target, value in [('ROOT', self.root), ('MCP', FakeMCP)]:
            mock = patch.object(runner, target, value)
            mock.start()
            self.addCleanup(mock.stop)
        env = patch.dict(os.environ, {'DEVICE_ID': 'test-device', 'APP_ID': 'test.app',
                                     'SCENARIO_RECORDS_VIDEO': 'false'})
        env.start()
        self.addCleanup(env.stop)

    def run_case(self, status='PASS', limits=None, code=0, video_error=False):
        def shell(script, env, directory, timeout):
            (directory / 'result.json').write_text(json.dumps({
                'status': status, 'limitations': limits or [],
            }))
            return code
        with patch.object(runner, 'run_shell', side_effect=shell), \
             patch.object(runner, 'collect_video', side_effect=RuntimeError('no video')
                          if video_error else None):
            return runner.run_case(self.script, self.root / 'evidence', 2)

    def test_failed_scenario_still_stops_recording(self):
        result = self.run_case(code=7)
        self.assertEqual(result['status'], 'FAIL')
        self.assertIn('mobile_stop_screen_recording', FakeMCP.calls)
        self.assertTrue((self.root / 'evidence/preliminary.json').is_file())

    def test_success_without_recording_is_error(self):
        result = self.run_case(video_error=True)
        self.assertEqual(result['status'], 'ERROR')
        self.assertEqual(result['qa_status'], 'PENDING')

    def test_partial_is_preserved_in_summary_and_junit(self):
        result = self.run_case(status='PARTIAL', limits=['Loading state not observed'])
        runner.write_reports([result], self.root)
        report = json.loads((self.root / 'summary.json').read_text())
        self.assertEqual(report['cases'][0]['status'], 'PARTIAL')
        self.assertEqual(report['qa_status'], 'PENDING')
        self.assertIsNotNone(ET.parse(self.root / 'junit.xml').find('.//skipped'))

    def test_pass_with_uncovered_assertions_is_rejected(self):
        result = self.run_case(limits=['Missing assertion'])
        self.assertEqual(result['status'], 'ERROR')

    def test_zero_exit_without_declared_result_is_error(self):
        with patch.object(runner, 'run_shell', return_value=0), \
             patch.object(runner, 'collect_video'):
            result = runner.run_case(self.script, self.root / 'evidence', 2)
        self.assertEqual(result['status'], 'ERROR')

    def test_scenario_owned_recording_requires_valid_video(self):
        with patch.dict(os.environ, {'SCENARIO_RECORDS_VIDEO': 'true'}), \
             patch.object(runner, 'verify_video', side_effect=RuntimeError('missing video')):
            result = self.run_case()
        self.assertEqual(result['status'], 'ERROR')
        self.assertEqual(FakeMCP.calls, [])

    def test_scenario_owned_recording_does_not_start_a_second_mcp(self):
        with patch.dict(os.environ, {'SCENARIO_RECORDS_VIDEO': 'true'}), \
             patch.object(runner, 'verify_video'):
            result = self.run_case()
        self.assertEqual(result['status'], 'PASS')
        self.assertEqual(FakeMCP.calls, [])

    def test_shell_timeout_still_collects_recording(self):
        import subprocess
        with patch.object(runner, 'run_shell', side_effect=subprocess.TimeoutExpired('bash', 1)), \
             patch.object(runner, 'collect_video'):
            result = runner.run_case(self.script, self.root / 'evidence', 1)
        self.assertEqual(result['status'], 'ERROR')
        self.assertIn('mobile_stop_screen_recording', FakeMCP.calls)

    def test_empty_suite_is_not_success(self):
        with patch.dict(os.environ, {'PLATFORM': 'android'}), \
             patch.object(runner.shutil, 'which', return_value='/test/tool'):
            self.assertEqual(runner.main(), 1)
        report = json.loads((self.root / 'artifacts/summary.json').read_text())
        self.assertEqual(report['cases'][0]['status'], 'ERROR')

    def test_discovers_new_scenario_names_and_excludes_shared_helpers(self):
        scenarios = self.root / 'automation_test'
        (scenarios / 'support').mkdir(parents=True)
        names = ['login-smoke.sh', 'it-10.sh', 'it-100.sh', 'it-01.sh']
        for name in names:
            (scenarios / name).write_text('exit 0\n')
        (scenarios / 'support/helper.sh').write_text('exit 1\n')
        (scenarios / 'README.md').write_text('Scenario documentation\n')
        (scenarios / 'directory.sh').mkdir()

        def passed(script, directory, timeout):
            return {'case': script.stem, 'status': 'PASS'}

        with patch.dict(os.environ, {'PLATFORM': 'ios-sim'}), \
             patch.object(runner.shutil, 'which', return_value='/test/tool'), \
             patch.object(runner, 'run_case', side_effect=passed) as case:
            self.assertEqual(runner.main(), 0)
        self.assertEqual([call.args[0].name for call in case.call_args_list], sorted(names))
        report = json.loads((self.root / 'artifacts/summary.json').read_text())
        self.assertEqual(len(report['cases']), len(names))

    def test_actual_shell_timeout_stops_background_children(self):
        import subprocess
        self.script.write_text('sleep 30 &\nwait\n')
        with self.assertRaises(subprocess.TimeoutExpired):
            runner.run_shell(self.script, dict(os.environ), self.root, 0.05)

    def test_windows_timeout_terminates_process_tree_without_posix_signals(self):
        import subprocess
        from unittest.mock import Mock
        process = Mock(pid=123, **{'wait.side_effect': [subprocess.TimeoutExpired('bash', 1), 0],
                                  'poll.return_value': None})
        with patch.object(runner, 'IS_WINDOWS', True), \
             patch.object(runner.subprocess, 'CREATE_NEW_PROCESS_GROUP', 512, create=True), \
             patch.object(runner.subprocess, 'Popen', return_value=process), \
             patch.object(runner.subprocess, 'run') as terminate:
            with self.assertRaises(subprocess.TimeoutExpired):
                runner.run_shell(self.script, dict(os.environ), self.root, 1)
        self.assertEqual(terminate.call_args.args[0], ['taskkill', '/PID', '123', '/T', '/F'])

    def test_windows_platform_can_run_discovered_scenarios(self):
        scenarios = self.root / 'automation_test'
        scenarios.mkdir()
        (scenarios / 'desktop-flow.sh').write_text('exit 0\n')
        with patch.dict(os.environ, {'PLATFORM': 'windows'}), \
             patch.object(runner.shutil, 'which', return_value='/test/tool'), \
             patch.object(runner, 'run_case', return_value={'case': 'desktop-flow', 'status': 'PASS'}) as case:
            self.assertEqual(runner.main(), 0)
        case.assert_called_once()

    def test_cancellation_stops_suite_before_next_case(self):
        scenarios = self.root / 'automation_test'
        scenarios.mkdir(parents=True)
        (scenarios / 'it-01.sh').write_text('exit 0\n')
        (scenarios / 'it-02.sh').write_text('exit 0\n')
        with patch.dict(os.environ, {'PLATFORM': 'ios-sim'}), \
             patch.object(runner.shutil, 'which', return_value='/test/tool'), \
             patch.object(runner, 'run_case', side_effect=runner.RunnerInterrupted('cancelled')) as case:
            self.assertEqual(runner.main(), 1)
            self.assertEqual(case.call_count, 1)

    def test_cancellation_still_finalizes_recording(self):
        with patch.object(runner, 'run_shell', side_effect=runner.RunnerInterrupted('cancelled')), \
             patch.object(runner, 'collect_video'):
            with self.assertRaises(runner.RunnerInterrupted):
                runner.run_case(self.script, self.root / 'evidence', 1)
        self.assertIn('mobile_stop_screen_recording', FakeMCP.calls)

    def test_video_with_no_video_stream_is_rejected(self):
        import subprocess
        source = self.root / 'source.mp4'
        source.write_bytes(b'not a real video')
        response = {'content': [{'text': f'Screen recording saved: {source}'}]}
        probe = subprocess.CompletedProcess([], 0, '{"format":{"duration":"1"},"streams":[]}')
        with patch.object(runner.subprocess, 'run', return_value=probe):
            with self.assertRaisesRegex(RuntimeError, 'no video stream'):
                runner.collect_video(response, self.root)


class TransportTests(unittest.TestCase):
    def test_stdio_lifecycle_and_tool_error(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            fake = root / 'fake-mcp'
            fake.write_text('''#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request:
        continue
    result = {'protocolVersion': '2025-11-25'}
    if request['method'] == 'tools/call':
        result = {'isError': True, 'content': [{'type': 'text', 'text': 'unsupported device'}]}
    print(json.dumps({'jsonrpc':'2.0', 'id':request['id'], 'result':result}), flush=True)
''')
            fake.chmod(0o700)
            with patch.dict(os.environ, {'DEVICE_BOT_BIN': str(fake)}):
                mcp = runner.MCP(root)
                try:
                    with self.assertRaisesRegex(RuntimeError, 'unsupported device'):
                        mcp.tool('mobile_start_screen_recording', {'deviceId': 'unknown'})
                finally:
                    mcp.close()
            self.assertIsNotNone(mcp.process.returncode)
            self.assertEqual(len((root / 'mcp.jsonl').read_text().splitlines()), 2)


if __name__ == '__main__':
    unittest.main()
