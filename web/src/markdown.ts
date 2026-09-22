import MarkdownIt from 'markdown-it';

const parser = new MarkdownIt({html: false, breaks: true, linkify: false});

export function markdown(source: string): string {
  return parser.render(source);
}
