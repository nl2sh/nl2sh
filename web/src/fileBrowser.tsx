import {useEffect,useState} from 'preact/hooks';
import {fileUrl,listFiles} from './api';
import type {BrowserFile} from './api';
import {useModalFocus} from './modalFocus';
import {highlightCode,fileLanguage} from './codeHighlight';
import {MediaPreview} from './mediaPreview';

const dateFormat=new Intl.DateTimeFormat('zh-CN',{year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit'});
function formatSize(bytes:number|null){
  if(bytes===null)return '—';
  if(bytes<1024)return `${bytes} B`;
  const units=['KB','MB','GB'];
  let value=bytes,unit='B';
  for(const next of units){value/=1024;unit=next;if(value<1024)break}
  return `${value.toFixed(value<10?1:0)} ${unit}`;
}
function fileIcon(file:BrowserFile){
  if(file.is_dir)return '📁';
  const extension=file.name.split('.').pop()?.toLowerCase();
  if(['png','jpg','jpeg','gif','webp','bmp','avif','svg'].includes(extension||''))return '🖼️';
  if(['mp4','m4v','webm','ogv','mov','mkv','avi'].includes(extension||''))return '🎞️';
  if(['rs','py','js','jsx','ts','tsx','java','kt','kts','c','h','cpp','hpp','go','sh','bash','zsh','sql','html','css'].includes(extension||''))return '⌘';
  if(['json','toml','yaml','yml','xml','ini','conf','properties','gradle'].includes(extension||''))return '⚙️';
  if(file.preview_kind==='text')return '📄';
  if(['zip','tar','gz','7z','rar'].includes(extension||''))return '📦';
  if(['mp3','wav','pcm','raw','flac','ogg','oga','m4a','aac'].includes(extension||''))return '🎵';
  return '📃';
}

function FilePreview({file,close}:{file:BrowserFile;close:()=>void}){
  const dialog=useModalFocus(close);
  const [text,setText]=useState<string|null>(null),[error,setError]=useState('');
  const [wrap,setWrap]=useState(true);
  useEffect(()=>{
    if(file.preview_kind!=='text')return;
    const controller=new AbortController();
    fetch(fileUrl(file.path),{signal:controller.signal,cache:'no-store'})
      .then(async response=>{if(!response.ok)throw new Error(await response.text());return response.text()})
      .then(setText).catch(e=>{if(!controller.signal.aborted)setError(String(e))});
    return()=>controller.abort();
  },[file.path,file.preview_kind]);
  return <div class="backdrop" onMouseDown={e=>{if(e.target===e.currentTarget)close()}}>
    <div ref={dialog} class="modal file-preview" role="dialog" aria-modal="true" aria-label={`预览 ${file.name}`} tabIndex={-1}>
      <div class="modal-heading"><h2 title={file.path}>{file.name}</h2><button type="button" onClick={close} aria-label="关闭文件预览">×</button></div>
      <p class="file-preview-meta">{formatSize(file.size)} · {file.modified_ms===null?'修改时间未知':dateFormat.format(file.modified_ms)}</p>
      {['image','video','audio'].includes(file.preview_kind||'')&&<MediaPreview file={file}/>}
      {file.preview_kind==='text'&&<label class="file-wrap"><input type="checkbox" checked={wrap} onChange={e=>setWrap(e.currentTarget.checked)}/> 自动换行</label>}
      {file.preview_kind==='text'&&text!==null&&text.length>200_000&&<p class="file-preview-meta">文件较长，已使用纯文本显示以保持页面流畅。</p>}
      {file.preview_kind==='text'&&!error&&<pre class={wrap?'wrap':''}><code dangerouslySetInnerHTML={{__html:text===null?'正在读取文本…':highlightCode(text,text.length>200_000?'':fileLanguage(file.name))}}/></pre>}
      {error&&<p class="bad" role="alert">{error}</p>}
    </div>
  </div>;
}

export function FilePanel({usePath}:{usePath:(path:string)=>void}){
  const [path,setPath]=useState('.'),[revision,setRevision]=useState(0);
  const [entries,setEntries]=useState<BrowserFile[]>([]),[error,setError]=useState(''),[loading,setLoading]=useState(false);
  const [preview,setPreview]=useState<BrowserFile|null>(null);
  useEffect(()=>{
    let active=true;setLoading(true);setError('');setEntries([]);
    listFiles(path).then(items=>{if(active)setEntries(items)}).catch(e=>{if(active)setError(String(e))}).finally(()=>{if(active)setLoading(false)});
    return()=>{active=false};
  },[path,revision]);
  const normalized=path.replace(/\/$/,'');
  const parent=path==='/'||path==='.'||path==='~'?null:normalized.slice(0,normalized.lastIndexOf('/'))||(path.startsWith('/')?'/':'.');
  return <div class="panel-body file-panel">
    <p class="panel-path" title={path}>{path}</p>
    <div class="panel-actions"><button onClick={()=>setPath(parent||'.')} disabled={!parent}>上一级</button><button onClick={()=>setPath('/')}>根目录</button><button onClick={()=>setPath('~')}>主目录</button><button onClick={()=>setRevision(value=>value+1)}>刷新</button></div>
    {loading&&<p class="muted">正在读取…</p>}{error&&<p class="bad">{error}</p>}
    <div class="panel-list">{entries.map(entry=><div key={entry.path} class="file-item">
      <button class="panel-list-item" title={entry.path} onClick={()=>entry.is_dir?setPath(entry.path):entry.preview_kind&&setPreview(entry)} disabled={!entry.is_dir&&!entry.preview_kind}>
        <span class="file-icon" aria-hidden="true">{fileIcon(entry)}</span>
        <span class="file-details"><span class="file-name">{entry.name}</span><small>{entry.modified_ms===null?'修改时间未知':dateFormat.format(entry.modified_ms)} · {entry.is_dir?'文件夹':formatSize(entry.size)}</small></span>
      </button>
      <button class="file-insert" type="button" aria-label={`将 ${entry.path} 填入对话输入框`} title="将路径填入对话输入框" onClick={()=>usePath(entry.path)}>→</button>
    </div>)}{!loading&&!error&&entries.length===0&&<p class="muted">此目录没有可显示的文件</p>}</div>
    <p class="panel-hint">点击文件夹浏览，点击可预览的文件查看内容；右侧箭头将路径填入对话。</p>
    {preview&&<FilePreview key={preview.path} file={preview} close={()=>setPreview(null)}/>}
  </div>;
}
