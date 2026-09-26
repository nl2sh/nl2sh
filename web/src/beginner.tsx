import {useEffect,useState} from 'preact/hooks';
import {api} from './api';
import {beginnerProviders,initialBeginnerProvider} from './providerPresets';

type ConfigData=Record<string,string|number|boolean|null|unknown[]>;

export function BeginnerSetup({initial,close,done}:{initial:string;close:()=>void;done:()=>Promise<void>}){
  const[base,setBase]=useState<ConfigData|null>(null);
  const[service,setService]=useState(0);
  const[key,setKey]=useState('');
  const[model,setModel]=useState(beginnerProviders[0].model);
  const[endpoint,setEndpoint]=useState(beginnerProviders[0].endpoint);
  const[step,setStep]=useState(0);
  const[busy,setBusy]=useState(false);
  const[error,setError]=useState('');
  const[result,setResult]=useState('');
  const selected=beginnerProviders[service];

  useEffect(()=>{
    let active=true;
    api.validateConfig(initial).then(preview=>{
      if(!active)return;
      if(!preview.config){setError(preview.error||'配置无法读取，请使用完整配置页修复。');return}
      setBase(preview.config);
      const choice=initialBeginnerProvider(preview.config);
      setService(choice.index);
      setModel(choice.model);
      setEndpoint(choice.endpoint);
      setKey(choice.key);
    }).catch(e=>{if(active)setError(String(e))});
    return()=>{active=false};
  },[initial]);

  const choose=(index:number)=>{
    const next=beginnerProviders[index];
    if(!next)return;
    setService(index);
    setModel(next.model);
    setEndpoint(next.endpoint);
    setKey('');
    setError('');
  };

  const save=async()=>{
    if(!base||!model.trim()||!endpoint.trim()||(selected.needsKey&&!key.trim())){
      setError(`请填写${selected.editableEndpoint?'Base URL、':''}模型名称，以及此服务所需的 API Key。`);
      return;
    }
    setBusy(true);
    setError('');
    try{
      const rendered=await api.renderConfig({...base,endpoint:endpoint.trim(),model:model.trim(),api_key:key.trim(),api_type:'auto'});
      if(!rendered.valid)throw new Error(rendered.error||'配置无效');
      await api.saveConfig(rendered.toml);
      await done();
      setStep(2);
      try{
        const models=await api.models();
        setResult(models.length?`模型列表可访问，共 ${models.length} 个模型。现在可以发送一个问题检验实际对话。`:'服务已响应，但模型列表为空。请确认模型名称，再发送一个问题检验实际对话。');
      }catch(e){
        setResult(`配置已保存，但模型列表检查未通过：${String(e)}。请检查密钥、网络或模型服务。`);
      }
    }catch(e){setError(String(e))}finally{setBusy(false)}
  };

  return <div class="backdrop"><div class="modal beginner" role="dialog" aria-modal="true" aria-label="快速开始">
    <div class="modal-heading"><h2>快速开始</h2><button onClick={close} aria-label="关闭快速开始">关闭</button></div>
    <p class="muted">三步连接模型。设备操作仍需安全检查和确认。此页面无需登录且对同一网络开放，请只在可信网络填写密钥。</p>
    <ol class="setup-steps"><li class={step===0?'active':''}>选服务</li><li class={step===1?'active':''}>填信息</li><li class={step===2?'active':''}>检查连接</li></ol>
    {step===0?<>
      <label>你使用哪个模型服务？<select value={service} onChange={e=>choose(Number(e.currentTarget.value))}>{beginnerProviders.map((item,index)=><option key={item.id} value={index}>{item.name}</option>)}</select></label>
      <div class="actions"><button class="primary" onClick={()=>setStep(1)}>下一步</button></div>
    </>:step===1?<>
      <label>模型名称<small>这是服务中的具体 AI；有推荐值时已预填，也可按服务商提供的名称修改。</small><input value={model} onInput={e=>setModel(e.currentTarget.value)} placeholder={selected.id==='ollama'?'填写已安装的 Ollama 模型名称':'填写服务商提供的模型名称'}/></label>
      {selected.editableEndpoint&&<label>Base URL<input value={endpoint} onInput={e=>setEndpoint(e.currentTarget.value)} placeholder="https://example.com/v1"/>{selected.id==='ollama'&&<small>本机指运行 nl2sh 的设备，不是浏览器所在的电脑。</small>}</label>}
      {selected.id!=='ollama'&&<label>API Key{selected.id==='custom'&&<small>本机兼容服务可留空；远程服务通常需要密钥。</small>}<input type="password" autocomplete="off" value={key} onInput={e=>setKey(e.currentTarget.value)} placeholder="从模型服务商获取"/><small>密钥只保存到设备配置，不会写入对话记录。</small></label>}
      <div class="actions"><button onClick={()=>setStep(0)}>上一步</button><button class="primary" disabled={busy||!base} onClick={save}>{busy?'正在保存和检查…':'保存并检查连接'}</button></div>
    </>:<><p role="status">{result}</p><div class="actions"><button class="primary" onClick={close}>开始使用</button><button onClick={()=>setStep(1)}>修改设置</button></div></>}
    {error&&<p class="bad" role="alert">{error}</p>}
  </div></div>;
}
