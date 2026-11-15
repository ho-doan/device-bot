const assert = require('node:assert/strict');
const test = require('node:test');
const { configurePage, configureAttachedTarget, configureBrowser } = require('./file_chooser.cjs');

test('browser setup intercepts future pages before their scripts can run', async () => {
  const calls = [];
  await configureBrowser(async (method, params) => { calls.push({ method, params }); });
  assert.deepEqual(calls, [{ method: 'Target.setAutoAttach', params: {
    autoAttach: true, waitForDebuggerOnStart: true, flatten: true,
    filter: [{ type: 'page', exclude: false }, { exclude: true }],
  } }]);
});

test('new pages remain paused until their own download context is configured', async () => {
  const calls = [];
  let finishDownloadSetup;
  const gate = new Promise(resolve => { finishDownloadSetup = resolve; });
  const call = async (method, params, session) => {
    calls.push({ method, session });
    if (method === 'Target.getTargetInfo') return { targetInfo: { browserContextId: 'new-context' } };
    if (method === 'Browser.setDownloadBehavior') await gate;
    return {};
  };
  const setup = configureAttachedTarget(call, { sessionId: 'new-session', targetInfo: { targetId: 'new-page' } }, '', '/tmp/downloads', () => {});
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls.map(c => c.method), ['Target.getTargetInfo', 'Browser.setDownloadBehavior']);
  finishDownloadSetup();
  await setup;
  assert.equal(calls.at(-1).method, 'Runtime.runIfWaitingForDebugger');
  assert.ok(calls.every(c => c.session === 'new-session'));
});

test('failed fixture setup resumes the page and cannot report target readiness', async () => {
  const calls = [], events = [];
  await assert.rejects(configureAttachedTarget(async method => {
    calls.push(method);
    if (method === 'Target.getTargetInfo') throw new Error('page detached');
    return {};
  }, { sessionId: 'gone', targetInfo: { targetId: 'gone' } }, '', '/tmp/downloads', event => events.push(event)), /page detached/);
  assert.equal(calls.at(-1), 'Runtime.runIfWaitingForDebugger');
  assert.deepEqual(events, []);
});

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
    params: { behavior: 'allow', downloadPath: '/tmp/report-downloads', eventsEnabled: true, browserContextId: 'flow-context' },
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
