import {useState} from 'preact/hooks';
import {api} from './api';
import type {Pending} from './types';

function riskDescription(pending:Pending):string{
  if(pending.strong)return '这是高风险操作，可能造成难以恢复的变化。请先查看具体内容。';
  if(pending.risk==='Mutating')return '这一步可能修改设备或文件内容。请确认它符合你的目的。';
  return '这一步需要你的批准。请先查看具体内容。';
}

function riskName(risk?:string):string{
  return ({ReadOnly:'只读',Mutating:'修改',Dangerous:'危险',Critical:'严重危险'} as Record<string,string>)[risk||'']||risk||'未知';
}

export function Approval({id,pending,done}:{id:string;pending:Pending;done:()=>void}){
  const[edit,setEdit]=useState(pending.command||''),[confirm,setConfirm]=useState(''),[answers,setAnswers]=useState<Record<string,string>>({}),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const decide=async(action:string,text?:string)=>{setBusy(true);setError('');try{await api.decision(id,action,text,answers);done()}catch(e){setError(String(e));setBusy(false)}};
  return <div class="backdrop"><div class="modal approval" role="dialog" aria-modal="true" aria-label={pending.kind==='approval'?'执行审批':'补充信息'}><h2>{pending.kind==='approval'?'确认设备操作':'补充信息'}</h2>{pending.kind==='approval'?<><p class="approval-impact">{riskDescription(pending)}</p><p>风险等级：{riskName(pending.risk)}{pending.root?' · 需要 Root 权限':''}{pending.strong?' · 需要强确认':''}</p>{pending.explanation&&<p>检查依据：{pending.explanation}</p>}<p class="muted">拒绝后不会执行这一步。批准仅对当前显示的操作有效。</p><strong>待执行内容</strong><pre class="approval-preview">{pending.command}</pre><details><summary>编辑待执行内容</summary><label>修改后会重新检查风险<textarea value={edit} onInput={e=>setEdit(e.currentTarget.value)}/></label><button disabled={busy||edit===pending.command} onClick={()=>decide('edit',edit)}>提交编辑并重新检查风险</button></details>{pending.strong&&<label>确认高风险操作：输入 CONFIRM<input value={confirm} onInput={e=>setConfirm(e.currentTarget.value)} autocomplete="off" placeholder="CONFIRM"/></label>}<div class="actions"><button class="primary" disabled={busy||(pending.strong&&confirm!=='CONFIRM')} onClick={()=>decide('approve',pending.strong?confirm:undefined)}>批准这一次</button>{!pending.strong&&!pending.root&&<button disabled={busy} onClick={()=>decide('remember')}>本任务记住同一操作</button>}<button disabled={busy} onClick={()=>decide('reject')}>拒绝</button></div></>:<>{pending.questions?.map(q=><label key={q.id}>{q.prompt}<input list={`q-${q.id}`} onInput={e=>setAnswers({...answers,[q.id]:e.currentTarget.value})}/><datalist id={`q-${q.id}`}>{q.options.map(o=><option value={o.value}>{o.label}</option>)}</datalist></label>)}<div class="actions"><button disabled={busy} onClick={()=>decide('answer')}>提交</button><button disabled={busy} onClick={()=>decide('cancel')}>取消</button></div></>}{error&&<p class="bad" role="alert">{error}</p>}</div></div>;
}
