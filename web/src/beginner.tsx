import {useEffect,useRef,useState} from 'preact/hooks';
import {api} from './api';
import {beginnerProviders,initialBeginnerProvider} from './providerPresets';
import {useModalFocus} from './modalFocus';

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
  const[models,setModels]=useState<string[]>([]);
  const[modelListOpen,setModelListOpen]=useState(false);
  const[loadingModels,setLoadingModels]=useState(false);
  const[modelNotice,setModelNotice]=useState('');
  const modelRequest=useRef(0);
  const selected=beginnerProviders[service];
  const dialog=useModalFocus(close);

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
    clearModels();
  };

  const clearModels=()=>{
    modelRequest.current++;
    setModels([]);
    setModelListOpen(false);
    setLoadingModels(false);
    setModelNotice('');
  };

  const fetchModels=async()=>{
    if(!endpoint.trim()||(selected.needsKey&&!key.trim())){
      setModelNotice('请先填写 Base URL 和此服务所需的 API Key。');
      return;
    }
    const requestId=++modelRequest.current;
    setLoadingModels(true);
    setModelNotice('');
    try{
      const found=await api.draftModels({endpoint:endpoint.trim(),api_key:key.trim()});
      if(requestId!==modelRequest.current)return;
      setModels(found.map(item=>item.id));
      setModelListOpen(found.length>0);
      setModelNotice(found.length?`已获取 ${found.length} 个模型，可从输入框选择或继续手动输入。`:'服务没有返回模型列表，可手动输入模型名称。');
    }catch(e){
      if(requestId!==modelRequest.current)return;
      setModels([]);
      setModelListOpen(false);
      setModelNotice(`获取模型列表失败：${String(e)}。仍可手动输入模型名称。`);
    }finally{
      if(requestId===modelRequest.current)setLoadingModels(false);
    }
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
        await api.checkModel();
        setResult('模型已成功回复测试请求，现在可以开始对话。');
      }catch(e){
        setResult(`配置已保存，但模型对话检查未通过：${String(e)}。请检查密钥、网络、模型名称或服务地址。`);
      }
    }catch(e){setError(String(e))}finally{setBusy(false)}
  };

  return <div class="backdrop"><div ref={dialog} tabIndex={-1} class="modal beginner" role="dialog" aria-modal="true" aria-label="快速开始">
    <div class="modal-heading"><h2>快速开始</h2><button onClick={close} aria-label="关闭快速开始">关闭</button></div>
    <p class="muted">三步连接模型。设备操作仍需安全检查和确认。此页面无需登录且对同一网络开放，请只在可信网络填写密钥。</p>
    <ol class="setup-steps"><li class={step===0?'active':''}>选服务</li><li class={step===1?'active':''}>填信息</li><li class={step===2?'active':''}>测试对话</li></ol>
    {step===0?<>
      <label>你使用哪个模型服务？<select value={service} onChange={e=>choose(Number(e.currentTarget.value))}>{beginnerProviders.map((item,index)=><option key={item.id} value={index}>{item.name}</option>)}</select></label>
      <div class="actions"><button class="primary" onClick={()=>setStep(1)}>下一步</button></div>
    </>:step===1?<>
      {selected.editableEndpoint&&<label>Base URL<input value={endpoint} onInput={e=>{setEndpoint(e.currentTarget.value);clearModels()}} placeholder="https://example.com/v1"/>{selected.id==='ollama'&&<small>本机指运行 nl2sh 的设备，不是浏览器所在的电脑。</small>}</label>}
      {selected.id!=='ollama'&&<label>API Key{selected.id==='custom'&&<small>本机兼容服务可留空；远程服务通常需要密钥。</small>}<input type="password" autocomplete="off" value={key} onInput={e=>{setKey(e.currentTarget.value);clearModels()}} placeholder="从模型服务商获取"/><small>密钥只保存到设备配置，不会写入对话记录。</small></label>}
      <div class="beginner-model-field"><label for="beginner-model">模型名称<small>可以获取服务返回的模型并选择，也可以手动输入名称。</small></label><div class="beginner-model-picker" onKeyDown={e=>{if(e.key==='Escape'&&modelListOpen){e.stopPropagation();setModelListOpen(false)}}}><span class="beginner-model-row"><input id="beginner-model" value={model} onInput={e=>{setModel(e.currentTarget.value);setModelListOpen(false)}} onKeyDown={e=>{if(e.key==='ArrowDown'&&models.length)setModelListOpen(true)}} placeholder={selected.id==='ollama'?'填写已安装的 Ollama 模型名称':'填写服务商提供的模型名称'}/><button type="button" class="beginner-model-toggle" aria-label="展开模型列表" aria-expanded={modelListOpen} aria-controls="beginner-model-options" disabled={!models.length} onClick={()=>setModelListOpen(value=>!value)}>▾</button><button type="button" disabled={loadingModels} onClick={fetchModels}>{loadingModels?'获取中…':'获取模型列表'}</button></span>{modelListOpen&&<div id="beginner-model-options" class="beginner-model-options" role="listbox" aria-label="可用模型">{models.map(id=><button type="button" role="option" aria-selected={model===id} key={id} onClick={()=>{setModel(id);setModelListOpen(false)}}>{id}</button>)}</div>}</div>{modelNotice&&<small role="status">{modelNotice}</small>}</div>
      <div class="actions"><button onClick={()=>setStep(0)}>上一步</button><button class="primary" disabled={busy||!base} onClick={save}>{busy?'正在保存和检查…':'保存并检查连接'}</button></div>
    </>:<><p role="status">{result}</p><div class="actions"><button class="primary" onClick={close}>开始使用</button><button onClick={()=>setStep(1)}>修改设置</button></div></>}
    {error&&<p class="bad" role="alert">{error}</p>}
  </div></div>;
}
