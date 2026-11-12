const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const script = fs.readFileSync(path.join(__dirname, '../src/live.js'), 'utf8')
  .replace('__KEY__', '"capture"').replace('__OFFER__', '"offer"');

async function capture(errorName, cropError = false) {
  const failure = Object.assign(new Error('Invalid state'), { name: errorName });
  let stopped = 0;
  const track = { stop() { stopped++; }, async cropTo() { if (cropError) throw failure; } };
  const media = { getTracks: () => [track], getVideoTracks: () => [track] };
  const window = { isSecureContext: true, CropTarget: {}, addEventListener() {}, removeEventListener() {} };
  class Peer {
    constructor() { this.iceGatheringState = 'complete'; this.connectionState = 'new'; }
    addTrack() {}
    async setRemoteDescription() {}
    async createAnswer() { return { type: 'answer', sdp: 'answer' }; }
    async setLocalDescription(answer) { this.localDescription = answer; }
    close() { this.connectionState = 'closed'; }
  }
  vm.runInNewContext(script, {
    window, location: { href: 'https://example.com', origin: 'https://example.com' },
    navigator: { mediaDevices: { async getDisplayMedia() { if (errorName && !cropError) throw failure; return media; } } },
    document: { documentElement: {} }, CropTarget: { async fromElement() { return {}; } },
    RTCPeerConnection: Peer, innerWidth: 800, innerHeight: 600, setTimeout() {}, clearTimeout() {},
  });
  await new Promise(setImmediate);
  return { state: window.capture, stopped: () => stopped };
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
