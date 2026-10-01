import {useEffect,useRef,useState} from 'preact/hooks';
import {api} from './api';
import {ConfigEditor} from './configEditor';
import {displayTerminalResponse} from './terminalOutput';
import type {InstalledApp} from './types';

export type Panel = 'sessions'|'files'|'apps'|'tools'|'terminal'|'config';

export const panelWidths:Record<Panel,number>={sessions:280,files:350,apps:340,tools:390,terminal:500,config:620};

const panels:{id:Panel;label:string;path:string}[]=[
  {id:'sessions',label:'会话列表',path:'M3 4h18v13H8l-5 4V4Zm4 4h10M7 12h7'},
  {id:'files',label:'文件列表',path:'M2 6h7l2 2h11v11H2V6Zm0 4h20'},
  {id:'apps',label:'应用列表',path:'M3 3h7v7H3V3Zm11 0h7v7h-7V3ZM3 14h7v7H3v-7Zm11 0h7v7h-7v-7Z'},
  {id:'tools',label:'工具列表',path:'M14 3a6 6 0 0 0 7 7l-9 9a3 3 0 0 1-4-4l9-9a6 6 0 0 0-3-3ZM4 4l5 5'},
  {id:'terminal',label:'终端',path:'M3 5h18v14H3V5Zm4 4 3 3-3 3m5 0h5'},
  {id:'config',label:'配置',path:'M4 6h16M4 12h16M4 18h16M9 4v4m6 2v4m-6 2v4'},
];

export function ActivityBar({active,select,create}:{active:Panel|null;select:(panel:Panel)=>void;create:()=>void}){
  return <nav class="activity-bar" aria-label="左侧菜单"><img src="/logo.png" alt="nl2sh"/>{panels.map(panel=><button key={panel.id} type="button" title={panel.label} aria-label={panel.label} aria-pressed={active===panel.id} onClick={()=>select(panel.id)}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={panel.path}/></svg></button>)}<button class="activity-new-session" type="button" title="新建会话" aria-label="新建会话" onClick={create}>＋</button></nav>
}

export function PanelHeading({title,minimize}:{title:string;minimize:()=>void}){
  return <header class="panel-heading"><h2>{title}</h2><button type="button" class="panel-minimize" onClick={minimize} aria-label={`最小化${title}`} title={`最小化${title}`}>−</button></header>
}

export function FilePanel({usePath}:{usePath:(path:string)=>void}){
  const[path,setPath]=useState('.'),[revision,setRevision]=useState(0),[entries,setEntries]=useState<string[]>([]),[error,setError]=useState(''),[loading,setLoading]=useState(false);
  useEffect(()=>{let active=true;setLoading(true);setError('');api.fileSuggestions(path).then(items=>{if(active)setEntries(items)}).catch(e=>{if(active)setError(String(e))}).finally(()=>{if(active)setLoading(false)});return()=>{active=false}},[path,revision]);
  const normalized=path.replace(/\/$/,'');
  const parent=path==='/'||path==='.'||path==='~'?null:normalized.slice(0,normalized.lastIndexOf('/'))|| (path.startsWith('/')?'/':'.');
  return <div class="panel-body file-panel"><p class="panel-path" title={path}>{path}</p><div class="panel-actions"><button onClick={()=>setPath(parent||'.')} disabled={!parent}>上一级</button><button onClick={()=>setPath('/')}>根目录</button><button onClick={()=>setPath('~')}>主目录</button><button onClick={()=>setRevision(value=>value+1)}>刷新</button></div>{loading&&<p class="muted">正在读取…</p>}{error&&<p class="bad">{error}</p>}<div class="panel-list">{entries.map(entry=><div key={entry} class="file-item"><button class="panel-list-item" title={entry} onClick={()=>entry.endsWith('/')?setPath(entry):usePath(entry)}><span aria-hidden="true">{entry.endsWith('/')?'▸':'·'}</span><span>{entry.replace(/\/$/,'').split('/').pop()||entry}</span></button><button class="file-insert" type="button" aria-label={`将 ${entry} 填入对话输入框`} title="将路径填入对话输入框" onClick={()=>usePath(entry)}>→</button></div>)}{!loading&&!error&&entries.length===0&&<p class="muted">此目录没有可显示的文件</p>}</div><p class="panel-hint">点击名称浏览文件夹，点击右侧箭头将路径填入对话。</p></div>
}

export function ConfigPanel({onSaved}:{onSaved:()=>Promise<void>}){
  const[raw,setRaw]=useState<string>(),[error,setError]=useState('');
  useEffect(()=>{let active=true;api.config().then(value=>{if(active)setRaw(value)}).catch(e=>{if(active)setError(String(e))});return()=>{active=false}},[]);
  if(error)return <p class="panel-hint bad">配置读取失败：{error}</p>;
  if(raw===undefined)return <p class="panel-hint">正在读取配置…</p>;
  return <ConfigEditor initial={raw} onSaved={onSaved}/>;
}

export function AppPanel({usePackage}:{usePackage:(name:string)=>void}){
  const[apps,setApps]=useState<InstalledApp[]>([]),[error,setError]=useState(''),[loading,setLoading]=useState(false),[query,setQuery]=useState('');
  const refresh=()=>{setLoading(true);setError('');api.apps().then(setApps).catch(e=>setError(String(e))).finally(()=>setLoading(false))};
  useEffect(refresh,[]);
  const visible=apps.filter(app=>app.package.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  return <div class="panel-body app-panel"><div class="panel-actions"><input type="search" aria-label="搜索应用包名" placeholder="搜索包名" value={query} onInput={e=>setQuery(e.currentTarget.value)}/><button onClick={refresh} disabled={loading}>刷新</button></div>{loading&&<p class="muted">正在读取…</p>}{error&&<p class="bad">{error}</p>}<p class="panel-count">{visible.length} / {apps.length} 个应用</p><div class="panel-list">{visible.map(app=><button key={app.package} class="panel-list-item" title={app.package} onClick={()=>usePackage(app.package)}>{app.package}</button>)}{!loading&&!error&&apps.length===0&&<p class="muted">没有可显示的应用</p>}</div><p class="panel-hint">选择应用可填入查看应用信息的提问。</p></div>
}

export function TerminalPanel({id}:{id:string}){
  const[lines,setLines]=useState(['WebSocket 安全终端：命令仍经过分类和审批。']),[input,setInput]=useState('');
  const socket=useRef<WebSocket>(),output=useRef<HTMLPreElement>(null);
  useEffect(()=>{const ws=new WebSocket(api.terminalUrl(id));socket.current=ws;ws.onmessage=e=>setLines(value=>[...value,displayTerminalResponse(String(e.data))]);ws.onerror=()=>setLines(value=>[...value,'连接错误']);return()=>{socket.current=undefined;ws.close()}},[id]);
  useEffect(()=>{output.current?.scrollTo(0,output.current.scrollHeight)},[lines]);
  const send=()=>{if(!input.trim()||socket.current?.readyState!==WebSocket.OPEN)return;socket.current.send(JSON.stringify({command:input}));setLines(value=>[...value,`$ ${input}`]);setInput('')};
  return <div class="panel-body terminal-panel"><pre ref={output}>{lines.join('\n')}</pre><div class="terminal-input"><input aria-label="终端命令" value={input} onInput={e=>setInput(e.currentTarget.value)} onKeyDown={e=>e.key==='Enter'&&send()}/><button onClick={send}>运行</button></div></div>
}
