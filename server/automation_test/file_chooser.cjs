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
      behavior: 'allow', downloadPath: downloads,
      ...(targetInfo.browserContextId ? { browserContextId: targetInfo.browserContextId } : {}),
    });
  }
  if (fixture) {
    await call('Page.enable');
    await call('Page.setInterceptFileChooserDialog', { enabled: true });
  }
}

function attach(target) {
  if (clients.has(target.id) || !target.webSocketDebuggerUrl) return;
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  clients.set(target.id, socket);
  let sequence = 0;
  const pending = new Map();
  function call(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++sequence;
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 5000);
      pending.set(id, { resolve, reject, timer });
      socket.send(JSON.stringify({ id, method, params }));
    });
  }
  socket.onmessage = async ({ data }) => {
    const message = JSON.parse(data);
    if (message.id) {
      const item = pending.get(message.id);
      if (!item) return;
      clearTimeout(item.timer); pending.delete(message.id);
      message.error ? item.reject(new Error(message.error.message)) : item.resolve(message.result);
    } else if (message.method === 'Page.fileChooserOpened') {
      try {
        if (!message.params.backendNodeId) throw new Error('File chooser has no input node');
        await call('DOM.setFileInputFiles', { files: [fixture], backendNodeId: message.params.backendNodeId });
        console.log(JSON.stringify({ event: 'selected', file: path.basename(fixture) }));
      } catch (error) { console.error(`File chooser failed: ${error.message}`); }
    }
  };
  socket.onopen = async () => {
    try {
      await configurePage(call, fixture, downloads);
      if (downloads) console.log(JSON.stringify({ event: 'downloads-ready', target: target.id }));
    } catch (error) {
      console.error(`File chooser setup failed: ${error.message}`);
      socket.close(); // Let the watcher retry transient attachment failures.
    }
  };
  socket.onerror = () => {};
  socket.onclose = () => {
    clients.delete(target.id);
    for (const item of pending.values()) { clearTimeout(item.timer); item.reject(new Error('Page closed')); }
  };
}

async function watch() {
  while (!stopping) {
    try {
      const port = Number(fs.readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0]);
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(1000) });
      for (const target of await response.json()) if (target.type === 'page') attach(target);
    } catch (_) { /* Chrome starts when the first YAML step boots it. */ }
    await new Promise(resolve => setTimeout(resolve, 100));
  }
}

if (require.main === module) {
  watch().catch(error => { console.error(error.message); process.exitCode = 1; });
}
module.exports = { configurePage };
