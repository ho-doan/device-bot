"""Keep CI's suite selection complete and independent of a desktop session."""
import unittest
import json
import tempfile
from pathlib import Path
from unittest.mock import Mock, patch

from run import CASES, SUITE, prepare_web_browser, select_cases, verify_report_download, wait_fixture_worker


class SuiteSelectionTests(unittest.TestCase):
    def test_web_suite_covers_ready_cases_without_drafts_or_native_desktop(self):
        names = select_cases('web')
        self.assertEqual(set(names), {name for name, config in CASES.items()
                                      if not config.get('desktop') and (SUITE / f'{name}.yaml').is_file()})
        for required in ('qc-web', 'report-web', 'qc-import-pass', 'qc-import-fail',
                         'qc-import-invalid', 'qc-import-empty', 'qc-import-syntax-error',
                         'qc-import-bad-version', 'qc-web-recovery', 'qc-web-token-whitespace',
                         'report-empty-search', 'report-download', 'report-persistence'):
            self.assertIn(required, names)

    def test_existing_default_and_live_selections_are_preserved(self):
        ready = [name for name in CASES if (SUITE / f'{name}.yaml').is_file()]
        self.assertEqual(select_cases('all'), ready)
        self.assertEqual(select_cases('live'), [name for name in ready if CASES[name].get('live')])
        self.assertEqual(select_cases('all', 'qc-desktop'), ['qc-desktop'])

    def test_promoting_a_draft_makes_it_eligible(self):
        with tempfile.TemporaryDirectory() as directory:
            suite = Path(directory)
            draft = suite / 'web-case.yaml.draft'
            draft.touch()
            with patch('run.SUITE', suite), patch('run.CASES', {'web-case': {}}):
                self.assertEqual(select_cases('web'), [])
                with self.assertRaisesRegex(ValueError, 'no ready .yaml scenario'):
                    select_cases('web', 'web-case')
                draft.rename(suite / 'web-case.yaml')
                self.assertEqual(select_cases('web'), ['web-case'])


class ChromeStartupTests(unittest.TestCase):
    def test_cold_chrome_reaches_a_working_case_context_before_replay(self):
        with patch('run.fixture_tool', side_effect=[
            RuntimeError('Fixture tool mobile_boot_device failed: debugging did not become ready within 15 seconds'),
            RuntimeError('Fixture tool mobile_boot_device failed: Chrome command Target.createTarget timed out after 5 seconds'),
            {},
        ]) as boot, patch('run.time.sleep'):
            prepare_web_browser('http://127.0.0.1:3338', 'fixture-token')
        self.assertEqual(boot.call_count, 3)
        boot.assert_called_with('http://127.0.0.1:3338', 'fixture-token',
                                'mobile_boot_device', {'deviceId': 'chrome:browser'})

    def test_permanent_errors_fail_immediately_and_cold_retries_are_bounded(self):
        with patch('run.fixture_tool', side_effect=RuntimeError('Chrome is not installed')) as boot:
            with self.assertRaisesRegex(RuntimeError, 'Chrome is not installed'):
                prepare_web_browser('http://127.0.0.1:3338', 'fixture-token')
        boot.assert_called_once()
        with patch('run.fixture_tool', side_effect=RuntimeError(
            'Fixture tool mobile_boot_device failed: Chrome command Target.createTarget timed out after 5 seconds'
        )) as boot, patch('run.time.sleep'):
            with self.assertRaisesRegex(RuntimeError, 'Target.createTarget timed out'):
                prepare_web_browser('http://127.0.0.1:3338', 'fixture-token')
        self.assertEqual(boot.call_count, 4)


class DownloadSynchronizationTests(unittest.TestCase):
    def test_worker_waits_for_browser_interception_not_an_old_page(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'worker.log'
            log.write_text('{"event":"target-ready"}\n', encoding='utf-8')
            worker = Mock()
            worker.poll.return_value = None
            with patch('run.time.sleep', side_effect=lambda _: log.write_text(
                    '{"event":"browser-ready"}\n', encoding='utf-8')) as sleep:
                wait_fixture_worker(worker, log)
            sleep.assert_called_once()

    def test_worker_exit_is_an_error_even_with_a_stale_ready_marker(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'worker.log'
            log.write_text('{"event":"browser-ready"}\n', encoding='utf-8')
            worker = Mock()
            worker.poll.return_value = 1
            with self.assertRaisesRegex(RuntimeError, 'exited before readiness'):
                wait_fixture_worker(worker, log)

    def test_partial_export_is_retried_until_both_fixture_cases_are_complete(self):
        with tempfile.TemporaryDirectory() as directory:
            downloads = Path(directory)
            report = downloads / 'run-report.json'
            report.write_text('{"cases":', encoding='utf-8')
            complete = {'cases': [{'report': {'name': name}}
                                  for name in ['self-test passed', 'self-test failure']]}
            with patch('run.time.sleep', side_effect=lambda _: report.write_text(
                    json.dumps(complete), encoding='utf-8')):
                result = verify_report_download(downloads)
            self.assertEqual(result, {'file': 'run-report.json', 'caseCount': 2})

    def test_wrong_export_reports_filename_and_observed_case_names(self):
        with tempfile.TemporaryDirectory() as directory:
            downloads = Path(directory)
            (downloads / 'other.json').write_text(json.dumps({'cases': [
                {'report': {'name': 'wrong-one'}}, {'report': {'name': 'wrong-two'}}]}), encoding='utf-8')
            with patch('run.time.monotonic', side_effect=[0, 0, 2]), patch('run.time.sleep'):
                with self.assertRaisesRegex(RuntimeError, 'other.json: unexpected fixture cases'):
                    verify_report_download(downloads, timeout=1)


if __name__ == '__main__':
    unittest.main()
