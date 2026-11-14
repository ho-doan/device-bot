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

test('empty fields have an empty value; non-fields have no value', async () => {
  assert.equal((await snapshot('input', {}, { value: '' })).value, '');
  assert.equal((await snapshot('textarea', {}, { value: 'hello' })).value, 'hello');
  assert.equal((await snapshot('div')).value, null);
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
  const document = {
    querySelector: selector => selector.startsWith('flt-semantics-placeholder') ? { click() {} } : null,
    querySelectorAll: () => [],
  };
  await assert.rejects(
    runInNewContext(expression, { document, setTimeout, getComputedStyle }),
    /Flutter Web accessibility did not become ready within 1500 ms/,
  );
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
