import {useEffect,useState} from 'preact/hooks';
import {api} from './api';
import {toolCategory,toolName,toolPrompt,toolPurpose,toolRisk} from './toolGuide';
import type {ToolInfo} from './types';

const groupNames={jadx:'APK / JADX',tailcat:'Tailcat'} as const;

type Toggle = {group?:string;tool?:string;enabled:boolean};

export function ToolCatalog({usePrompt}:{usePrompt:(value:string)=>void}){
  const [tools,setTools]=useState<ToolInfo[]>([]);
  const [query,setQuery]=useState('');
  const [category,setCategory]=useState('全部');
  const [error,setError]=useState('');
  const [busy,setBusy]=useState(false);
  useEffect(()=>{
    let active=true;
    api.tools().then(items=>{if(active)setTools(items)}).catch(e=>{if(active)setError(String(e))});
    return ()=>{active=false};
  },[]);
  const categories=['全部',...new Set(tools.map(toolCategory))];
  const filtered=tools.filter(tool=>
    (category==='全部'||toolCategory(tool)===category)&&
    `${toolName(tool)} ${tool.name} ${toolPurpose(tool)} ${tool.description} ${toolCategory(tool)} ${tool.group||''}`
      .toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())
  );
  const toggle=async(change:Toggle)=>{
    setBusy(true);
    setError('');
    try{setTools(await api.toggleTool(change))}catch(e){setError(String(e))}finally{setBusy(false)}
  };
  return <div class="panel-body tool-catalog">
    <input type="search" aria-label="搜索工具" placeholder="搜索用途或工具名称" value={query} onInput={e=>setQuery(e.currentTarget.value)}/>
    <div class="catalog-categories" aria-label="工具分类">
      {categories.map(item=><button key={item} aria-pressed={item===category} onClick={()=>setCategory(item)}>{item}</button>)}
    </div>
    <section class="tool-groups">
      <h3>可选工具组</h3>
      {(Object.entries(groupNames) as [keyof typeof groupNames,string][]).map(([id,label])=>{
        const members=tools.filter(tool=>tool.group===id);
        const enabled=members.filter(tool=>tool.enabled).length;
        return <div class="tool-group" key={id}>
          <span><strong>{label}</strong><small>{enabled}/{members.length} 个工具已开启</small></span>
          <span class="tool-group-actions">
            <button disabled={busy||enabled===members.length} onClick={()=>toggle({group:id,enabled:true})}>全部开启</button>
            <button disabled={busy||enabled===0} onClick={()=>toggle({group:id,enabled:false})}>全部关闭</button>
          </span>
        </div>;
      })}
      <small>单个工具开关会覆盖工具组设置；按组操作会清除该组的单项覆盖。配置会保存到设备。</small>
    </section>
    {error&&<p class="bad">{error}</p>}
    <p class="catalog-count">{filtered.length} / {tools.length} 个工具</p>
    <div class="catalog-list">
      {filtered.map(tool=><article key={tool.name}>
        <strong>{toolName(tool)}</strong>
        <small>{tool.group?`${groupNames[tool.group]||tool.group} · `:''}{toolCategory(tool)} · {toolRisk(tool)} · {tool.name}</small>
        <p>{toolPurpose(tool)}</p>
        {tool.group&&<label class="tool-enable"><input type="checkbox" checked={tool.enabled} disabled={busy} onChange={e=>toggle({tool:tool.name,enabled:e.currentTarget.checked})}/>模型可用</label>}
        <button disabled={!tool.enabled} onClick={()=>usePrompt(toolPrompt(tool))}>填入示例提问</button>
      </article>)}
      {tools.length>0&&filtered.length===0&&<p>没有匹配的工具</p>}
    </div>
  </div>;
}
