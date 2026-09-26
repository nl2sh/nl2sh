import type {Entry} from './types';

export type ConversationRow =
  | {kind: 'entry'; entry: Entry}
  | {kind: 'tool'; call: Entry | null; result: Entry | null};

export interface TaskEvidence {completed:string[];partial:string[];failed:string[];pending:number}

export function taskEvidence(rows: readonly ConversationRow[], end: number): TaskEvidence {
  const completed:string[]=[];
  const partial:string[]=[];
  const failed:string[]=[];
  let pending=0;
  for(let index=end-1;index>=0;index--){
    const row=rows[index];
    if(row.kind==='entry'&&row.entry.kind==='user')break;
    if(row.kind!=='tool')continue;
    const name=row.call?.text||'工具';
    if(!row.result)pending++;
    else if(row.result.kind==='tool_error')failed.unshift(name);
    else {
      let status='';
      try{const value=JSON.parse(row.result.text);if(value&&typeof value.status==='string')status=value.status}catch{/* Tool results may be plain text. */}
      if(!status)status=/^status=(partial|failed|timed_out|complete)(?:\s|$)/m.exec(row.result.text)?.[1]||'';
      if(status==='partial')partial.unshift(name);
      else if(status==='failed'||status==='timed_out')failed.unshift(name);
      else completed.unshift(name);
    }
  }
  return {completed,partial,failed,pending};
}

export function displayToolOutput(text: string): string {
  return text.replace(/(\\+)n/g, (match, slashes: string) =>
    slashes.length % 2 === 1 ? `${slashes.slice(1)}\n` : match);
}

export function toggleToolExpansion(cards: Iterable<{open: boolean}>): boolean {
  const items = Array.from(cards);
  if (items.length === 0) return false;
  const expand = items.some(item => !item.open);
  for (const item of items) item.open = expand;
  return true;
}

export function conversationRows(entries: readonly Entry[]): ConversationRow[] {
  const rows: ConversationRow[] = [];
  for (const entry of entries) {
    if (entry.kind === 'tool_call') {
      rows.push({kind: 'tool', call: entry, result: null});
    } else if (entry.kind === 'tool_result' || entry.kind === 'tool_error') {
      const previous = rows.at(-1);
      if (previous?.kind === 'tool' && previous.call && !previous.result) {
        previous.result = entry;
      } else {
        rows.push({kind: 'tool', call: null, result: entry});
      }
    } else {
      rows.push({kind: 'entry', entry});
    }
  }
  return rows;
}
