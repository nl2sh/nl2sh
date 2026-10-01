import MarkdownIt from 'markdown-it';
import {highlightCode} from './codeHighlight.ts';
const parser = new MarkdownIt({
  html: false,
  breaks: true,
  linkify: false,
  highlight(code, language) {
    const name = language.split(/\s+/, 1)[0].toLowerCase();
    return highlightCode(code,name);
  },
});

export function markdown(source: string): string {
  return parser.render(source);
}
