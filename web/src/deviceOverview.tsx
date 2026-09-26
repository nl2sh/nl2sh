import {useEffect,useState} from 'preact/hooks';
import {api} from './api';
import type {DeviceOverview as Overview} from './types';
import {useModalFocus} from './modalFocus';

function size(value?:number|null):string{return value==null?'未知':`${(value/1024/1024).toFixed(1)} GiB`}

export function DeviceOverview({title=true}:{title?:boolean}){
  const[data,setData]=useState<Overview|null>(null),[checked,setChecked]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const refresh=async()=>{
    setBusy(true);setError('');
    try{setData(await api.deviceOverview());setChecked(new Date().toLocaleString())}
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
    {!data&&!error&&<p class="muted">正在读取固定的只读设备信息，无需模型服务。</p>}
  </section>;
}

export function DeviceOverviewDialog({close}:{close:()=>void}){
  const dialog=useModalFocus(close);
  return <div class="backdrop"><div ref={dialog} tabIndex={-1} class="modal" role="dialog" aria-modal="true" aria-label="设备概览"><div class="modal-heading"><h2>设备概览</h2><button onClick={close}>关闭</button></div><DeviceOverview title={false}/></div></div>;
}
