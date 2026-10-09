import {useState} from 'preact/hooks';
import {fileUrl} from './api';
import type {BrowserFile} from './api';
import {AudioPreview} from './audioPreview';
export function MediaPreview({file}:{file:BrowserFile}){
  const [error,setError]=useState('');
  return <span class="media-preview">
    {file.preview_kind==='image'&&<img loading="lazy" src={fileUrl(file.path)} alt={file.name} onError={()=>setError('图片加载失败')}/>}
    {file.preview_kind==='video'&&<video src={fileUrl(file.path)} controls preload="metadata" onError={()=>setError('视频无法播放或浏览器不支持该格式')}/>}
    {file.preview_kind==='audio'&&<AudioPreview file={file}/>}
    {error&&<span class="bad" role="alert">{error}</span>}
  </span>;
}
