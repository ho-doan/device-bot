// Fixture boundary only: select files after the YAML clicks the real Import UI.
// All browser connections belong to this run's disposable Chrome profile.
const fs = require('node:fs');
const path = require('node:path');
const [profile, fixture, downloads] = process.argv.slice(2);
const clients = new Map();
let stopping = false;
process.on('SIGTERM', () => { stopping = true; for (const socket of clients.values()) socket.close(); });

async function configurePage(call, fixture, downloads) {
  // Each flow owns an incognito context. Configure that context without requiring
  // Page.enable, which can fail while an old page is being detached.
  if (downloads) {
    const { targetInfo } = await call('Target.getTargetInfo');
    await call('Browser.setDownloadBehavior', {
      behavior: 'allow', downloadPath: downloads, eventsEnabled: true,
      ...(targetInfo.browserContextId ? { browserContextId: targetInfo.browserContextId } : {}),
    });
  }
  if (fixture) {
    await call('Page.enable');
    await call('Page.setInterceptFileChooserDialog', { enabled: true });
  }
}

// The browser pauses each new page until its own context is configured. Polling
// /json/list cannot guarantee setup precedes the YAML's first click.
async function configureAttachedTarget(call, params, fixture, downloads, log = event => console.log(JSON.stringify(event))) {
  const session = params.sessionId;
  const pageCall = (method, args) => call(method, args, session);
  try {
    await configurePage(pageCall, fixture, downloads);
    log({ event: 'target-ready', target: params.targetInfo.targetId });
  } finally {
    // Never leave an application paused when fixture setup fails.
    await pageCall('Runtime.runIfWaitingForDebugger');
  }
}

async function configureBrowser(call) {
  await call('Target.setAutoAttach', {
    autoAttach: true, waitForDebuggerOnStart: true, flatten: true,
    filter: [{ type: 'page', exclude: false }, { exclude: true }],
  });
}

function attachBrowser(url) {
  return new Promise(resolve => {
    const socket = new WebSocket(url);
    clients.set(url, socket);
    let sequence = 0;
    const pending = new Map();
    function call(method, params = {}, sessionId) {
      if (socket.readyState !== WebSocket.OPEN) return Promise.reject(new Error('Browser connection closed'));
      return new Promise((resolve, reject) => {
        const id = ++sequence;
        const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 5000);
        pending.set(id, { resolve, reject, timer });
        socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
      });
    }
    socket.onmessage = async ({ data }) => {
      const message = JSON.parse(data);
      if (message.id) {
        const item = pending.get(message.id);
        if (!item) return;
        clearTimeout(item.timer); pending.delete(message.id);
        message.error ? item.reject(new Error(message.error.message)) : item.resolve(message.result);
      } else if (message.method === 'Target.attachedToTarget') {
        try {
          await configureAttachedTarget(call, message.params, fixture, downloads);
        } catch (error) {
          console.error(`Fixture setup failed: ${error.message}`);
          socket.close();
        }
      } else if (message.method === 'Page.fileChooserOpened') {
        try {
          if (!message.params.backendNodeId) throw new Error('File chooser has no input node');
          await call('DOM.setFileInputFiles', { files: [fixture], backendNodeId: message.params.backendNodeId }, message.sessionId);
          console.log(JSON.stringify({ event: 'selected', file: path.basename(fixture) }));
        } catch (error) { console.error(`File chooser failed: ${error.message}`); }
      } else if (message.method === 'Browser.downloadProgress') {
        console.log(JSON.stringify({ event: 'download', ...message.params }));
      }
    };
    socket.onopen = async () => {
      try {
        await configureBrowser(call);
        console.log(JSON.stringify({ event: 'browser-ready' }));
      } catch (error) {
        console.error(`Browser fixture setup failed: ${error.message}`);
        socket.close();
      }
    };
    socket.onerror = () => {};
    socket.onclose = () => {
      clients.delete(url);
      for (const item of pending.values()) { clearTimeout(item.timer); item.reject(new Error('Browser connection closed')); }
      resolve();
    };
  });
}

async function watch() {
  while (!stopping) {
    try {
      const [port, browserPath] = fs.readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').trim().split(/\r?\n/);
      if (!port || !browserPath) throw new Error('Chrome is not ready');
      await attachBrowser(`ws://127.0.0.1:${Number(port)}${browserPath}`);
    } catch (_) { /* The harness bounds startup and reports missing readiness. */ }
    if (!stopping) await new Promise(resolve => setTimeout(resolve, 100));
  }
}

if (require.main === module) {
  watch().catch(error => { console.error(error.message); process.exitCode = 1; });
}
module.exports = { configurePage, configureAttachedTarget, configureBrowser };
