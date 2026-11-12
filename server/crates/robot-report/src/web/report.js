'use strict';
const ROOT = location.pathname.startsWith('/reports') ? '/reports' : '';
const $ = id => document.getElementById(id);
const state = { projects: [], project: null, runs: [], run: null, selectedCase: null, canManage: false, tab: 'steps', kind: 'video', busy: false, generation: 0 };
const labels = { PASS: 'PASS', PARTIAL: 'PARTIAL', FAIL: 'FAIL', DRY_RUN: 'CHƯA CHẠY', 'NOT RUN': 'CHƯA CHẠY' };
const actions = { select_device: 'Chọn thiết bị', launch_app: 'Mở ứng dụng / trang web', start_screen_recording: 'Bắt đầu ghi hình', stop_screen_recording: 'Kết thúc ghi hình', assert_element: 'Kiểm tra nội dung hiển thị', assert_absent: 'Kiểm tra nội dung đã ẩn', assert_state: 'Kiểm tra trạng thái', assert_value: 'Kiểm tra giá trị', assert_count: 'Kiểm tra số phần tử', tap_element: 'Chạm control', focus_field: 'Đặt focus vào ô nhập', fill_field: 'Điền / thay dữ liệu', type_text: 'Nhập văn bản', gesture: 'Thao tác cử chỉ', scroll: 'Cuộn màn hình', screenshot: 'Chụp ảnh evidence', call_tool: 'Gọi công cụ kiểm thử', if_element: 'Kiểm tra điều kiện', note_limitation: 'Ghi nhận giới hạn coverage', reset_fields: 'Đặt lại dữ liệu theo dõi', wait: 'Chờ trạng thái', tap: 'Chạm tọa độ', swipe: 'Vuốt màn hình' };
function node(tag, className, text) { const e = document.createElement(tag); if (className) e.className = className; if (text !== undefined) e.textContent = text; return e; }
function status(report) { return report.error || report.steps.some(s => s.status === 'failed') ? 'FAIL' : report.dry_run ? 'DRY_RUN' : report.status; }
function errorHint(report, failed) {
  const message = `${failed?.error || ''} ${report.error || ''}`.toLowerCase();
  if (message.includes('ambiguous')) return 'Automation tìm thấy nhiều phần tử phù hợp và dừng để tránh chọn nhầm.';
  if (message.includes('ime') && message.includes('not available')) return 'Không tìm thấy phím IME được yêu cầu trên bàn phím hiện tại.';
  if (message.includes('timed out') || message.includes('timeout')) return 'Công cụ không phản hồi hoặc giao diện chưa đạt trạng thái mong đợi trong thời gian chờ.';
  if (message.includes('missing') || message.includes('unstable')) return 'Không xác nhận được nội dung hoặc trạng thái giao diện trước khi hết thời gian chờ.';
  if (report.error_kind === 'setup') return 'Lượt chạy chưa vượt qua bước chuẩn bị môi trường.';
  if (report.error_kind === 'assertion') return 'Kết quả quan sát được chưa khớp kiểm tra mong đợi.';
  return 'Bước kiểm thử chưa hoàn tất. Đối chiếu lỗi gốc với ảnh, video và log bên dưới.';
}
function badge(value) { return node('span', `badge ${value}`, labels[value] || value); }
function date(ms) { return ms ? new Intl.DateTimeFormat('vi-VN', { dateStyle: 'short', timeStyle: 'short' }).format(new Date(ms)) : 'Không có thời điểm'; }
function size(bytes) { return bytes > 1048576 ? `${(bytes / 1048576).toFixed(1)} MB` : `${(bytes / 1024).toFixed(1)} KB`; }
function duration(seconds) { if (seconds == null) return 'Chưa có thời lượng'; return seconds >= 60 ? `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s` : `${seconds.toFixed(1)}s`; }
function notice(message) { $('notice').textContent = message; $('notice').hidden = !message; }
async function api(path, body) {
  const response = await fetch(ROOT + path, body === undefined ? {} : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  const data = await response.json();
  if (!response.ok) throw new Error(data.error || `Yêu cầu thất bại (${response.status})`);
  return data;
}
function route() { return new URLSearchParams(location.hash.slice(1)); }
function remember() {
  const params = new URLSearchParams();
  if (state.project) params.set('project', state.project.id);
  if (state.run) params.set('run', state.run.id);
  if (state.selectedCase !== null) params.set('case', state.selectedCase);
  history.replaceState(null, '', `#${params}`);
}
function renderProjects() {
  $('project-count').textContent = state.projects.length;
  $('projects').replaceChildren(...state.projects.map(project => {
    const b = node('button', `project-button${project.id === state.project?.id ? ' active' : ''}`);
    b.setAttribute('aria-current', project.id === state.project?.id ? 'page' : 'false');
    b.append(node('span', 'project-initial', project.name.slice(0, 1).toUpperCase()), node('span', '', project.name));
    b.onclick = () => chooseProject(project.id).catch(showError);
    return b;
  }));
  document.querySelectorAll('.manage').forEach(e => { e.hidden = !state.canManage; });
  $('import-run').hidden = !state.canManage || !state.project;
}
function renderHistory() {
  $('run-count').textContent = state.runs.length;
  $('no-runs').hidden = !!state.runs.length;
  $('history').replaceChildren(...state.runs.map(run => {
    const row = node('tr', run.id === state.run?.id ? 'selected' : '');
    const name = node('td'); const b = node('button', 'run-link', run.name);
    b.onclick = () => chooseRun(run.id).catch(showError); name.append(b); row.append(name, node('td', 'muted', date(run.lastReportAt)));
    for (const [key, cl] of [['passed', 'pass'], ['partial', 'partial'], ['failed', 'fail'], ['notRun', 'muted']]) row.append(node('td', cl, run.counts[key]));
    return row;
  }));
}
async function chooseProject(id, runId, caseId) {
  const generation = ++state.generation;
  state.project = state.projects.find(p => p.id === id) || null;
  state.run = null; state.runs = []; state.selectedCase = null;
  renderProjects();
  $('workspace').hidden = !state.project; $('empty').hidden = !!state.project; $('run-content').hidden = true;
  if (!state.project) return;
  $('project-title').textContent = state.project.name;
  $('project-subtitle').textContent = 'Lịch sử từng lượt chạy · Kết quả, bước lỗi và evidence trong cùng một nơi.';
  const runs = await api(`/api/projects/${state.project.id}/runs`);
  if (generation !== state.generation) return;
  state.runs = runs; renderHistory();
  if (runs.length) await chooseRun(runs.some(r => r.id === runId) ? runId : runs[0].id, caseId);
  else remember();
}
async function chooseRun(id, requestedCase) {
  const projectId = state.project.id;
  const generation = ++state.generation;
  const run = await api(`/api/projects/${projectId}/runs/${id}`);
  if (generation !== state.generation) return;
  state.run = run;
  state.tab = 'steps'; state.kind = 'video';
  state.selectedCase = run.cases.some(c => c.id === Number(requestedCase)) && requestedCase != null ? Number(requestedCase) : (run.cases.find(c => status(c.report) === 'FAIL') || run.cases[0])?.id ?? null;
  $('search').value = ''; $('status-filter').value = 'all';
  $('run-content').hidden = false;
  $('run-title').textContent = run.name;
  $('run-meta').textContent = `Kết quả: ${date(run.lastReportAt)} · Lưu vào history: ${date(run.importedAt)} · ${run.cases.length} case`;
  $('download-run').href = `${ROOT}/api/projects/${projectId}/runs/${id}`;
  renderHistory(); renderStats(); renderCaseList(); renderCase(); remember(); notice('');
}
function renderStats() {
  const counts = state.run.counts;
  $('stats').replaceChildren(...[
    ['total', '', 'TỔNG CASE', 'Trong lượt đang xem'], ['passed', 'pass', 'PASS · ĐẠT', 'Đạt kiểm tra đã khai báo'],
    ['partial', 'partial', 'PARTIAL · GIỚI HẠN', 'Còn phần chưa được phủ'], ['failed', 'fail', 'FAIL · CÓ LỖI', 'Xem bước lỗi & evidence'], ['notRun', '', 'CHƯA CHẠY', 'Dry-run / not run'],
  ].map(([key, cl, label, note]) => { const e = node('div', `stat ${cl}`); e.append(node('div', 'stat-label', label), node('div', 'stat-value', counts[key]), node('div', 'stat-note', note)); return e; }));
  $('run-warnings').hidden = !state.run.warnings.length;
  $('warning-list').replaceChildren(...state.run.warnings.map(w => node('li', '', w)));
}
function renderCaseList() {
  if (!state.run) return;
  const query = $('search').value.trim().toLowerCase(); const filter = $('status-filter').value;
  const cases = state.run.cases.filter(c => (filter === 'all' || (filter === 'DRY_RUN' ? ['DRY_RUN', 'NOT RUN'].includes(status(c.report)) : status(c.report) === filter)) && `${c.report.name} ${c.report.error || ''} ${c.report.steps.map(s => s.error || '').join(' ')}`.toLowerCase().includes(query));
  $('shown-count').textContent = `${cases.length} / ${state.run.cases.length} case`;
  $('case-list').replaceChildren(...cases.map(c => {
    const b = node('button', `case-row${c.id === state.selectedCase ? ' active' : ''}`);
    b.setAttribute('aria-pressed', c.id === state.selectedCase);
    const top = node('div', 'case-row-top'); top.append(node('span', 'case-name', c.report.name), badge(status(c.report)));
    const meta = node('div', 'case-meta'); meta.append(node('span', '', `${c.report.steps.filter(s => s.status === 'passed').length}/${c.report.steps.length} bước đạt`), node('span', '', duration(c.durationSeconds)));
    b.append(top, meta); b.onclick = () => { state.selectedCase = c.id; state.tab = 'steps'; renderCaseList(); renderCase(); remember(); };
    return b;
  }));
  if (!cases.length) $('case-list').append(node('p', 'empty-inline', 'Không có case khớp bộ lọc.'));
}
function fileUrl(c, file, download = false) { return `${ROOT}/api/projects/${state.project.id}/runs/${state.run.id}/cases/${c.id}/files/${file.id}${download ? '?download=true' : ''}`; }
function downloadLink(c, file, text = 'Tải xuống') { const a = node('a', '', text); a.href = fileUrl(c, file, true); a.download = file.name.split('/').pop(); return a; }
function renderCase() {
  const detail = $('case-detail'); detail.replaceChildren();
  const c = state.run?.cases.find(c => c.id === state.selectedCase);
  if (!c) { detail.append(node('p', 'empty-inline', 'Chọn case để xem chi tiết.')); return; }
  const report = c.report;
  const head = node('div', 'detail-head'); const title = node('div', 'detail-title'); title.append(node('h2', '', report.name), badge(status(report)));
  const device = report.selected_device;
  head.append(title, node('div', 'device-meta', device ? `${device.name || 'Thiết bị'} · ${device.platform || ''} · ${device.id || ''}` : 'Chưa ghi nhận thiết bị · kiểm tra lỗi setup hoặc dry-run'));
  detail.append(head);
  const failed = report.steps.filter(s => s.status === 'failed');
  if (report.error || failed.length) {
    const error = node('section', 'error-panel'); error.setAttribute('role', 'region'); error.setAttribute('aria-label', 'Bước lỗi');
    error.append(node('h3', '', failed.length ? `Lỗi tại ${failed[0].id}` : 'Lỗi trước khi có kết quả bước'));
    if (failed.length) error.append(node('div', '', actions[failed[0].action] || failed[0].action));
    error.append(node('p', '', errorHint(report, failed[0])));
    if (report.error_kind) error.append(node('div', 'small', `Nhóm lỗi: ${report.error_kind}`));
    error.append(node('pre', '', failed[0]?.error || report.error || 'Bước kiểm thử không đạt.'));
    if (report.error && report.error !== failed[0]?.error) { const more = node('details'); more.append(node('summary', '', 'Thông báo toàn case'), node('pre', '', report.error)); error.append(more); }
    detail.append(error);
  }
  if (report.limitations.length) {
    const limitations = node('details', 'limitation-panel'); limitations.open = true;
    const list = node('ul'); list.append(...report.limitations.map(l => node('li', '', l)));
    limitations.append(node('summary', '', 'Giới hạn coverage của case'), list); detail.append(limitations);
  }
  const tabs = node('div', 'tabs'); tabs.setAttribute('role', 'tablist'); tabs.setAttribute('aria-label', 'Nội dung case');
  for (const [id, label] of [['steps', `Các bước (${report.steps.length})`], ['evidence', `Evidence (${c.files.length})`]]) {
    const b = node('button', 'tab', label); b.id = `tab-${id}`; b.setAttribute('role', 'tab'); b.setAttribute('aria-selected', state.tab === id); b.setAttribute('aria-controls', 'detail-panel');
    b.onclick = () => { state.tab = id; renderCase(); $(`tab-${id}`).focus(); }; tabs.append(b);
  }
  detail.append(tabs);
  const panel = node('div', 'panel-body'); panel.id = 'detail-panel'; panel.setAttribute('role', 'tabpanel'); panel.setAttribute('aria-labelledby', `tab-${state.tab}`);
  if (state.tab === 'steps') renderSteps(c, panel); else renderEvidence(c, panel);
  detail.append(panel);
}
function renderSteps(c, panel) {
  const summary = node('div', 'step-summary');
  summary.append(node('span', '', `${c.report.steps.filter(s => s.status === 'passed').length} bước đạt · ${c.report.steps.filter(s => s.status === 'failed').length} bước lỗi`));
  const label = node('label'); const check = node('input'); check.type = 'checkbox'; label.append(check, document.createTextNode('Chỉ xem bước lỗi')); summary.append(label); panel.append(summary);
  const table = node('table', 'step-table'); table.innerHTML = '<thead><tr><th>#</th><th>Bước / ID</th><th>Thao tác</th><th>Kết quả</th></tr></thead>';
  const body = node('tbody'); table.append(body); panel.append(table);
  function rows() {
    body.replaceChildren(...c.report.steps.map((step, i) => ({ step, i })).filter(({ step }) => !check.checked || step.status === 'failed').map(({ step, i }) => {
      const tr = node('tr', step.status === 'failed' ? 'step-failed' : ''); const id = node('td'); id.append(node('code', '', step.id));
      const action = node('td', '', actions[step.action] || step.action);
      const result = node('td', `step-status ${step.status}`, ({ passed: 'Đạt', failed: 'Lỗi', omitted: 'Đã bỏ qua', skipped: 'Chưa chạy' })[step.status] || step.status);
      if (step.error) { const error = node('pre', 'step-error', step.error); action.append(error); }
      tr.append(node('td', '', i + 1), id, action, result); return tr;
    }));
    if (!body.children.length) { const tr = node('tr'); const td = node('td', 'muted', check.checked ? 'Không có bước lỗi.' : 'Chưa có bước thực thi được ghi nhận.'); td.colSpan = 4; tr.append(td); body.append(tr); }
  }
  check.onchange = rows; rows();
}
function renderEvidence(c, panel) {
  const groups = [['video', 'Video'], ['image', 'Ảnh'], ['log', 'Log'], ['data', 'JSON / XML']];
  if (!c.files.some(f => f.kind === state.kind)) state.kind = groups.find(([kind]) => c.files.some(f => f.kind === kind))?.[0] || 'video';
  const controls = node('div', 'kind-tabs');
  for (const [kind, label] of groups) {
    const b = node('button', 'kind-filter', `${label} (${c.files.filter(f => f.kind === kind).length})`); b.disabled = !c.files.some(f => f.kind === kind); b.setAttribute('aria-pressed', state.kind === kind);
    b.onclick = () => { state.kind = kind; renderCase(); }; controls.append(b);
  }
  panel.append(controls);
  const files = c.files.filter(f => f.kind === state.kind);
  if (!files.length) { panel.append(node('p', 'empty-inline', 'Không có evidence loại này trong snapshot.')); return; }
  if (state.kind === 'image') {
    const grid = node('div', 'image-grid');
    for (const file of files) {
      const card = node('article', 'evidence-card'); const button = node('button', 'image-button'); button.setAttribute('aria-label', `Phóng to ${file.name}`);
      const img = node('img'); img.src = fileUrl(c, file); img.alt = file.name; img.loading = 'lazy'; button.append(img);
      button.onclick = () => { $('image-preview').src = img.src; $('image-preview').alt = file.name; $('image-title').textContent = file.name; $('image-download').href = fileUrl(c, file, true); $('image-download').download = file.name; $('image-dialog').showModal(); };
      const caption = node('div', 'evidence-caption'); const name = node('div'); name.append(node('strong', '', file.name), node('span', 'muted', size(file.bytes))); caption.append(name, downloadLink(c, file, 'Tải ảnh'));
      card.append(button, caption); grid.append(card);
    }
    panel.append(grid);
  } else if (state.kind === 'video') {
    for (const file of files) {
      const card = node('article', 'evidence-card'); const video = node('video'); video.controls = true; video.preload = 'metadata'; video.src = fileUrl(c, file); video.setAttribute('aria-label', `Video ${file.name}`);
      const caption = node('div', 'evidence-caption'); const name = node('div'); name.append(node('strong', '', file.name), node('span', 'muted', `${size(file.bytes)} · Có thể tải để xem bằng trình phát trên máy.`));
      video.onerror = () => { name.append(node('p', 'muted', 'Trình duyệt không phát được định dạng video này. Dùng Tải video để xem bản gốc.')); };
      caption.append(name, downloadLink(c, file, 'Tải video')); card.append(video, caption); panel.append(card);
    }
  } else {
    const logArea = node('div');
    for (const file of files) {
      const row = node('div', 'file-row'); const name = node('div', 'file-name'); name.append(node('strong', '', file.name), node('span', 'muted', size(file.bytes)));
      const links = node('div', 'file-actions'); const view = node('button', 'link-button', 'Xem');
      view.onclick = async () => {
        view.disabled = true;
        try {
          if (file.bytes === 0) { logArea.replaceChildren(node('div', 'log-title', file.name), node('pre', 'log-preview', 'Tệp rỗng.')); return; }
          const response = await fetch(fileUrl(c, file), { headers: { Range: 'bytes=0-65535' } });
          if (!response.ok) throw new Error('Không đọc được evidence.');
          logArea.replaceChildren(node('div', 'log-title', `${file.name} · Xem tối đa 64 KB; tải xuống để lấy toàn bộ.`), node('pre', 'log-preview', await response.text()));
        } catch (error) { logArea.replaceChildren(node('p', 'notice', error.message)); }
        finally { view.disabled = false; }
      };
      links.append(view, downloadLink(c, file)); row.append(name, links); panel.append(row);
    }
    panel.append(logArea);
  }
}
function showError(error) { notice(error.message || String(error)); }
async function refresh() {
  const projects = await api('/api/projects'); state.projects = projects; renderProjects();
  if (!state.project) {
    const params = route(); await chooseProject(projects.some(p => p.id === params.get('project')) ? params.get('project') : projects[0]?.id, params.get('run'), params.get('case'));
    return;
  }
  const id = state.project.id; const runs = await api(`/api/projects/${id}/runs`);
  if (state.project?.id !== id) return;
  state.runs = runs; renderHistory();
  if (!state.run && runs.length) await chooseRun(runs[0].id);
}
$('refresh').onclick = () => refresh().catch(showError);
$('search').oninput = renderCaseList; $('status-filter').onchange = renderCaseList;
function openProject() { $('project-form').reset(); $('project-form').querySelector('.form-error').textContent = ''; $('project-dialog').showModal(); }
$('add-project').onclick = openProject; $('empty-add').onclick = openProject;
$('import-run').onclick = () => { $('run-form').reset(); $('run-name').value = `Regression ${new Date().toLocaleString('vi-VN')}`; $('run-source').value = state.project.sourceDir; $('run-form').querySelector('.form-error').textContent = ''; $('run-dialog').showModal(); };
document.querySelectorAll('[data-close]').forEach(b => { b.onclick = () => b.closest('dialog').close(); });
function formHandler(id, submit) {
  $(id).onsubmit = async event => {
    event.preventDefault(); if (state.busy) return; state.busy = true;
    const form = event.target; const button = form.querySelector('button[type=submit]'); const original = button.textContent;
    button.disabled = true; button.textContent = 'Đang lưu…'; form.querySelector('.form-error').textContent = '';
    try { await submit(Object.fromEntries(new FormData(form))); form.closest('dialog').close(); notice(''); }
    catch (error) { form.querySelector('.form-error').textContent = error.message; }
    finally { state.busy = false; button.disabled = false; button.textContent = original; }
  };
}
formHandler('project-form', async body => { const project = await api('/api/projects', body); state.projects = await api('/api/projects'); await chooseProject(project.id); });
formHandler('run-form', async body => { const id = state.project.id; const run = await api(`/api/projects/${id}/runs`, body); state.runs = await api(`/api/projects/${id}/runs`); await chooseRun(run.id); });
window.addEventListener('hashchange', () => { const params = route(); chooseProject(params.get('project'), params.get('run'), params.get('case')).catch(showError); });
(async () => {
  try { const capability = await api('/api/capabilities'); state.canManage = capability.canManage; await refresh(); }
  catch (error) { showError(error); }
  setInterval(() => { if (!state.busy && !document.hidden) refresh().catch(showError); }, 10000);
})();
