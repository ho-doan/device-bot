import { createInterface } from "node:readline";
import { readFileSync, realpathSync } from "node:fs";
import { pathToFileURL } from "node:url";

const referencesScript = readFileSync(new URL('./dom-references.js', import.meta.url), 'utf8');
const tapScript = readFileSync(new URL('./dom-tap.js', import.meta.url), 'utf8');
const editFieldScript = readFileSync(new URL('./dom-edit.js', import.meta.url), 'utf8');
const scrollScript = readFileSync(new URL('./dom-scroll.js', import.meta.url), 'utf8');

async function main() {
const input = createInterface({ input: process.stdin });
for await (const line of input) {
  if (!line.trim()) continue;
  let request;
  try {
    request = JSON.parse(line);
  } catch (error) {
    reply({ code: "invalid", error: "WebView helper request must be one JSON object" });
    break;
  }
  try {
    await handle(request);
  } catch (error) {
    reply({
      code: "inspect_unavailable",
      error: error instanceof Error ? error.message : String(error),
    });
  }
  break;
}
input.close();
}
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) await main();

function reply(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

async function handle(request) {
  let remoteDebugger;
  try {
    remoteDebugger = await import("appium-remote-debugger");
  } catch {
    reply({
      code: "helper_dependency_missing",
      error:
        "appium-remote-debugger@19.1.3 is not installed. Native accessibility stays available; an explicit web context does not fall back to it.",
    });
    return;
  }
  const bundleId = String(request.bundleId || "").trim();
  if (!bundleId) {
    reply({ code: "invalid", error: "web context needs appId for the iOS WebView" });
    return;
  }
  const realDevice = request.transport === "usbmux";
  const options = {
    bundleId,
    isSafari: false,
    pageLoadMs: 1000,
  };
  if (realDevice) {
    if (!request.udid) {
      reply({ code: "invalid", error: "real-device WebView inspect needs a device udid" });
      return;
    }
    options.udid = request.udid;
    options.transport = "usbmux";
  } else {
    options.socketPath = request.socketPath || (await simulatorSocket(request.udid));
    if (!options.socketPath) {
      reply({
        code: "inspect_unavailable",
        error:
          "Web Inspector socket for this simulator was not found. A development signature does not enable inspect.",
      });
      return;
    }
  }
  const remote = remoteDebugger.createRemoteDebugger(options, realDevice);
  let apps;
  try {
    apps = await remote.connect(4000);
  } catch (error) {
    reply({
      code: "inspect_unavailable",
      error: `WebKit inspect did not connect. A development signature does not enable it. ${error instanceof Error ? error.message : error}`,
    });
    return;
  }
  try {
    const pages = (await remote.selectApp(request.pageUrl || null, 2, false))
      .filter((page) => page.bundleId === bundleId);
    const page = choosePage(pages, request);
    if (page.error) {
      reply(page);
      return;
    }
    // selectApp returns compound IDs (PID.page). Do not pair a page from one
    // process with the first app entry or send the compound ID as pageId.
    const [pid, pageId, extra] = String(page.id).split('.');
    const appKey = `PID:${pid}`;
    if (extra || !pageId || (remote.appDict || apps)?.[appKey]?.bundleId !== bundleId) {
      reply({
        code: "not_found",
        error: "No inspectable WebView belongs to the selected app",
      });
      return;
    }
    await remote.selectPage(appKey, pageId, true);
    const framePath = request.webContext?.framePath || [];
    const script =
      request.action === "list"
        ? listScript(framePath, { appId: bundleId, page: {url: page.url, title: page.title} })
        : editScript(request.action, request.text || "", framePath, request.webContext?.elementRef);
    const result = await remote.execute(script, false);
    if (result && typeof result === "object" && result.error) {
      reply({ code: result.code || "not_found", error: result.error });
      return;
    }
    if (request.action === "list") {
      reply({ elements: Array.isArray(result) ? result : [] });
    } else {
      reply({ ok: true });
    }
  } finally {
    await remote.disconnect().catch(() => {});
  }
}

export function choosePage(pages, request) {
  const url = request.pageUrl || request.webContext?.page?.url;
  const title = request.pageTitle || request.webContext?.page?.title;
  let matches = pages;
  if (url) matches = matches.filter((page) => page.url === url);
  if (title) matches = matches.filter((page) => page.title === title);
  if (matches.length === 1) return matches[0];
  if (matches.length === 0) {
    return {
      code: "not_found",
      error: "No inspectable WebView page matches the selected app and page",
    };
  }
  return {
    code: "ambiguous",
    error: "More than one WebView page matches; set page url or title",
  };
}

function appKeyFor(apps, bundleId) {
  for (const [key, app] of Object.entries(apps || {})) {
    if (app?.bundleId === bundleId && app.isActive !== false) return key;
  }
  for (const [key, app] of Object.entries(apps || {})) {
    if (app?.bundleId === bundleId) return key;
  }
  return null;
}

export async function simulatorSocket(udid, resolveSocket) {
  if (!udid) return null;
  // Never select a global launchd_sim socket, even when only one is visible.
  const resolver = resolveSocket || (async (selectedUdid) => {
    const { NativeSimctl } = await import('@appium/coresim');
    return await new NativeSimctl().getWebInspectorSocket(selectedUdid);
  });
  return await resolver(udid);
}

export function listScript(framePath, context = {}) {
  const requested = JSON.stringify(framePath);
  return `(() => {
    const requested = ${requested};
    const context = ${JSON.stringify(context)};
    const references = ${referencesScript};
    const hopOf = (frame, position) => ({
      css: frame.id ? 'iframe#' + CSS.escape(frame.id) : null,
      name: frame.getAttribute('name') || null,
      url: frame.getAttribute('src') || null,
      position
    });
    const sameHop = (actual, expected) => (!expected.css || actual.css === expected.css)
      && (!expected.name || actual.name === expected.name)
      && (!expected.url || actual.url === expected.url)
      && (!expected.position || actual.position === expected.position);
    const read = (doc, path) => {
      const root = doc.body || doc.documentElement;
      if (!root) return [];
      const nodes = [...root.querySelectorAll('*')].slice(0, 500).map((node) => {
        const rect = node.getBoundingClientRect();
        const type = /^(INPUT|TEXTAREA)$/.test(node.tagName) || node.isContentEditable ? 'textField'
          : node.tagName === 'BUTTON' || node.getAttribute('role') === 'button' ? 'button'
          : node.tagName === 'A' ? 'link' : null;
        return {
          className: node.tagName.toLowerCase(),
          resourceId: node.id || null,
          text: (node.innerText || node.getAttribute('aria-label') || '').trim() || null,
          contentDescription: node.getAttribute('aria-label'),
          bounds: { left: Math.round(rect.left), top: Math.round(rect.top), right: Math.round(rect.right), bottom: Math.round(rect.bottom) },
          clickable: type === 'button' || type === 'link' || node.onclick != null,
          enabled: !node.disabled,
          focused: doc.activeElement === node,
          value: 'value' in node ? String(node.value ?? '') : null,
          elementType: type,
          webContext: { ...context, framePath: path, elementRef: references(doc).ref(node) }
        };
      });
      const frames = [...doc.querySelectorAll('iframe,frame')];
      for (let index = 0; index < frames.length; index += 1) {
        const hop = hopOf(frames[index], index + 1);
        const next = path.concat(hop);
        const requestedHere = requested.length >= next.length
          && requested.slice(0, next.length).every((expected, hopIndex) => sameHop(next[hopIndex], expected));
        try {
          const child = frames[index].contentDocument;
          if (!child) {
            if (requestedHere) return { error: 'The frame is cross-origin or not inspectable', code: 'not_found', webContext: { framePath: next } };
            continue;
          }
          const nested = read(child, next);
          if (nested && nested.error) {
            if (requestedHere) return nested;
            continue;
          }
          nodes.push(...nested);
        } catch {
          if (requestedHere) return { error: 'The frame is cross-origin or not inspectable', code: 'not_found', webContext: { framePath: next } };
        }
      }
      return nodes;
    };
    const found = read(document, []);
    if (found && found.error) return found;
    if (!requested.length) return found.filter((element) => element.webContext.framePath.length === 0);
    const matched = found.filter((element) => requested.every((hop, index) => sameHop(element.webContext.framePath[index] || {}, hop))
      && element.webContext.framePath.length === requested.length);
    if (!matched.length) return { error: 'The requested WebView frame was not found', code: 'not_found' };
    return matched;
  })()`;
}

export function editScript(action, text, framePath, elementRef = null) {
  const requested = JSON.stringify(framePath);
  const literal = JSON.stringify(text);
  return `(() => {
    const requested = ${requested};
    const open = (doc, path) => {
      if (path.length === requested.length) return doc;
      const frames = [...doc.querySelectorAll('iframe,frame')];
      const expected = requested[path.length];
      const matches = frames.map((frame, index) => ({ frame, hop: {
        css: frame.id ? 'iframe#' + CSS.escape(frame.id) : null,
        name: frame.getAttribute('name') || null,
        url: frame.getAttribute('src') || null,
        position: index + 1
      }})).filter(({ hop }) => (!expected.css || hop.css === expected.css)
        && (!expected.name || hop.name === expected.name)
        && (!expected.url || hop.url === expected.url)
        && (!expected.position || hop.position === expected.position));
      if (matches.length !== 1) return { error: matches.length ? 'The WebView frame is ambiguous; set an explicit one-based position' : 'The requested WebView frame was not found', code: matches.length ? 'ambiguous' : 'not_found' };
      try {
        const child = matches[0].frame.contentDocument;
        if (!child) return { error: 'The frame is cross-origin or not inspectable', code: 'not_found' };
        return open(child, path.concat(matches[0].hop));
      } catch {
        return { error: 'The frame is cross-origin or not inspectable', code: 'not_found' };
      }
    };
    const doc = open(document, []);
    if (doc && doc.error) return doc;
    const references = ${referencesScript};
    if (${JSON.stringify(action)} === 'tap') return (${tapScript})(doc, references, ${JSON.stringify(elementRef)});
    if (${JSON.stringify(action)} === 'scroll') {
      const delta = JSON.parse(${literal});
      return (${scrollScript})(doc, references, ${JSON.stringify(elementRef)}, delta.dx, delta.dy);
    }
    return (${editFieldScript})(doc, references, ${JSON.stringify(elementRef)}, ${JSON.stringify(action)}, ${literal});
  })()`;
}
