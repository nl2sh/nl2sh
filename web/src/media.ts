export function localMedia(target:string):{path:string;kind:'image'|'video'|'audio'}|null{
  let path=target;
  if(target.startsWith('/api/file-preview?'))path=new URL(target,'http://localhost').searchParams.get('path')||'';
  else {
    if(/^[a-z][a-z\d+.-]*:/i.test(target)||target.startsWith('//')||/[?#]/.test(target))return null;
    try{path=decodeURIComponent(target)}catch{return null}
  }
  if(!path||path.length>4096||/[\x00-\x1f\x7f]/.test(path))return null;
  const extension=path.split('.').pop()?.toLowerCase()||'';
  const kind=['png','jpg','jpeg','gif','webp','bmp','avif'].includes(extension)?'image':
    ['mp4','m4v','webm','ogv','mov'].includes(extension)?'video':
    ['wav','pcm','raw','mp3','m4a','aac','ogg','oga','flac'].includes(extension)?'audio':null;
  return kind?{path,kind}:null;
}
