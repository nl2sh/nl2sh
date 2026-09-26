import MarkdownIt from 'markdown-it';
import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import css from 'highlight.js/lib/languages/css';
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import python from 'highlight.js/lib/languages/python';
import rust from 'highlight.js/lib/languages/rust';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';

hljs.registerLanguage('bash', bash);
hljs.registerLanguage('css', css);
hljs.registerLanguage('javascript', javascript);
hljs.registerLanguage('json', json);
hljs.registerLanguage('python', python);
hljs.registerLanguage('rust', rust);
hljs.registerLanguage('typescript', typescript);
hljs.registerLanguage('xml', xml);

const escapeHtml = new MarkdownIt().utils.escapeHtml;
const parser = new MarkdownIt({
  html: false,
  breaks: true,
  linkify: false,
  highlight(code, language) {
    const name = language.split(/\s+/, 1)[0].toLowerCase();
    if (name && hljs.getLanguage(name)) {
      return hljs.highlight(code, {language: name, ignoreIllegals: true}).value;
    }
    return escapeHtml(code);
  },
});

export function markdown(source: string): string {
  return parser.render(source);
}
