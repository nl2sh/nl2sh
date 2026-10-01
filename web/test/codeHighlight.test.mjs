import assert from 'node:assert/strict';
import test from 'node:test';
import {fileLanguage,highlightCode} from '../src/codeHighlight.ts';

test('file extension selects code highlighting and plain text remains escaped',()=>{
  assert.equal(fileLanguage('main.rs'),'rust');
  assert.equal(fileLanguage('README.txt'),'');
  assert.match(highlightCode('fn main() {}',fileLanguage('main.rs')),/hljs-keyword/);
  assert.equal(highlightCode('<img src=x onerror=alert(1)>',''),'&lt;img src=x onerror=alert(1)&gt;');
});
