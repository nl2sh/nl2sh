import {useEffect,useState} from 'preact/hooks';
import {api} from './api';
import {toolCategory,toolName,toolPrompt,toolPurpose,toolRisk} from './toolGuide';
import type {ToolInfo} from './types';
import {useModalFocus} from './modalFocus';

export function ToolCatalog({close,usePrompt}:{close:()=>void;usePrompt:(value:string)=>void}){
  const[tools,setTools]=useState<ToolInfo[]>([]),[query,setQuery]=useState(''),[category,setCategory]=useState('全部'),[error,setError]=useState('');
  const dialog=useModalFocus(close);
  useEffect(()=>{let active=true;api.tools().then(items=>{if(active)setTools(items)}).catch(e=>{if(active)setError(String(e))});return()=>{active=false}},[]);
  const categories=['全部',...new Set(tools.map(toolCategory))];
  const filtered=tools.filter(tool=>(category==='全部'||toolCategory(tool)===category)&&`${toolName(tool)} ${tool.name} ${toolPurpose(tool)} ${tool.description} ${toolCategory(tool)}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  return <div class="backdrop" onMouseDown={e=>{if(e.target===e.currentTarget)close()}}><div ref={dialog} tabIndex={-1} class="modal tool-catalog" role="dialog" aria-modal="true" aria-label="工具列表"><div class="modal-heading"><h2>能做什么</h2><button onClick={close} aria-label="关闭工具列表">关闭</button></div><input type="search" placeholder="搜索用途或工具名称" value={query} onInput={e=>setQuery(e.currentTarget.value)}/><div class="catalog-categories" aria-label="工具分类">{categories.map(item=><button key={item} aria-pressed={item===category} onClick={()=>setCategory(item)}>{item}</button>)}</div>{error?<p class="bad">{error}</p>:<><p class="catalog-count">{filtered.length} / {tools.length} 个工具</p><div class="catalog-list">{filtered.map(tool=><article key={tool.name}><strong>{toolName(tool)}</strong><small>{toolCategory(tool)} · {toolRisk(tool)} · {tool.name}</small><p>{toolPurpose(tool)}</p><button onClick={()=>usePrompt(toolPrompt(tool))}>填入示例提问</button></article>)}{tools.length>0&&filtered.length===0&&<p>没有匹配的工具</p>}</div></>}</div></div>
}
