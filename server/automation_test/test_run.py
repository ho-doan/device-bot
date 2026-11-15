"""Keep CI's suite selection complete and independent of a desktop session."""
import unittest
import json
import tempfile
from pathlib import Path
from unittest.mock import Mock, patch

from run import CASES, select_cases, verify_report_download, wait_fixture_worker


class SuiteSelectionTests(unittest.TestCase):
    def test_web_suite_covers_every_web_case_and_no_native_desktop_case(self):
        names = select_cases('web')
        self.assertEqual(len(names), 10)
        self.assertEqual(set(names), {name for name, config in CASES.items()
                                      if not config.get('desktop')})
        self.assertIn('qc-import-fail', names)
        self.assertIn('report-download', names)
        self.assertIn('report-persistence', names)

    def test_existing_default_and_live_selections_are_preserved(self):
        self.assertEqual(select_cases('all'), list(CASES))
        self.assertEqual(select_cases('live'), [name for name, config in CASES.items()
                                               if config.get('live')])
        self.assertEqual(select_cases('all', 'qc-desktop'), ['qc-desktop'])


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
