export interface ActiveReference {start:number;fragment:string}

export function activeReference(text:string,cursor:number):ActiveReference|null {
  const before=text.slice(0,cursor);
  const lastSpace=Array.from(before.matchAll(/\s/g)).at(-1);
  const start=lastSpace?lastSpace.index!+lastSpace[0].length:0;
  const token=before.slice(start);
  return token.startsWith('@')?{start,fragment:token.slice(1)}:null;
}

export function completeReference(text:string,cursor:number,reference:ActiveReference,path:string):{text:string;cursor:number} {
  const insertion=`@${path}`;
  return {text:text.slice(0,reference.start)+insertion+text.slice(cursor),cursor:reference.start+insertion.length};
}
