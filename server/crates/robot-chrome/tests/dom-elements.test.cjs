const { readFileSync } = require('node:fs');
const { runInNewContext } = require('node:vm');
const { test } = require('node:test');
const assert = require('node:assert/strict');

const expression = readFileSync(`${__dirname}/../src/dom-elements.js`, 'utf8');
function snapshot(tag, attributes = {}, properties = {}) {
  const element = {
    tagName: tag.toUpperCase(),
    getAttribute: name => attributes[name] ?? null,
    getBoundingClientRect: () => ({ left: 10, top: 20, right: 110, bottom: 70 }),
    ...properties,
  };
  return runInNewContext(expression, {
    document: { querySelectorAll: () => [element], activeElement: null },
  })[0];
}

test('Flutter semantics button is clickable without an onclick property', () => {
  const element = snapshot('flt-semantics', { role: 'button' });
  assert.equal(element.elementType, 'button');
  assert.equal(element.clickable, true);
});

test('native controls and onclick handlers remain clickable', () => {
  assert.equal(snapshot('button').clickable, true);
  assert.equal(snapshot('div', {}, { onclick() {} }).clickable, true);
  assert.equal(snapshot('div').clickable, false);
});

test('empty fields have an empty value; non-fields have no value', () => {
  assert.equal(snapshot('input', {}, { value: '' }).value, '');
  assert.equal(snapshot('textarea', {}, { value: 'hello' }).value, 'hello');
  assert.equal(snapshot('div').value, null);
});
