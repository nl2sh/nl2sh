import type {ToolInfo} from './types';

export function optionalGroups(tools:ToolInfo[]):{id:string;members:ToolInfo[]}[]{
  const groups=new Map<string,ToolInfo[]>();
  for(const tool of tools){
    if(!tool.group)continue;
    const members=groups.get(tool.group)||[];
    members.push(tool);
    groups.set(tool.group,members);
  }
  return [...groups].map(([id,members])=>({id,members}));
}

export function canUseToolPrompt(tool:ToolInfo):boolean{
  return tool.enabled&&tool.available!==false;
}

export function availabilityLabel(tool:ToolInfo):string{
  return tool.available===true?'当前环境可用':tool.available===false?'当前环境不可用':'可用性未报告';
}
