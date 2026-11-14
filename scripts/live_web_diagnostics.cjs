// Read-only telemetry from the disposable tab used by test_live_web.py.
const { once } = require('node:events');
const endpoint = process.argv[2];
const expression = `(async () => {
  const states = [];
  for (const key of Object.keys(window).filter(k => k.startsWith('__device_bot_live_'))) {
    const s = window[key];
    const track = s.media?.getVideoTracks()[0];
    const stats = s.peer ? [...(await s.peer.getStats()).values()].filter(v =>
      ['outbound-rtp', 'media-source', 'transport', 'codec'].includes(v.type)) : [];
    states.push({key, closed:s.closed, error:s.error, phase:s.phase, answer:!!s.answer,
      connection:s.peer?.connectionState, ice:s.peer?.iceConnectionState,
      track:track && {state:track.readyState, muted:track.muted, enabled:track.enabled,
        settings:track.getSettings()}, stats});
  }
  return {url:location.href, visibility:document.visibilityState, states};
})()`;

async function snapshot(target) {
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  try {
    await once(socket, 'open', {signal:AbortSignal.timeout(3000)});
    const reply = once(socket, 'message', {signal:AbortSignal.timeout(3000)});
    socket.send(JSON.stringify({id:1, method:'Runtime.evaluate',
      params:{expression, awaitPromise:true, returnByValue:true}}));
    const [message] = await reply;
    return JSON.parse(message.data).result?.result?.value;
  } finally { socket.close(); }
}

(async () => {
  while (true) {
    try {
      const targets = await (await fetch(`http://${endpoint}/json/list`,
        {signal:AbortSignal.timeout(3000)})).json();
      for (const target of targets.filter(t => t.type === 'page' && t.url.endsWith('/index.html'))) {
        console.log(JSON.stringify({time:new Date().toISOString(), ...await snapshot(target)}));
      }
    } catch (error) { console.log(JSON.stringify({error:String(error)})); }
    await new Promise(resolve => setTimeout(resolve, 1000));
  }
})();
