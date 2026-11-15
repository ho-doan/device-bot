"""Keep CI's suite selection complete and independent of a desktop session."""
import unittest

from run import CASES, select_cases


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


if __name__ == '__main__':
    unittest.main()
