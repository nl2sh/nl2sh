import {localMedia} from './media.ts';
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

const copyButton = '<button class="code-copy" type="button" aria-label="复制代码" title="复制代码">'
  + '<svg class="copy-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M8 7V5a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-2"/><rect x="3" y="8" width="13" height="13" rx="2"/></svg>'
  + '<svg class="copy-done-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="m5 12 4 4L19 6"/></svg>'
  + '</button>';
const fenceRenderer = parser.renderer.rules.fence;
parser.renderer.rules.fence = (tokens, index, options, env, renderer) => {
  const code = fenceRenderer
    ? fenceRenderer(tokens, index, options, env, renderer)
    : renderer.renderToken(tokens, index, options);
  return `<div class="code-block">${copyButton}${code}</div>`;
};

const imageRenderer=parser.renderer.rules.image;
parser.renderer.rules.image=(tokens,index,options,env,renderer)=>{
  const media=localMedia(String(tokens[index].attrGet('src')||''));
  if(media)return `<span data-media-path="${parser.utils.escapeHtml('/api/file-preview?path='+encodeURIComponent(media.path))}"></span>`;
  return imageRenderer?imageRenderer(tokens,index,options,env,renderer):renderer.renderToken(tokens,index,options);
};
const linkRenderer=parser.renderer.rules.link_open;
parser.renderer.rules.link_open=(tokens,index,options,env,renderer)=>{
  const target=String(tokens[index].attrGet('href')||'');
  const media=localMedia(target);
  const preview=media?`<span data-media-path="${parser.utils.escapeHtml('/api/file-preview?path='+encodeURIComponent(media.path))}"></span>`:'';
  return preview+(linkRenderer?linkRenderer(tokens,index,options,env,renderer):renderer.renderToken(tokens,index,options));
};

export function markdown(source: string): string {
  return parser.render(source);
}
