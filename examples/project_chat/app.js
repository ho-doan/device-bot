'use strict';
const $ = id => document.getElementById(id);
let current, active, source, socket;
async function api(url, options) { const response = await fetch(url, options); const data = await response.json(); if (!response.ok) throw Error(data.error); return data; }
function post(url, value) { return api(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(value) }); }
function error(err) { $('status').textContent = err.message; }
function displayMessage(message) { if (document.getElementById(`message-${message.id}`)) return; const li = document.createElement('li'); li.id = `message-${message.id}`; li.textContent = `${message.sender}: ${message.text}`; $('messages').append(li); }
async function enter(user) {
 current = user; $('identity').textContent = `Signed in as ${user}`; $('login').hidden = true; $('chat').hidden = false; $('status').textContent = '';
 source = new EventSource('/api/events'); source.addEventListener('message', event => { const data = JSON.parse(event.data); if (active === data.message.conversation) displayMessage(data.message); }); source.onerror = () => { $('status').textContent = 'Event stream disconnected'; };
 socket = new WebSocket(`${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/api/socket`); socket.onmessage = event => { const data = JSON.parse(event.data); if (data.type === 'message' && active === data.message.conversation) displayMessage(data.message); };
}
$('login-form').onsubmit = async event => { event.preventDefault(); try { const data = await post('/api/login', { username: $('username').value, password: $('password').value }); await enter(data.user); } catch (e) { error(e); } };
$('logout').onclick = async () => { try { await post('/api/logout', {}); source.close(); socket.close(); active = null; current = null; $('chat').hidden = true; $('login').hidden = false; $('messages').replaceChildren(); $('contacts').replaceChildren(); $('message-form').hidden = true; $('conversation-title').textContent = 'No conversation selected'; } catch (e) { error(e); } };
$('search-form').onsubmit = async event => { event.preventDefault(); try { const data = await api(`/api/contacts?q=${encodeURIComponent($('search').value)}`); $('contacts').replaceChildren(); for (const contact of data.contacts) { const button = document.createElement('button'); button.textContent = `Open conversation with ${contact}`; button.onclick = async () => { try { const conversation = await post('/api/conversations', { contact }); active = conversation.id; $('conversation-title').textContent = `Conversation with ${contact}`; $('message-form').hidden = false; $('messages').replaceChildren(); const history = await api(`/api/conversations/${active}/messages`); history.messages.forEach(displayMessage); } catch (e) { error(e); } }; $('contacts').append(button); } } catch (e) { error(e); } };
$('message-form').onsubmit = async event => { event.preventDefault(); try { await post(`/api/conversations/${active}/messages`, { text: $('message').value }); $('message').value = ''; } catch (e) { error(e); } };
$('notifications').onclick = async () => { if (!('Notification' in window)) { $('permission-status').textContent = 'Notifications unavailable'; return; } try { const result = await Notification.requestPermission(); $('permission-status').textContent = `Notification permission: ${result}`; } catch (e) { error(e); } };
api('/api/me').then(data => enter(data.user)).catch(() => {});
