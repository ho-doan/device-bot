const { test } = require('node:test');
const assert = require('node:assert/strict');
const history = require('../src/web/history.js');
const groups = [
  { projectId: 'android', runs: [{ id: 'hybrid', lastReportAt: 2, counts: { passed: 1 } }, { id: 'app', lastReportAt: 1 }] },
  { projectId: 'web', runs: [{ id: 'hybrid', lastReportAt: 2, counts: { passed: 1 } }, { id: 'page', lastReportAt: 3 }] },
];
test('multi-group history deduplicates aliases without adding their counts', () => {
  const runs = history.merge(groups, ['android', 'web']);
  assert.deepEqual(runs.map(run => run.id), ['page', 'hybrid', 'app']);
  assert.deepEqual(runs[1].projectIds, ['android', 'web']);
  assert.equal(runs[1].counts.passed, 1);
  assert.equal(runs[1].projectId, 'android');
});
test('all-groups filter finds flows that tested both targets', () => {
  assert.deepEqual(history.merge(groups, ['android', 'web'], 'all').map(run => run.id), ['hybrid']);
});
test('an empty group cannot match an all-groups filter', () => {
  assert.deepEqual(history.merge([...groups, { projectId: 'empty', runs: [] }], ['web', 'empty'], 'all'), []);
  assert.deepEqual(history.merge([], []), []);
});
