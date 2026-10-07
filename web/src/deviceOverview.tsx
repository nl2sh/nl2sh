import {useEffect,useState} from 'preact/hooks';
import {api} from './api';
import type {DeviceOverview as Overview} from './types';
import {runtimeComponents,updateGuidance} from './runtimeStatus';
import type {RuntimeInfo} from './types';
import {useModalFocus} from './modalFocus';

function size(value?:number|null):string{return value==null?'未知':`${(value/1024/1024).toFixed(1)} GiB`}

export function DeviceOverview({title=true}:{title?:boolean}){
  const[info,setInfo]=useState<RuntimeInfo|null>(null),[infoError,setInfoError]=useState('');
  const[data,setData]=useState<Overview|null>(null),[checked,setChecked]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const refresh=async()=>{
    setBusy(true);setError('');
    try{const [overview,runtime]=await Promise.allSettled([api.deviceOverview(),api.info()]);
      if(overview.status==='fulfilled'){setData(overview.value);setChecked(new Date().toLocaleString())}else{setData(null);setError(String(overview.reason))}
      if(runtime.status==='fulfilled'){setInfo(runtime.value);setInfoError('')}else{setInfo(null);setInfoError(String(runtime.reason))}}
    catch(e){setError(String(e))}
    finally{setBusy(false)}
  };
  useEffect(()=>{void refresh()},[]);
  return <section class="device-overview" aria-label="设备概览"><div class="overview-heading">{title&&<h2>设备概览</h2>}<button disabled={busy} onClick={refresh}>{busy?'正在读取…':'刷新'}</button></div>
    {error&&<p class="bad" role="alert">设备信息读取失败：{error}</p>}
    {data&&<><p class="muted">{data.status==='complete'?'读取完成':'部分信息不可用'} · 读取于 {checked}</p><dl>
      <div><dt>Android</dt><dd>{data.android_release||'未知'}{data.api_level?` · API ${data.api_level}`:''}</dd></div>
      <div><dt>设备架构</dt><dd>{data.device_abi||'未知'}</dd></div>
      <div><dt>可用内存</dt><dd>{size(data.memory_kib?.available)} / {size(data.memory_kib?.total)}</dd></div>
      <div><dt>数据分区剩余</dt><dd>{size(data.data_kib?.available)} / {size(data.data_kib?.total)}</dd></div>
    </dl></>}
    {info&&<><h3>运行组件</h3><p>nl2sh {info.version} · {info.abi} · 端口 {info.port}</p><p>{updateGuidance(info.update_ownership.owner)}</p><p class="muted">{info.runtime_policy.status==='verified'?'兼容性清单签名已验证':info.runtime_policy.status==='unsigned_source_build'?'源码构建未附发行签名，推荐版本未知':'发行策略无法验证，推荐版本未知'}</p><dl>{runtimeComponents(info).map(row=><div><dt>{row.name}</dt><dd>{row.installed} · 推荐 {row.expected}{row.drift?' · 版本不一致':''}</dd></div>)}<div><dt>Accessibility / IME</dt><dd>{info.capabilities.bridge?`${info.capabilities.bridge.services.accessibility?'运行中':'未运行'} / ${info.capabilities.bridge.services.ime?'运行中':'未运行'}`:'未发现 Bridge'}</dd></div><div><dt>Tailcat</dt><dd>{info.capabilities.tailcat||'未发现'}</dd></div></dl>{info.runtime_policy.status==='verified'&&<p class="muted">Helper 最低版本 {info.runtime_policy.manifest?.nl2sh_helper.min_version}；当前 Helper 版本请在 Helper 中查看。</p>}</>}
    {infoError&&<p class="bad" role="alert">运行组件读取失败：{infoError}</p>}
    {!data&&!error&&<p class="muted">正在读取固定的只读设备信息，无需模型服务。</p>}
  </section>;
}

export function DeviceOverviewDialog({close}:{close:()=>void}){
  const dialog=useModalFocus(close);
  return <div class="backdrop"><div ref={dialog} tabIndex={-1} class="modal" role="dialog" aria-modal="true" aria-label="设备概览"><div class="modal-heading"><h2>设备概览</h2><button onClick={close}>关闭</button></div><DeviceOverview title={false}/></div></div>;
}
