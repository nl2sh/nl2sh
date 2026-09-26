export function formatSessionCreated(createdSeconds:number,nowMilliseconds=Date.now()):string{
  if(!Number.isFinite(createdSeconds)||createdSeconds<=0)return '日期未知';
  const elapsed=Math.max(0,Math.floor(nowMilliseconds/1000)-createdSeconds);
  if(elapsed<60)return '刚刚';
  if(elapsed<3600)return `${Math.floor(elapsed/60)} 分钟前`;
  if(elapsed<86400)return `${Math.floor(elapsed/3600)} 小时前`;
  const date=new Date(createdSeconds*1000);
  const two=(value:number)=>String(value).padStart(2,'0');
  return `${date.getFullYear()}-${two(date.getMonth()+1)}-${two(date.getDate())} ${two(date.getHours())}:${two(date.getMinutes())}`;
}
