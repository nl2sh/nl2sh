import assert from 'node:assert/strict';
import test from 'node:test';
import {markdown} from '../src/markdown.ts';

test('renders common model answer Markdown', () => {
  const html = markdown('# 标题\n\n1. **第一项**\n2. 第二项\n\n| 名称 | 值 |\n| --- | --- |\n| A | `ok` |\n\n```sh\necho hello\n```');
  assert.match(html, /<h1>标题<\/h1>/);
  assert.match(html, /<ol>[\s\S]*<strong>第一项<\/strong>[\s\S]*<\/ol>/);
  assert.match(html, /<table>[\s\S]*<td><code>ok<\/code><\/td>[\s\S]*<\/table>/);
  assert.match(html, /<pre><code class="language-sh">echo hello\n<\/code><\/pre>/);
});

test('treats model HTML as text and rejects script links', () => {
  const html = markdown('<img src=x onerror=alert(1)>\n\n[run](javascript:alert(1))');
  assert.match(html, /&lt;img src=x onerror=alert\(1\)&gt;/);
  assert.doesNotMatch(html, /<img|href="javascript:/);
});
