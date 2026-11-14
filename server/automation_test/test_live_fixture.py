"""Real HTTP evidence routing for Live recordings, without native IME dependence."""
import json
import unittest
from urllib.error import HTTPError
from urllib.parse import urlsplit, parse_qs
from urllib.request import Request, urlopen

from live_fixture import LiveFixture


class LiveFixtureTests(unittest.TestCase):
    def test_numeric_urls_route_manual_and_replay_evidence_to_the_correct_case(self):
        with LiveFixture('127.0.0.1') as fixture:
            urls = [fixture.url_for(name) for name in ['qc-live-replay', 'qc-live-reopen']]
            for url in urls:
                query = urlsplit(url).query
                self.assertRegex(query, r'^0=\d+$')
                with urlopen(url) as response:
                    self.assertIn(b"get('0')", response.read())
                for session in ['manual', 'replay-1', 'replay-2']:
                    event = {'test': parse_qs(query)['0'][0], 'session': session, 'sequence': 1}
                    request = Request(fixture.url.replace('/live', '/events'),
                                      data=json.dumps(event).encode(),
                                      headers={'Content-Type': 'application/json'})
                    with urlopen(request) as response:
                        self.assertEqual(response.status, 204)
            for name in ['qc-live-replay', 'qc-live-reopen']:
                events = fixture.observations(name)
                self.assertEqual([event['session'] for event in events],
                                 ['manual', 'replay-1', 'replay-2'])
                self.assertTrue(all(event['test'] == name for event in events))

    def test_unknown_case_is_rejected_instead_of_silently_losing_evidence(self):
        with LiveFixture('127.0.0.1') as fixture:
            for key in [None, 'missing', {}]:
                request = Request(fixture.url.replace('/live', '/events'),
                                  data=json.dumps({'test': key, 'session': 'manual'}).encode())
                with self.assertRaises(HTTPError) as error:
                    urlopen(request)
                self.assertEqual(error.exception.code, 400)
                error.exception.close()
            self.assertEqual(fixture.events, [])


if __name__ == '__main__':
    unittest.main()
