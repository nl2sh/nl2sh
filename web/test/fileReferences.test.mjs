import assert from 'node:assert/strict';
import test from 'node:test';
import {activeReference,completeReference} from '../src/fileReferences.ts';

test('finds the reference at the caret within a sentence', () => {
  assert.deepEqual(activeReference('查看 @dir/fi 后续', 10), {start:3,fragment:'dir/fi'});
  assert.equal(activeReference('查看 @dir/fi 后续', 13), null);
  assert.deepEqual(activeReference('@', 1), {start:0,fragment:''});
});

test('completion preserves text after the caret', () => {
  const text='查看 @dir/fi后续';
  const reference=activeReference(text, 10);
  assert.deepEqual(completeReference(text,10,reference,'dir/file.txt'), {text:'查看 @dir/file.txt后续',cursor:16});
});
