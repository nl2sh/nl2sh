import {useEffect,useRef,useState} from 'preact/hooks';
import {api,getVersion} from './api';
export {FilePanel} from './fileBrowser';
import {ConfigEditor} from './configEditor';
import {displayTerminalResponse} from './terminalOutput';
import type {InstalledApp} from './types';
import type {MemoryEntry} from './api';

export type Panel = 'sessions'|'files'|'apps'|'tools'|'terminal'|'memory'|'config';

export const panelWidths:Record<Panel,number>={sessions:280,files:350,apps:340,tools:390,terminal:500,memory:420,config:620};

const panels:{id:Panel;label:string;path:string}[]=[
  {id:'sessions',label:'会话列表',path:'M3 4h18v13H8l-5 4V4Zm4 4h10M7 12h7'},
  {id:'files',label:'文件列表',path:'M2 6h7l2 2h11v11H2V6Zm0 4h20'},
  {id:'apps',label:'应用列表',path:'M3 3h7v7H3V3Zm11 0h7v7h-7V3ZM3 14h7v7H3v-7Zm11 0h7v7h-7v-7Z'},
  {id:'tools',label:'工具列表',path:'M14 3a6 6 0 0 0 7 7l-9 9a3 3 0 0 1-4-4l9-9a6 6 0 0 0-3-3ZM4 4l5 5'},
  {id:'terminal',label:'终端',path:'M3 5h18v14H3V5Zm4 4 3 3-3 3m5 0h5'},
  {id:'memory',label:'记忆',path:'M5 5c0-2 2-3 4-2 1-2 4-2 5 0 2-1 5 1 4 3 2 1 2 4 0 5 1 2-1 4-3 4-1 2-4 3-6 1-2 2-5 1-5-2-2 1-5-1-4-3-2 0-4-2-3-4-2-1-1-4 1-5 3-2-1-4-3-3-5 0-1 0-2 0-3-1Z'},
  {id:'config',label:'配置',path:'M4 6h16M4 12h16M4 18h16M9 4v4m6 2v4m-6 2v4'},
];

export function ActivityBar({active,select,create}:{active:Panel|null;select:(panel:Panel)=>void;create:()=>void}){
  const[version,setVersion]=useState('');
  useEffect(()=>{let active=true;getVersion().then(value=>{if(active)setVersion(value)}).catch(()=>{});return()=>{active=false}},[]);
  return <nav class="activity-bar" aria-label="左侧菜单"><img src="/logo.png" alt="nl2sh"/>{version&&<small class="activity-version" title={`nl2sh v${version}`}>v{version}</small>}{panels.map(panel=><button key={panel.id} type="button" title={panel.label} aria-label={panel.label} aria-pressed={active===panel.id} onClick={()=>select(panel.id)}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={panel.path}/></svg></button>)}<button class="activity-new-session" type="button" title="新建会话" aria-label="新建会话" onClick={create}>＋</button></nav>
}

export function PanelHeading({title,minimize}:{title:string;minimize:()=>void}){
  return <header class="panel-heading"><h2>{title}</h2><button type="button" class="panel-minimize" onClick={minimize} aria-label={`最小化${title}`} title={`最小化${title}`}>−</button></header>
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

export function MemoryPanel(){
  const[entries,setEntries]=useState<MemoryEntry[]>([]),[query,setQuery]=useState(''),[key,setKey]=useState(''),[value,setValue]=useState(''),[editing,setEditing]=useState<string|null>(null),[loading,setLoading]=useState(false),[error,setError]=useState('');
  const refresh=()=>{setLoading(true);setError('');api.memory().then(setEntries).catch(e=>setError(String(e))).finally(()=>setLoading(false))};
  useEffect(refresh,[]);
  const reset=()=>{setKey('');setValue('');setEditing(null)};
  const save=async()=>{if(!key.trim()||loading)return;setLoading(true);setError('');try{setEntries(await api.saveMemory(key.trim(),value));reset()}catch(e){setError(String(e))}finally{setLoading(false)}};
  const remove=async(entry:MemoryEntry)=>{if(!window.confirm(`删除记忆“${entry.key}”？`))return;setLoading(true);setError('');try{setEntries(await api.deleteMemory(entry.key));if(editing===entry.key)reset()}catch(e){setError(String(e))}finally{setLoading(false)}};
  const clear=async()=>{if(!entries.length||!window.confirm(`清空全部 ${entries.length} 条记忆？此操作无法撤销。`))return;setLoading(true);setError('');try{setEntries(await api.clearMemory());reset()}catch(e){setError(String(e))}finally{setLoading(false)}};
  const edit=(entry:MemoryEntry)=>{setEditing(entry.key);setKey(entry.key);setValue(entry.value)};
  const needle=query.trim().toLocaleLowerCase();
  const visible=entries.filter(entry=>!needle||entry.key.toLocaleLowerCase().includes(needle)||entry.value.toLocaleLowerCase().includes(needle));
  return <div class="panel-body memory-panel"><div class="panel-actions"><input type="search" aria-label="搜索记忆" placeholder="搜索键或内容" value={query} onInput={e=>setQuery(e.currentTarget.value)}/><button onClick={refresh} disabled={loading}>刷新</button></div><div class="memory-editor"><label>键<input aria-label="记忆键" placeholder="例如 user_name" value={key} readOnly={editing!==null} onInput={e=>setKey(e.currentTarget.value)}/></label><label>内容<textarea aria-label="记忆内容" placeholder="输入需要持久保存的内容" value={value} onInput={e=>setValue(e.currentTarget.value)}/></label><div class="memory-editor-actions"><button class="primary" disabled={loading||!key.trim()} onClick={save}>{editing?'保存修改':'新增记忆'}</button>{editing&&<button disabled={loading} onClick={reset}>取消</button>}</div><small>键仅支持字母、数字及 _ - . : /，单条内容最多 16 KiB。</small></div>{error&&<p class="bad">记忆操作失败：{error}</p>}<div class="memory-list-heading"><span>{visible.length} / {entries.length} 条记忆</span><button class="memory-clear" disabled={loading||!entries.length} onClick={clear}>清空全部</button></div><div class="panel-list memory-list">{visible.map(entry=><article class="memory-item" key={entry.key}><header><strong>{entry.key}</strong><small>{new Date(entry.updated_unix_secs*1000).toLocaleString('zh-CN')}</small></header><p>{entry.value}</p><div><button onClick={()=>edit(entry)}>编辑</button><button class="danger" onClick={()=>remove(entry)}>删除</button></div></article>)}{!loading&&!error&&entries.length===0&&<p class="muted">还没有持久记忆。</p>}{!loading&&entries.length>0&&visible.length===0&&<p class="muted">没有匹配的记忆。</p>}</div><p class="panel-hint">这里的操作由你直接发起；Agent 写入仍需经过确认。</p></div>
}

export function TerminalPanel({id}:{id:string}){
  const[lines,setLines]=useState(['WebSocket 安全终端：命令仍经过分类和审批。']),[input,setInput]=useState('');
  const socket=useRef<WebSocket>(),output=useRef<HTMLPreElement>(null);
  useEffect(()=>{const ws=new WebSocket(api.terminalUrl(id));socket.current=ws;ws.onmessage=e=>setLines(value=>[...value,displayTerminalResponse(String(e.data))]);ws.onerror=()=>setLines(value=>[...value,'连接错误']);return()=>{socket.current=undefined;ws.close()}},[id]);
  useEffect(()=>{output.current?.scrollTo(0,output.current.scrollHeight)},[lines]);
  const send=()=>{if(!input.trim()||socket.current?.readyState!==WebSocket.OPEN)return;socket.current.send(JSON.stringify({command:input}));setLines(value=>[...value,`$ ${input}`]);setInput('')};
  return <div class="panel-body terminal-panel"><pre ref={output}>{lines.join('\n')}</pre><div class="terminal-input"><input aria-label="终端命令" value={input} onInput={e=>setInput(e.currentTarget.value)} onKeyDown={e=>e.key==='Enter'&&send()}/><button onClick={send}>运行</button></div></div>
}
