import {useEffect,useRef,useState} from 'preact/hooks';
import {fileUrl} from './api';
import type {BrowserFile} from './api';
import {decodePcm,parseWavHeader,pcmFormats} from './audioPcm';
import type {PcmSettings} from './audioPcm';

const MAX_CUSTOM_BYTES=32*1024*1024;
const defaultSettings:PcmSettings={sampleRate:44100,channels:1,format:'s16le'};
const sampleRates=[8000,11025,16000,22050,32000,44100,48000,96000,192000];
const bits:Record<PcmSettings['format'],number>={u8:8,s16le:16,s24le:24,s32le:32,f32le:32};

export function AudioPreview({file}:{file:BrowserFile}){
  const wav=/\.wav$/i.test(file.name);
  const raw=/\.(pcm|raw)$/i.test(file.name);
  const [settings,setSettings]=useState<PcmSettings>(defaultSettings);
  const [dataOffset,setDataOffset]=useState(0),[dataLength,setDataLength]=useState<number|null>(null);
  const [headerError,setHeaderError]=useState(''),[error,setError]=useState('');
  const [playing,setPlaying]=useState(false),[loading,setLoading]=useState(false);
  const context=useRef<AudioContext|null>(null),source=useRef<AudioBufferSourceNode|null>(null),request=useRef<AbortController|null>(null);

  const stop=()=>{
    request.current?.abort();request.current=null;
    if(source.current){source.current.onended=null;source.current.stop();source.current.disconnect();source.current=null}
    void context.current?.close();context.current=null;
    setPlaying(false);setLoading(false);
  };
  useEffect(()=>()=>stop(),[]);
  useEffect(()=>{
    if(!wav)return;
    const controller=new AbortController();
    fetch(fileUrl(file.path),{headers:{Range:'bytes=0-65535'},signal:controller.signal,cache:'no-store'})
      .then(async response=>{if(!response.ok)throw new Error(await response.text());return response.arrayBuffer()})
      .then(buffer=>{const header=parseWavHeader(buffer);setSettings({sampleRate:header.sampleRate,channels:header.channels,format:header.format});setDataOffset(header.dataOffset);setDataLength(header.dataLength)})
      .catch(e=>{if(!controller.signal.aborted)setHeaderError(String(e))});
    return()=>controller.abort();
  },[file.path,wav]);

  const play=async()=>{
    if(playing||loading){stop();return}
    if((file.size||0)>MAX_CUSTOM_BYTES){setError('参数播放仅支持不超过 32 MiB 的文件；可尝试上方直接播放。');return}
    setError('');setLoading(true);
    const controller=new AbortController();request.current=controller;
    try{
      // Create/resume from the click gesture so browsers permit audio playback.
      const audioContext=new AudioContext();context.current=audioContext;
      await audioContext.resume();
      const response=await fetch(fileUrl(file.path),{signal:controller.signal,cache:'no-store'});
      if(!response.ok)throw new Error(await response.text());
      const bytes=await response.arrayBuffer();
      if(controller.signal.aborted)return;
      const header=wav?parseWavHeader(bytes):null;
      const offset=header?.dataOffset??0;
      const length=header?Math.min(header.dataLength,bytes.byteLength-offset):bytes.byteLength;
      const channels=decodePcm(bytes,settings,offset,length);
      const buffer=audioContext.createBuffer(settings.channels,channels[0].length,settings.sampleRate);
      channels.forEach((values,index)=>buffer.getChannelData(index).set(values));
      const node=audioContext.createBufferSource();
      node.buffer=buffer;node.connect(audioContext.destination);
      node.onended=()=>{if(source.current===node){source.current=null;void audioContext.close();context.current=null;setPlaying(false)}};
      source.current=node;node.start();setPlaying(true);
    }catch(e){if(!controller.signal.aborted){setError(String(e));stop()}}
    finally{if(request.current===controller)request.current=null;setLoading(false)}
  };
  const change=(value:Partial<PcmSettings>)=>{stop();setSettings(current=>({...current,...value}))};
  const bitrate=settings.sampleRate*settings.channels*bits[settings.format];
  return <div class="audio-preview">
    {!raw&&<div><p class="file-preview-meta">浏览器直接播放</p><audio controls preload="metadata" src={fileUrl(file.path)} onError={()=>setError('浏览器无法解码此音频格式')}/></div>}
    {(raw||wav)&&<div class="pcm-controls">
      <h3>{wav?'按指定参数播放 WAV':'播放原始 PCM'}</h3>
      {wav&&<p class="panel-hint">WAV 头部参数会自动填入；修改后可用新的采样率、声道或格式解释数据。</p>}
      {raw&&<p class="panel-hint">原始 PCM 没有格式头，请按文件实际参数选择。</p>}
      {headerError&&<p class="bad">无法读取 WAV 参数：{headerError}</p>}
      <div class="pcm-fields">
        <label>采样率（Hz）<input type="number" min="3000" max="192000" step="1" list="pcm-rates" value={settings.sampleRate} onInput={e=>change({sampleRate:Number(e.currentTarget.value)})}/><datalist id="pcm-rates">{sampleRates.map(rate=><option value={rate}/>)}</datalist></label>
        <label>声道数<select value={settings.channels} onChange={e=>change({channels:Number(e.currentTarget.value)})}>{[1,2,3,4,5,6,7,8].map(count=><option value={count}>{count}</option>)}</select></label>
        <label>采样格式<select value={settings.format} onChange={e=>change({format:e.currentTarget.value as PcmSettings['format']})}>{pcmFormats.map(format=><option value={format.value}>{format.label}</option>)}</select></label>
      </div>
      <p class="file-preview-meta">对应 PCM 比特率：{(bitrate/1000).toLocaleString()} kbps{wav&&dataLength!==null?` · 数据 ${dataLength.toLocaleString()} 字节，偏移 ${dataOffset}`:''}</p>
      {(file.size||0)>MAX_CUSTOM_BYTES&&<p class="panel-hint">参数播放仅支持不超过 32 MiB 的文件。{wav?'可使用上方直接播放。':''}</p>}
      <button type="button" onClick={play} disabled={Boolean(headerError&&wav)||((file.size||0)>MAX_CUSTOM_BYTES&&!playing)}>{loading?'取消读取':playing?'停止播放':'按所选参数播放'}</button>
    </div>}
    {error&&<p class="bad" role="alert">{error}</p>}
  </div>;
}
