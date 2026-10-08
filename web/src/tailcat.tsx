import {useEffect,useRef,useState} from 'preact/hooks';
import {useModalFocus} from './modalFocus';
import {copyCode} from './codeCopy';
interface State{id:number;busy:boolean;status:string;downloaded:number;total:number|null;messages:string[];commands:string[]}
async function request(body?:object):Promise<State>{const response=await fetch('/api/tailcat',{cache:'no-store',...(body?{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)}:{})});if(!response.ok)throw new Error(await response.text());return response.json()}
export function TailcatIcon(){return <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><path d="M5 10 4 3l6 4h4l6-4-1 7c2 3 1 10-7 10S3 13 5 10Z"/><path d="M8 12h1m6 0h1m-6 4 2 1 2-1M1 14l5 1m12 0 5-1"/></svg>}
export function TailcatDialog({close}:{close:()=>void}){
 const[state,setState]=useState<State>(),[web,setWeb]=useState(true),[adb,setAdb]=useState(true),[port,setPort]=useState<number>(),[adbPort,setAdbPort]=useState('5555'),[error,setError]=useState(''),[working,setWorking]=useState(false),[copied,setCopied]=useState('');
 const dialog=useModalFocus(close);
 const adbEdited=useRef(false);
 const validAdbPort=/^\d{1,5}$/.test(adbPort)&&Number(adbPort)>0&&Number(adbPort)<=65535;
 useEffect(()=>{let alive=true;let timer:ReturnType<typeof setTimeout>;const poll=async()=>{try{const value=await request();if(alive){setState(value)}}catch(e){if(alive)setError(String(e))}if(alive)timer=setTimeout(poll,500)};void poll();void fetch('/api/tailcat/ports').then(r=>{if(!r.ok)throw new Error('端口读取失败');return r.json()}).then(v=>{if(alive){setPort(v.web_port);if(!adbEdited.current)setAdbPort(String(v.adb_port))}}).catch(e=>{if(alive)setError(String(e))});return()=>{alive=false;clearTimeout(timer)}},[]);
 const act=async(body:object)=>{setWorking(true);try{setState(await request(body));setError('')}catch(e){setError(String(e))}finally{setWorking(false)}};
 return <div class="backdrop"><div class="modal tailcat-modal" ref={dialog} tabIndex={-1} role="dialog" aria-modal="true" aria-label="Tailcat"><div class="modal-heading"><h2><TailcatIcon/> Tailcat</h2><button onClick={close}>关闭</button></div>
 <p class="muted">共享 Web / ADB，地址仅交给可信对端。</p>
 <fieldset disabled={state?.busy||working}><legend>共享端口</legend><label><input type="checkbox" checked={web} onChange={e=>setWeb(e.currentTarget.checked)}/> Web {port??'读取中…'}</label><label><input type="checkbox" checked={adb} onChange={e=>setAdb(e.currentTarget.checked)}/> ADB <input class="tailcat-port" type="number" min="1" max="65535" aria-label="ADB 端口" disabled={!adb} value={adbPort} onInput={e=>{adbEdited.current=true;setAdbPort(e.currentTarget.value)}}/></label></fieldset>
 <button disabled={state?.busy||working||(!web&&!adb)||(web&&!port)||(adb&&!validAdbPort)} onClick={()=>act({web,adb,adb_port:Number(adbPort)})}>确定{state?.id?' / 重试':''}</button>
 {state&&state.id!==0&&<><p role="status">{state?.status}</p>{state&&state.downloaded>0&&<div><progress max={state.total||undefined} value={state.total?state.downloaded:undefined}/><p>下载 {state.downloaded.toLocaleString()} {state.total?`/ ${state.total.toLocaleString()} 字节 (${Math.min(100,Math.floor(state.downloaded/state.total*100))}%)`:'字节（总大小未知）'}</p></div>}
 {Boolean(state?.commands.length)&&<><h3>对端命令</h3><p class="muted">对端安装 Tailcat，保持 forward 运行。</p>{state?.commands.map(command=><div class="tailcat-command"><code>{command}</code><button aria-label={`复制 ${command}`} title="复制命令" onClick={async()=>{try{await copyCode(command);setCopied(command)}catch(e){setError(String(e))}}}><svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><rect x="8" y="8" width="12" height="13" rx="2"/><path d="M15 8V3H3v13h5"/></svg>{copied===command?'已复制':''}</button></div>)}</>}
 {state?.messages.map(message=><pre class="tailcat-message">{message}</pre>)}</>}{error&&<p role="alert" class="bad">{error}</p>}<p class="muted">关闭窗口不停止共享。</p>
 </div></div>
}
