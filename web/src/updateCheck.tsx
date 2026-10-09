import {useEffect,useRef,useState} from 'preact/hooks';
import {useModalFocus} from './modalFocus';
import {updateGuidance} from './runtimeStatus';
import {hasUpdate,updatePercent,updateStage} from './updateState';
import type {UpdateJob,UpdateResult} from './updateState';

async function request<T>(path:string,init?:RequestInit):Promise<T>{
  const response=await fetch(path,{cache:'no-store',...init});
  if(!response.ok)throw new Error(await response.text());
  return response.json();
}
function UpdateDialog({result,job,checking,working,error,check,start,skip,close}:{result:UpdateResult|null;job:UpdateJob|null;checking:boolean;working:boolean;error:string;check:()=>void;start:()=>void;skip:()=>void;close:()=>void}){
  const dialog=useModalFocus(close);
  const percent=job?updatePercent(job):null;
  const installed=job?.progress?.stage==='complete';
  const running=working||job?.busy;
  return <div class="backdrop" onMouseDown={e=>{if(e.target===e.currentTarget)close()}}><div class="modal update-modal" ref={dialog} tabIndex={-1} role="dialog" aria-modal="true" aria-label="版本更新">
    <div class="modal-heading"><h2>版本更新</h2><button type="button" onClick={close} aria-label="关闭更新窗口">×</button></div>
    {result&&<p>当前版本 v{result.current}{result.latest?` · 新版本 v${result.latest}`:' · 暂无更新'}</p>}
    {checking&&<p role="status">正在检查更新…</p>}
    {job?.progress&&<div class="update-progress" role="status"><p>{updateStage(job)}</p>
      {job.progress.total>0&&<><progress aria-label="更新下载进度" max={job.progress.total} value={job.progress.downloaded}/><p>下载 {job.progress.downloaded.toLocaleString()} / {job.progress.total.toLocaleString()} 字节（{percent}%）</p></>}
      {job.busy&&job.progress.total===0&&<progress aria-label="正在检查更新"/>}
      {job.error&&<p class="bad" role="alert">{job.error}</p>}
    </div>}
    {!running&&!installed&&result?.latest&&<>
      {result.can_install?<p class="muted">选择立即更新后下载并校验新版本。安装完成后需重启 nl2sh。</p>:<p class="muted">{updateGuidance(result.ownership.owner)}{result.ownership.owner==='standalone'?'网页自更新仅支持 Android 安装。':''}</p>}
      <div class="update-choices"><button type="button" disabled={!result.can_install||checking} onClick={start}>{job?.error?'重试更新':'立即更新'}</button><button type="button" onClick={close}>暂不更新</button><button type="button" onClick={skip}>跳过本次版本</button></div>
    </>}
    {running&&<p class="muted">关闭窗口后更新继续，可点击更新图标重新查看进度。</p>}
    {!running&&!installed&&<button type="button" disabled={checking} onClick={check}>重新检查更新</button>}
    {error&&<p class="bad" role="alert">{error}</p>}
  </div></div>;
}
export function UpdateCheck(){
  const [result,setResult]=useState<UpdateResult|null>(null),[job,setJob]=useState<UpdateJob|null>(null);
  const [checking,setChecking]=useState(false),[working,setWorking]=useState(false),[open,setOpen]=useState(false);
  const [error,setError]=useState(''),[pollError,setPollError]=useState('');
  const [skipped,setSkipped]=useState(localStorage.getItem('nl2sh-skipped-update')||'');
  const alive=useRef(true),checkRunning=useRef(false),starting=useRef(false);
  const check=async(automatic=false)=>{
    if(checkRunning.current)return;
    checkRunning.current=true;setChecking(true);setError('');
    try{
      const next=await request<UpdateResult>('/api/update');
      if(alive.current){setResult(next);if(automatic&&next.latest&&next.latest!==localStorage.getItem('nl2sh-skipped-update'))setOpen(true)}
    }catch(error){if(alive.current)setError(`检查更新失败：${String(error)}`)}
    finally{checkRunning.current=false;if(alive.current)setChecking(false)}
  };
  useEffect(()=>{
    alive.current=true;void check(true);
    return()=>{alive.current=false};
  },[]);
  useEffect(()=>{
    let active=true;
    let timer:ReturnType<typeof setTimeout>;
    const poll=async()=>{
      let busy=false;
      try{const next=await request<UpdateJob>('/api/update/progress');busy=next.busy;if(active){setJob(next);setPollError('')}}
      catch(error){if(active)setPollError(`进度读取失败，正在重试：${String(error)}`)}
      if(active)timer=setTimeout(poll,busy?500:5000);
    };
    void poll();
    return()=>{active=false;clearTimeout(timer)};
  },[job?.busy]);
  const start=async()=>{
    if(!result?.latest||starting.current)return;
    starting.current=true;setWorking(true);setError('');
    try{
      const next=await request<UpdateJob>('/api/update',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({version:result.latest})});
      if(alive.current){setJob(next);localStorage.removeItem('nl2sh-skipped-update');setSkipped('')}
    }catch(error){if(alive.current)setError(`启动更新失败：${String(error)}`)}
    finally{starting.current=false;if(alive.current)setWorking(false)}
  };
  const skip=()=>{if(result?.latest){localStorage.setItem('nl2sh-skipped-update',result.latest);setSkipped(result.latest)}setOpen(false)};
  const available=hasUpdate(result,job,skipped);
  const label=job?.busy?'更新进行中，查看进度':available?`发现新版本 ${result?.latest}`:'检查更新';
  return <div class="update-check"><button type="button" aria-label={label} title={checking?'正在检查更新…':label} onClick={()=>{setOpen(true);if(!job?.busy&&job?.progress?.stage!=='complete')void check()}}><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M20 7v5h-5M4 17v-5h5"/><path d="M6 7a7 7 0 0 1 12-1l2 3M4 15l2 3a7 7 0 0 0 12-1"/></svg>{available&&<i class="update-dot"/>}</button>
    {open&&<UpdateDialog result={result} job={job} checking={checking} working={working} error={error||pollError} check={()=>{void check()}} start={()=>{void start()}} skip={skip} close={()=>setOpen(false)}/>}
  </div>;
}
