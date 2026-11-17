'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { once } = require('node:events');
const { WebSocket } = require('ws');
const { createApp } = require('./server');

test('sessions isolate accounts and authenticated peers receive SSE and WebSocket messages', async t => {
 const app = createApp({ users: { alice: 'alice-demo', bob: 'bob-demo', eve: 'eve-demo' } }); await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve)); t.after(() => app.close());
 const base = `http://127.0.0.1:${app.server.address().port}`;
 const request = (path, cookie, method = 'GET', data) => fetch(base + path, { method, headers: { ...(cookie ? { cookie } : {}), ...(data ? { 'content-type': 'application/json' } : {}) }, ...(data ? { body: JSON.stringify(data) } : {}) });
 assert.equal((await request('/api/me')).status, 401);
 assert.equal((await request('/api/login', null, 'POST', { username: 'alice', password: 'bad' })).status, 401);
 async function login(username) { const r = await request('/api/login', null, 'POST', { username, password: `${username}-demo` }); assert.equal(r.status, 200); return r.headers.get('set-cookie').split(';')[0]; }
 const alice = await login('alice'), bob = await login('bob'), eve = await login('eve'); assert.notEqual(alice, bob);
 assert.equal((await (await request('/api/me', alice)).json()).user, 'alice'); assert.equal((await (await request('/api/me', bob)).json()).user, 'bob');
 assert.deepEqual((await (await request('/api/contacts?q=bo', alice)).json()).contacts, ['bob']);
 assert.equal((await request('/api/conversations/alice:bob/messages', alice)).status, 404);
 assert.equal((await request('/api/conversations/alice:bob/messages', alice, 'POST', { text: 'too early' })).status, 404);
 const a = await (await request('/api/conversations', alice, 'POST', { contact: 'bob' })).json();
 const again = await (await request('/api/conversations', alice, 'POST', { contact: 'bob' })).json(); assert.equal(a.id, again.id);
 assert.equal((await request(`/api/conversations/${a.id}/messages`, eve)).status, 404);
 assert.equal((await request(`/api/conversations/${a.id}/messages`, eve, 'POST', { text: 'unauthorized' })).status, 404);
 const b = await (await request('/api/conversations', bob, 'POST', { contact: 'alice' })).json(); assert.equal(a.id, b.id);
 const other = await (await request('/api/conversations', eve, 'POST', { contact: 'alice' })).json(); assert.notEqual(a.id, other.id);
 assert.equal((await request(`/api/conversations/${other.id}/messages`, bob)).status, 404);
 assert.equal((await request('/api/conversations', bob, 'POST', { contact: 'bob' })).status, 400);
 assert.equal((await request('/api/conversations/other/messages', alice)).status, 404);
 const abort = new AbortController(); const stream = await fetch(base + '/api/events', { headers: { cookie: bob }, signal: abort.signal }); const reader = stream.body.getReader(); await reader.read();
 const ws = new WebSocket(base.replace('http:', 'ws:') + '/api/socket', { headers: { cookie: bob } }); t.after(() => { abort.abort(); ws.terminate(); });
 const [ready] = await once(ws, 'message'); assert.equal(JSON.parse(ready).user, 'bob');
 const received = once(ws, 'message');
 const sent = await request(`/api/conversations/${a.id}/messages`, alice, 'POST', { text: 'hello Bob' }); assert.equal(sent.status, 201);
 const [frame] = await received; assert.equal(JSON.parse(frame).message.text, 'hello Bob');
 const sse = new TextDecoder().decode((await reader.read()).value); assert.match(sse, /hello Bob/);
 assert.equal((await request(`/api/conversations/${a.id}/messages`, bob, 'POST', { text: '' })).status, 400);
 await request(`/api/conversations/${a.id}/messages`, bob, 'POST', { text: 'hello Alice' });
 const history = (await (await request(`/api/conversations/${a.id}/messages`, alice)).json()).messages; assert.deepEqual(history.map(m => m.sender), ['alice', 'bob']);
 const closed = once(ws, 'close'); await request('/api/logout', bob, 'POST', {}); await closed;
 assert.equal((await request('/api/me', bob)).status, 401); assert.equal((await request('/api/me', alice)).status, 200);
});

test('websocket rejects anonymous connections and cleanup closes open clients', async () => {
 const app = createApp(); await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
 const base = `http://127.0.0.1:${app.server.address().port}`;
 const ws = new WebSocket(base.replace('http:', 'ws:') + '/api/socket'); ws.on('error', () => {}); const [, response] = await once(ws, 'unexpected-response'); assert.equal(response.statusCode, 401); ws.terminate(); await app.close();
});

test('shutdown closes authenticated event connections', async () => {
 const app = createApp(); await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
 const base = `http://127.0.0.1:${app.server.address().port}`;
 const login = await fetch(base + '/api/login', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ username: 'alice', password: 'alice-demo' }) });
 const cookie = login.headers.get('set-cookie').split(';')[0];
 const stream = await fetch(base + '/api/events', { headers: { cookie } }); const reader = stream.body.getReader(); await reader.read();
 const ws = new WebSocket(base.replace('http:', 'ws:') + '/api/socket', { headers: { cookie } }); await once(ws, 'message');
 const closed = once(ws, 'close'); await app.close(); await closed; assert.equal((await reader.read()).done, true);
});
