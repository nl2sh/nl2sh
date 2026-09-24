import type {Entry} from './types';

export type ConversationRow =
  | {kind: 'entry'; entry: Entry}
  | {kind: 'tool'; call: Entry | null; result: Entry | null};

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
