const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const script = fs.readFileSync(path.join(__dirname, '../src/live.js'), 'utf8')
  .replace('__KEY__', '"capture"').replace('__OFFER__', '"offer"');

async function capture(errorName, cropError = false, options = {}) {
  const failure = Object.assign(new Error('Invalid state'), { name: errorName });
  let stopped = 0;
  const track = { stop() { stopped++; }, async cropTo() { if (cropError) throw failure; await options.crop; } };
  const media = { getTracks: () => [track], getVideoTracks: () => [track] };
  const listeners = new Set();
  const timers = new Map();
  const calls = [];
  const window = { isSecureContext: true, CropTarget: {},
    addEventListener(name) { listeners.add(name); }, removeEventListener(name) { listeners.delete(name); } };
  class Peer {
    constructor() { this.iceGatheringState = options.gathering ?? 'complete'; this.connectionState = 'new'; }
    addTrack() {}
    async setRemoteDescription() { calls.push('remote'); await options.remote; }
    async createAnswer() { calls.push('answer'); await options.answer; return { type: 'answer', sdp: 'answer' }; }
    async setLocalDescription(answer) { calls.push('local'); await options.local; this.localDescription = answer; }
    close() { calls.push('close'); this.connectionState = 'closed'; }
  }
  vm.runInNewContext(script, {
    window, location: { href: 'https://example.com', origin: 'https://example.com' },
    navigator: { mediaDevices: { async getDisplayMedia() { if (errorName && !cropError) throw failure; await options.media; return media; } } },
    document: { documentElement: {} }, CropTarget: { async fromElement() { return {}; } },
    RTCPeerConnection: Peer, innerWidth: 800, innerHeight: 600,
    setTimeout(callback, delay) { const id = {}; timers.set(id, { callback, delay }); return id; },
    clearTimeout(id) { timers.delete(id); },
  });
  await new Promise(setImmediate);
  return { state: window.capture, stopped: () => stopped, track, listeners, timers, calls };
}

test('capture reports an unfocused document separately from user denial', async () => {
  for (const name of ['InvalidStateError', 'NotAllowedError']) {
    const { state } = await capture(name);
    assert.equal(state.phase, 'capture');
    assert.equal(state.errorName, name);
    assert.equal(state.closed, true);
    assert.equal(state.answer, null);
  }
});

test('an invalid crop stays a capture error and stops the approved track', async () => {
  const { state, stopped } = await capture('InvalidStateError', true);
  assert.equal(state.phase, 'crop');
  assert.equal(state.errorName, 'InvalidStateError');
  assert.equal(stopped(), 1);
});

test('approved native capture returns its answer and cleans up on stop', async () => {
  const { state, stopped } = await capture();
  assert.equal(state.answer.sdp, 'answer');
  assert.equal(state.answer.width, 800);
  assert.equal(state.answer.height, 600);
  assert.equal(state.error, null);
  state.stop();
  assert.equal(stopped(), 1);
  assert.equal(state.peer.connectionState, 'closed');
});

function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

test('a late capture permission result releases the track after cancellation', async () => {
  const pending = deferred();
  const { state, stopped, timers, listeners } = await capture(null, false, { media: pending.promise });
  state.stop();
  pending.resolve();
  await new Promise(setImmediate);
  assert.equal(stopped(), 1);
  assert.equal(state.peer, null);
  assert.equal(state.answer, null);
  assert.equal(timers.size, 0);
  assert.equal(listeners.size, 0);
});

for (const phase of ['crop', 'remote', 'answer', 'local']) {
  test(`cancellation during ${phase} cannot publish an answer or resume negotiation`, async () => {
    const pending = deferred();
    const { state, stopped, calls, timers, listeners } = await capture(null, false, { [phase]: pending.promise });
    state.stop();
    const before = calls.slice();
    pending.resolve();
    await new Promise(setImmediate);
    assert.equal(state.answer, null);
    assert.deepEqual(calls, before);
    assert.equal(stopped(), 1);
    assert.equal(timers.size, 0);
    assert.equal(listeners.size, 0);
  });
}

test('stop cancels ICE gathering without leaving a deadline or publishing an answer', async () => {
  const { state, stopped, timers } = await capture(null, false, { gathering: 'gathering' });
  assert.equal(timers.size, 1);
  state.stop();
  await new Promise(setImmediate);
  assert.equal(timers.size, 0);
  assert.equal(state.answer, null);
  assert.equal(stopped(), 1);
});

test('ICE gathering timeout releases capture and its timer', async () => {
  const { state, stopped, timers } = await capture(null, false, { gathering: 'gathering' });
  [...timers.values()][0].callback();
  await new Promise(setImmediate);
  assert.match(state.error, /ICE gathering timed out/);
  assert.equal(state.answer, null);
  assert.equal(state.closed, true);
  assert.equal(stopped(), 1);
  assert.equal(timers.size, 0);
});

test('completed ICE replaces the gathering deadline with the connection deadline', async () => {
  const { state, timers } = await capture(null, false, { gathering: 'gathering' });
  state.peer.iceGatheringState = 'complete';
  state.peer.onicegatheringstatechange();
  await new Promise(setImmediate);
  assert.equal(state.answer.sdp, 'answer');
  assert.deepEqual([...timers.values()].map(timer => timer.delay), [20000]);
  assert.equal(state.peer.onicegatheringstatechange, null);
  state.stop();
  assert.equal(timers.size, 0);
});

test('stopping an established capture is idempotent and cancels its deadline', async () => {
  const { state, stopped, timers, listeners, calls } = await capture();
  assert.equal(timers.size, 1);
  state.stop();
  state.stop();
  assert.equal(stopped(), 1);
  assert.equal(calls.filter(call => call === 'close').length, 1);
  assert.equal(timers.size, 0);
  assert.equal(listeners.size, 0);
});

test('ending the shared track during crop cancels negotiation', async () => {
  const pending = deferred();
  const { state, track, timers } = await capture(null, false, { crop: pending.promise });
  assert.equal(typeof track.onended, 'function');
  track.onended();
  pending.resolve();
  await new Promise(setImmediate);
  assert.equal(state.closed, true);
  assert.equal(state.peer, null);
  assert.equal(state.answer, null);
  assert.equal(timers.size, 0);
});
