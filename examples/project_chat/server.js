'use strict';
const http = require('node:http');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { WebSocketServer, WebSocket } = require('ws');

function createApp({ users = { alice: 'alice-demo', bob: 'bob-demo' } } = {}) {
  const sessions = new Map(), streams = new Map(), sockets = new Map();
  const conversations = new Map(), pairs = new Map();
  function user(req) {
    const token = (req.headers.cookie || '').split(';').map(v => v.trim()).find(v => v.startsWith('session='))?.slice(8);
    return sessions.get(token);
  }
  function json(res, status, value, headers = {}) {
    res.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store', ...headers });
    res.end(JSON.stringify(value));
  }
  async function body(req) {
    let data = '';
    for await (const chunk of req) { data += chunk; if (data.length > 8192) throw Error('Body too large'); }
    return JSON.parse(data || '{}');
  }
  function broadcast(event) {
    const serialized = JSON.stringify(event);
    const participants = conversations.get(event.message.conversation).participants;
    for (const [token, set] of streams) if (participants.includes(sessions.get(token))) for (const res of set) res.write(`event: message\ndata: ${serialized}\n\n`);
    for (const [token, set] of sockets) if (participants.includes(sessions.get(token))) for (const ws of set) if (ws.readyState === WebSocket.OPEN) ws.send(serialized);
  }
  const server = http.createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://localhost');
      if (req.method === 'GET' && ['/', '/app.js', '/style.css'].includes(url.pathname)) {
        const file = url.pathname === '/' ? 'index.html' : url.pathname.slice(1);
        res.writeHead(200, { 'Content-Type': file.endsWith('.js') ? 'text/javascript' : file.endsWith('.css') ? 'text/css' : 'text/html', 'Cache-Control': 'no-store' });
        return res.end(fs.readFileSync(path.join(__dirname, file)));
      }
      if (req.method === 'POST' && url.pathname === '/api/login') {
        const data = await body(req);
        if (!Object.hasOwn(users, data.username) || users[data.username] !== data.password) return json(res, 401, { error: 'Invalid credentials' });
        const token = crypto.randomBytes(24).toString('hex'); sessions.set(token, data.username);
        return json(res, 200, { user: data.username }, { 'Set-Cookie': `session=${token}; HttpOnly; SameSite=Strict; Path=/` });
      }
      const current = user(req);
      if (!current) return json(res, 401, { error: 'Sign in required' });
      if (req.method === 'POST' && url.pathname === '/api/logout') {
        const token = (req.headers.cookie || '').split(';').map(v => v.trim()).find(v => v.startsWith('session='))?.slice(8);
        sessions.delete(token);
        for (const res of streams.get(token) || []) res.end();
        for (const ws of sockets.get(token) || []) ws.close(1000, 'Signed out');
        return json(res, 200, { ok: true }, { 'Set-Cookie': 'session=; Max-Age=0; HttpOnly; SameSite=Strict; Path=/' });
      }
      if (req.method === 'GET' && url.pathname === '/api/me') return json(res, 200, { user: current });
      if (req.method === 'GET' && url.pathname === '/api/contacts') return json(res, 200, { contacts: Object.keys(users).filter(u => u !== current && u.includes((url.searchParams.get('q') || '').toLowerCase())) });
      if (req.method === 'POST' && url.pathname === '/api/conversations') {
        const data = await body(req);
        if (!Object.hasOwn(users, data.contact) || data.contact === current) return json(res, 400, { error: 'Unknown contact' });
        const participants = [current, data.contact].sort();
        const pair = JSON.stringify(participants);
        let id = pairs.get(pair);
        if (!id) { id = crypto.randomUUID(); pairs.set(pair, id); conversations.set(id, { participants, messages: [] }); }
        return json(res, 200, { id, contact: data.contact });
      }
      const match = /^\/api\/conversations\/([^/]+)\/messages$/.exec(url.pathname);
      if (match) {
        const conversation = conversations.get(match[1]);
        if (!conversation || !conversation.participants.includes(current)) return json(res, 404, { error: 'Conversation not found' });
        if (req.method === 'GET') return json(res, 200, { messages: conversation.messages });
        if (req.method === 'POST') {
          const data = await body(req);
          if (typeof data.text !== 'string' || !data.text.trim() || data.text.length > 2000) return json(res, 400, { error: 'Message must be 1–2000 characters' });
          const message = { id: conversation.messages.length + 1, conversation: match[1], sender: current, text: data.text };
          conversation.messages.push(message); broadcast({ type: 'message', message }); return json(res, 201, message);
        }
      }
      if (req.method === 'GET' && url.pathname === '/api/events') {
        const token = (req.headers.cookie || '').split(';').map(v => v.trim()).find(v => v.startsWith('session='))?.slice(8);
        res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache', Connection: 'keep-alive' });
        res.write(': connected\n\n');
        if (!streams.has(token)) streams.set(token, new Set()); streams.get(token).add(res);
        req.on('close', () => { streams.get(token)?.delete(res); if (!streams.get(token)?.size) streams.delete(token); });
        return;
      }
      json(res, 404, { error: 'Not found' });
    } catch (err) { if (!res.headersSent) json(res, 400, { error: err.message }); else res.destroy(); }
  });
  const wss = new WebSocketServer({ noServer: true });
  server.on('upgrade', (req, socket, head) => {
    const current = user(req);
    if (!current || req.url !== '/api/socket') { socket.end('HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n'); return; }
    const token = req.headers.cookie.split(';').map(v => v.trim()).find(v => v.startsWith('session=')).slice(8);
    wss.handleUpgrade(req, socket, head, ws => {
      if (!sockets.has(token)) sockets.set(token, new Set()); sockets.get(token).add(ws);
      ws.on('error', () => {}); ws.on('close', () => { sockets.get(token)?.delete(ws); if (!sockets.get(token)?.size) sockets.delete(token); });
      ws.send(JSON.stringify({ type: 'ready', user: current }));
    });
  });
  return { server, close: () => { for (const set of streams.values()) for (const res of set) res.end(); for (const ws of wss.clients) ws.terminate(); wss.close(); return new Promise(resolve => server.close(resolve)); } };
}
if (require.main === module) {
  const app = createApp(); const port = Number(process.env.PORT || 4173);
  app.server.listen(port, process.env.HOST || '127.0.0.1', () => console.log(`Project Chat listening at http://${process.env.HOST || '127.0.0.1'}:${port}`));
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => app.close().then(() => process.exit(0)));
}
module.exports = { createApp };
