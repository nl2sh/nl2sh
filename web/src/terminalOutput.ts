interface TerminalResult {
  stdout:string;
  stderr:string;
  exit_code:number|null;
  timed_out:boolean;
  interrupted:boolean;
}

export function displayTerminalResponse(text:string):string{
  let value:unknown;
  try{value=JSON.parse(text)}catch{return text}
  if(!value||typeof value!=='object'||Array.isArray(value))return text;
  const result=value as Partial<TerminalResult>;
  if(typeof result.stdout!=='string'||typeof result.stderr!=='string')return text;
  const sections:string[]=[];
  if(result.stdout)sections.push(`标准输出：\n${result.stdout.replace(/\n+$/,'')}`);
  if(result.stderr)sections.push(`标准错误：\n${result.stderr.replace(/\n+$/,'')}`);
  if(!sections.length)sections.push('无输出');
  sections.push(`退出码：${typeof result.exit_code==='number'?result.exit_code:'未知'}`);
  if(result.timed_out)sections.push('命令已超时');
  if(result.interrupted)sections.push('命令已中断');
  return sections.join('\n');
}
