export type UpdateResult={current:string;latest:string|null;can_install:boolean;ownership:{owner:string;message:string}};
export type UpdateJob={busy:boolean;version:string|null;progress:{stage:string;downloaded:number;total:number}|null;error:string|null};
export function updatePercent(job:UpdateJob):number|null{
  const progress=job.progress;
  if(!progress||progress.total<=0)return null;
  return Math.min(100,Math.floor(progress.downloaded/progress.total*100));
}
export function updateStage(job:UpdateJob):string{
  if(job.error)return '更新失败';
  switch(job.progress?.stage){
    case 'checking':return '正在检查版本与签名发布清单…';
    case 'downloading':return '正在下载更新…';
    case 'signature':return '正在下载程序签名…';
    case 'verifying':return '正在校验签名、摘要和设备架构…';
    case 'installing':return '正在安装更新…';
    case 'complete':return `已安装 ${job.version}，请重启 nl2sh 后使用新版本。`;
    default:return '等待更新';
  }
}
export function hasUpdate(result:UpdateResult|null,job:UpdateJob|null,skipped:string):boolean{
  return Boolean(result?.latest&&result.latest!==skipped&&job?.progress?.stage!=='complete');
}
