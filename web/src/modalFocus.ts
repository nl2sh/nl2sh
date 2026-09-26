import {useEffect,useRef} from 'preact/hooks';

export function useModalFocus(onEscape?:()=>void){
  const dialog=useRef<HTMLDivElement>(null);
  const escape=useRef(onEscape);
  escape.current=onEscape;
  useEffect(()=>{
    const previous=document.activeElement instanceof HTMLElement?document.activeElement:null;
    const node=dialog.current;
    node?.focus();
    const onKey=(event:KeyboardEvent)=>{
      if(!node)return;
      const dialogs=document.querySelectorAll('.modal[role="dialog"]');
      if(dialogs[dialogs.length-1]!==node)return;
      if(event.key==='Escape'&&escape.current){event.preventDefault();escape.current();return}
      if(event.key!=='Tab')return;
      const items=Array.from(node.querySelectorAll<HTMLElement>('button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])')).filter(item=>item.getClientRects().length>0);
      if(items.length===0){event.preventDefault();node.focus();return}
      const first=items[0],last=items[items.length-1];
      if(event.shiftKey&&(document.activeElement===first||document.activeElement===node)){event.preventDefault();last.focus()}
      else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus()}
    };
    document.addEventListener('keydown',onKey);
    return()=>{document.removeEventListener('keydown',onKey);previous?.focus()};
  },[]);
  return dialog;
}
