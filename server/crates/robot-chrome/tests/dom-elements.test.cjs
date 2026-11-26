const { readFileSync } = require('node:fs');
const { runInNewContext } = require('node:vm');
const { test } = require('node:test');
const assert = require('node:assert/strict');

const expression = readFileSync(`${__dirname}/../src/dom-elements.js`, 'utf8');
const getComputedStyle = element => element.style || { display: 'block', visibility: 'visible' };
async function snapshot(tag, attributes = {}, properties = {}) {
  const element = {
    tagName: tag.toUpperCase(),
    getAttribute: name => attributes[name] ?? null,
    getBoundingClientRect: () => ({ left: 10, top: 20, right: 110, bottom: 70 }),
    ...properties,
  };
  return (await runInNewContext(expression, {
    document: { querySelector: () => null, querySelectorAll: () => [element], activeElement: null },
    setTimeout,
    getComputedStyle,
  }))[0];
}

test('Flutter semantics button is clickable without an onclick property', async () => {
  const element = await snapshot('flt-semantics', { role: 'button' });
  assert.equal(element.elementType, 'button');
  assert.equal(element.clickable, true);
});

test('native controls and onclick handlers remain clickable', async () => {
  assert.equal((await snapshot('button')).clickable, true);
  assert.equal((await snapshot('div', {}, { onclick() {} })).clickable, true);
  assert.equal((await snapshot('div')).clickable, false);
});

test('Flutter popup menu rows expose a clickable action supported by YAML selectors', async () => {
  for (const role of ['menuitem', 'menuitemcheckbox', 'menuitemradio']) {
    const element = await snapshot('flt-semantics', { role, 'aria-label': 'Editor · pid 123' });
    assert.equal(element.elementType, 'button');
    assert.equal(element.clickable, true);
    assert.equal(element.contentDescription, 'Editor · pid 123');
  }
  const disabled = await snapshot('flt-semantics', { role: 'menuitem', 'aria-disabled': 'true' });
  assert.equal(disabled.enabled, false);
});

test('empty fields have an empty value; non-fields have no value', async () => {
  assert.equal((await snapshot('input', {}, { value: '' })).value, '');
  assert.equal((await snapshot('textarea', {}, { value: 'hello' })).value, 'hello');
  assert.equal((await snapshot('div')).value, null);
});

test('Flutter editors keep their semantic identifier when focus assigns a DOM id', async () => {
  const parentElement = { getAttribute: name => name === 'flt-semantics-identifier' ? 'email-field' : null };
  for (const id of ['', 'username']) {
    const editor = await snapshot('input', { 'data-semantics-role': 'text-field' }, { id, parentElement });
    assert.equal(editor.resourceId, 'email-field');
  }
});

test('ARIA checkbox state is observable without a native checked property', async () => {
  const checked = await snapshot('flt-semantics', { role: 'checkbox', 'aria-checked': 'true' });
  assert.equal(checked.elementType, 'checkBox');
  assert.equal(checked.clickable, true);
  assert.equal(checked.checked, true);
});

test('query enables Flutter accessibility and waits for fields; a later query does not click again', async () => {
  let ready = false, clicks = 0;
  const input = {
    tagName: 'INPUT', value: '',
    getAttribute: name => name === 'aria-label' ? 'Username' : null,
    getBoundingClientRect: () => ({ left: 10, top: 20, right: 110, bottom: 70 }),
  };
  const placeholder = { click() { clicks++; setTimeout(() => { ready = true; }, 10); } };
  const document = {
    querySelector: selector => selector.startsWith('flt-semantics-placeholder') ? (ready ? null : placeholder) : (ready ? input : null),
    querySelectorAll: () => ready ? [input] : [],
    activeElement: null,
  };
  for (let i = 0; i < 2; i++) {
    const elements = await runInNewContext(expression, { document, setTimeout, getComputedStyle });
    assert.equal(elements.length, 1);
    assert.equal(elements[0].elementType, 'textField');
    assert.equal(elements[0].contentDescription, 'Username');
  }
  assert.equal(clicks, 1);
});

test('a Flutter placeholder that never exposes semantics fails with a bounded setup error', async () => {
  let elapsed = 0;
  const document = {
    querySelector: selector => selector.startsWith('flt-semantics-placeholder') ? { click() {} } : null,
    querySelectorAll: () => [],
  };
  await assert.rejects(
    runInNewContext(expression, {
      document, getComputedStyle,
      Date: { now: () => elapsed },
      setTimeout: (callback, delay) => { elapsed += delay; callback(); },
    }),
    /Flutter Web accessibility did not become ready within 3500 ms/,
  );
  assert.equal(elapsed, 3500);
});

test('slow Flutter semantics setup is allowed to complete before the CDP deadline', async () => {
  let elapsed = 0;
  let clicks = 0;
  const placeholder = { click() { clicks++; } };
  const document = {
    querySelector: selector => selector.startsWith('flt-semantics-placeholder')
      ? placeholder : (elapsed >= 2200 ? {} : null),
    querySelectorAll: () => [],
  };
  await runInNewContext(expression, {
    document, getComputedStyle,
    Date: { now: () => elapsed },
    setTimeout: (callback, delay) => { elapsed += delay; callback(); },
  });
  assert.equal(clicks, 1);
  assert.equal(elapsed, 2200);
});

test('closed dialog controls and CSS-hidden elements are absent from snapshots', async () => {
  assert.equal(await snapshot('button', {}, {
    getBoundingClientRect: () => ({ left: 0, top: 0, right: 0, bottom: 0 }),
  }), undefined);
  assert.equal(await snapshot('button', {}, {
    style: { display: 'block', visibility: 'hidden' },
  }), undefined);
  assert.equal(await snapshot('button', {}, {
    style: { display: 'none', visibility: 'visible' },
  }), undefined);
});
