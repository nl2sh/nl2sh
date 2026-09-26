export interface BeginnerProvider {
  id:string;
  name:string;
  endpoint:string;
  model:string;
  needsKey:boolean;
  editableEndpoint:boolean;
}

export const beginnerProviders:BeginnerProvider[]=[
  {id:'deepseek',name:'DeepSeek',endpoint:'https://api.deepseek.com',model:'deepseek-flash',needsKey:true,editableEndpoint:false},
  {id:'openrouter',name:'OpenRouter',endpoint:'https://openrouter.ai/api/v1',model:'openrouter/free',needsKey:true,editableEndpoint:false},
  {id:'openai',name:'OpenAI',endpoint:'https://api.openai.com/v1',model:'gpt-4o-mini',needsKey:true,editableEndpoint:false},
  {id:'moonshot',name:'Moonshot / Kimi',endpoint:'https://api.moonshot.cn/v1',model:'',needsKey:true,editableEndpoint:false},
  {id:'siliconflow',name:'SiliconFlow',endpoint:'https://api.siliconflow.cn/v1',model:'',needsKey:true,editableEndpoint:false},
  {id:'ollama',name:'Ollama（本机模型）',endpoint:'http://127.0.0.1:11434/v1',model:'',needsKey:false,editableEndpoint:true},
  {id:'custom',name:'其他自定义服务',endpoint:'',model:'',needsKey:false,editableEndpoint:true},
];

export function initialBeginnerProvider(config:{endpoint?:unknown;api_key?:unknown;model?:unknown}){
  const endpoint=String(config.endpoint||'');
  const key=String(config.api_key||'');
  const isUnconfiguredDefault=endpoint===beginnerProviders[1].endpoint&&!key.trim();
  const found=isUnconfiguredDefault?-1:beginnerProviders.findIndex(provider=>provider.endpoint===endpoint);
  const index=found>=0?found:endpoint&&!isUnconfiguredDefault?beginnerProviders.length-1:0;
  const preset=beginnerProviders[index];
  const existingModel=String(config.model||'');
  return {
    index,
    endpoint:index===beginnerProviders.length-1?endpoint:preset.endpoint,
    model:isUnconfiguredDefault||(!key.trim()&&existingModel==='openrouter/free'&&index!==1)?preset.model:existingModel||preset.model,
    key,
  };
}
