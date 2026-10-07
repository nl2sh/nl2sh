import type {RuntimeInfo} from './types';
export function updateGuidance(owner:string):string{
  switch(owner){
    case 'nl2sh-helper':return '由 nl2sh Helper 管理，请在 Helper 中升级。';
    case 'termux-apt':return '由 Termux APT 管理，请使用 pkg upgrade nl2sh。';
    case 'standalone':return '独立安装，可使用 nl2sh update。';
    default:return '安装归属无法验证，请先检查安装记录。';
  }
}
export function runtimeComponents(info:RuntimeInfo):{name:string;installed:string;expected:string;drift:boolean}[]{
  const manifest=info.runtime_policy.status==='verified'?info.runtime_policy.manifest:undefined;
  return [
    {name:'Android Bridge',installed:info.capabilities.bridge?.app_version||'未发现',expected:manifest?.android_bridge?.version||'未知'},
    {name:'JADX helper',installed:info.capabilities.jadx?.helper_version||'未发现',expected:manifest?.jadx_helper?.version||'未知'},
  ].map(row=>({...row,drift:row.expected!=='未知'&&row.installed!==row.expected}));
}
