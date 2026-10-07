import assert from 'node:assert/strict';
import test from 'node:test';
import {copyCode} from '../src/codeCopy.ts';

test('copies code with the Clipboard API', async () => {
  let copied = '';
  Object.defineProperty(globalThis, 'navigator', {
    configurable: true,
    value: {clipboard: {writeText: async text => { copied = text; }}},
  });
  await copyCode('echo hello\n');
  assert.equal(copied, 'echo hello\n');
});

test('falls back when Clipboard API permission is denied', async () => {
  let selected = false;
  let removed = false;
  let appended = false;
  const input = {
    value: '',
    style: {},
    setAttribute() {},
    select() { selected = true; },
    remove() { removed = true; },
  };
  Object.defineProperty(globalThis, 'navigator', {
    configurable: true,
    value: {clipboard: {writeText: async () => { throw new Error('denied'); }}},
  });
  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    value: {
      createElement: () => input,
      body: {append: () => { appended = true; }},
      execCommand: command => command === 'copy',
    },
  });
  await copyCode('完整代码');
  assert.equal(input.value, '完整代码');
  assert.equal(appended, true);
  assert.equal(selected, true);
  assert.equal(removed, true);
});
