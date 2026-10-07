import assert from 'node:assert/strict';
import test from 'node:test';
import {markdown} from '../src/markdown.ts';

test('renders common model answer Markdown', () => {
  const html = markdown('# 标题\n\n1. **第一项**\n2. 第二项\n\n| 名称 | 值 |\n| --- | --- |\n| A | `ok` |\n\n```sh\necho hello\n```');
  assert.match(html, /<h1>标题<\/h1>/);
  assert.match(html, /<ol>[\s\S]*<strong>第一项<\/strong>[\s\S]*<\/ol>/);
  assert.match(html, /<table>[\s\S]*<td><code>ok<\/code><\/td>[\s\S]*<\/table>/);
  assert.match(html, /<div class="code-block"><button class="code-copy"[^>]*aria-label="复制代码"[^>]*>[\s\S]*<pre><code class="language-sh"><span class="hljs-built_in">echo<\/span> hello\n<\/code><\/pre>\s*<\/div>/);
  assert.equal((html.match(/class="code-copy"/g)||[]).length,1);
  assert.doesNotMatch(html, />复制代码<\/button>/);
});

test('highlights known fences and escapes unknown or malicious code', () => {
  const html = markdown('```rust\nfn main() { let x = 42; }\n```\n\n```unknown\n<img src=x onerror=alert(1)>\n```');
  assert.match(html, /<span class="hljs-keyword">fn<\/span>/);
  assert.match(html, /<span class="hljs-number">42<\/span>/);
  assert.match(html, /<code class="language-unknown">&lt;img src=x onerror=alert\(1\)&gt;/);
  assert.doesNotMatch(html, /<img/);
});

test('treats model HTML as text and rejects script links', () => {
  const html = markdown('<img src=x onerror=alert(1)>\n\n[run](javascript:alert(1))');
  assert.match(html, /&lt;img src=x onerror=alert\(1\)&gt;/);
  assert.doesNotMatch(html, /<img|href="javascript:/);
});
