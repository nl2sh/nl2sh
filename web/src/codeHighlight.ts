import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import css from 'highlight.js/lib/languages/css';
import javascript from 'highlight.js/lib/languages/javascript';
import java from 'highlight.js/lib/languages/java';
import json from 'highlight.js/lib/languages/json';
import kotlin from 'highlight.js/lib/languages/kotlin';
import cpp from 'highlight.js/lib/languages/cpp';
import go from 'highlight.js/lib/languages/go';
import ini from 'highlight.js/lib/languages/ini';
import python from 'highlight.js/lib/languages/python';
import rust from 'highlight.js/lib/languages/rust';
import sql from 'highlight.js/lib/languages/sql';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';
import yaml from 'highlight.js/lib/languages/yaml';

hljs.registerLanguage('bash',bash);
hljs.registerLanguage('css',css);
hljs.registerLanguage('javascript',javascript);
hljs.registerLanguage('java',java);
hljs.registerLanguage('json',json);
hljs.registerLanguage('kotlin',kotlin);
hljs.registerLanguage('cpp',cpp);
hljs.registerLanguage('go',go);
hljs.registerLanguage('ini',ini);
hljs.registerLanguage('python',python);
hljs.registerLanguage('rust',rust);
hljs.registerLanguage('sql',sql);
hljs.registerLanguage('typescript',typescript);
hljs.registerLanguage('xml',xml);
hljs.registerLanguage('yaml',yaml);

const languages:Record<string,string>={
  rs:'rust',py:'python',js:'javascript',jsx:'javascript',ts:'typescript',tsx:'typescript',
  json:'json',css:'css',html:'xml',htm:'xml',xml:'xml',svg:'xml',
  sh:'bash',bash:'bash',zsh:'bash',java:'java',kt:'kotlin',kts:'kotlin',
  c:'cpp',h:'cpp',cpp:'cpp',hpp:'cpp',go:'go',sql:'sql',
  yaml:'yaml',yml:'yaml',ini:'ini',conf:'ini',properties:'ini',toml:'ini',
};
const escapeHtml=(text:string)=>text.replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]||char));

export function highlightCode(source:string,language:string):string{
  if(!language||!hljs.getLanguage(language))return escapeHtml(source);
  return hljs.highlight(source,{language,ignoreIllegals:true}).value;
}
export function fileLanguage(filename:string):string{
  const name=filename.toLowerCase();
  if(['dockerfile','makefile','.gitignore'].includes(name))return 'bash';
  return languages[name.split('.').pop()||'']||'';
}
