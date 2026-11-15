const assert = require('node:assert/strict');
const test = require('node:test');
const { configurePage } = require('./file_chooser.cjs');

test('download setup works even when the target has no active Page domain', async () => {
  const calls = [];
  const call = async (method, params) => {
    if (method.startsWith('Page.')) throw new Error('Not attached to an active page');
    calls.push({ method, params });
    return { targetInfo: { browserContextId: 'flow-context' } };
  };
  await configurePage(call, '', '/tmp/report-downloads');
  assert.deepEqual(calls, [{ method: 'Target.getTargetInfo', params: undefined }, {
    method: 'Browser.setDownloadBehavior',
    params: { behavior: 'allow', downloadPath: '/tmp/report-downloads', browserContextId: 'flow-context' },
  }]);
});

test('import fixtures still enable the real file chooser interception', async () => {
  const calls = [];
  await configurePage(async (method, params) => {
    calls.push({ method, params });
    return { targetInfo: {} };
  }, '/tmp/pass.yaml', '/tmp/import-downloads');
  assert.ok(calls.some(call => call.method === 'Page.enable'));
  assert.ok(calls.some(call => call.method === 'Page.setInterceptFileChooserDialog' && call.params.enabled));
  assert.ok(calls.some(call => call.method === 'Browser.setDownloadBehavior' && call.params.downloadPath === '/tmp/import-downloads'));
});
