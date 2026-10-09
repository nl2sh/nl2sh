import {render} from 'preact';
import {useEffect,useRef} from 'preact/hooks';
import {markdown} from './markdown';
import {localMedia} from './media';
import {MediaPreview} from './mediaPreview';
export function Markdown({text}:{text:string}){
  const container=useRef<HTMLDivElement>(null);
  useEffect(()=>{
    const root=container.current;
    if(!root)return;
    root.innerHTML=markdown(text);
    const targets=Array.from(root.querySelectorAll<HTMLElement>('[data-media-path]'));
    for(const target of targets){
      const media=localMedia(target.dataset.mediaPath||'');
      if(media)render(<MediaPreview file={{name:media.path.split('/').pop()||media.path,path:media.path,is_dir:false,size:null,modified_ms:null,preview_kind:media.kind}}/>,target);
    }
    return()=>{for(const target of targets)render(null,target)};
  },[text]);
  return <div ref={container}/>;
}
