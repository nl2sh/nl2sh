import assert from 'node:assert/strict';
import test from 'node:test';
import {conversationRows, displayToolOutput, taskEvidence, toggleToolExpansion} from '../src/conversation.ts';

test('F2 toggles all tool cards while individual cards remain independent', () => {
  const cards = [{open: false}, {open: false}];
  assert.equal(toggleToolExpansion(cards), true);
  assert.deepEqual(cards.map(card => card.open), [true, true]);
  cards[0].open = false;
  assert.equal(toggleToolExpansion(cards), true);
  assert.deepEqual(cards.map(card => card.open), [true, true]);
  assert.equal(toggleToolExpansion(cards), true);
  assert.deepEqual(cards.map(card => card.open), [false, false]);
  assert.equal(toggleToolExpansion([]), false);
});

test('escaped newlines in tool output display as line breaks without changing source', () => {
  const output = 'first\\n\\nsecond\nthird';
  assert.equal(displayToolOutput(output), 'first\n\nsecond\nthird');
  assert.equal(output, 'first\\n\\nsecond\nthird');
  assert.equal(displayToolOutput(String.raw`literal \\n stays`), String.raw`literal \\n stays`);
});

test('each tool call keeps its own result and order', () => {
  const entries = [
    {kind: 'user', text: '检查设备'},
    {kind: 'tool_call', text: 'inspect_android'},
    {kind: 'tool_result', text: '设备正常'},
    {kind: 'tool_call', text: 'read_file'},
    {kind: 'tool_error', text: '读取失败'},
    {kind: 'assistant', text: '完成'},
  ];
  assert.deepEqual(conversationRows(entries), [
    {kind: 'entry', entry: entries[0]},
    {kind: 'tool', call: entries[1], result: entries[2]},
    {kind: 'tool', call: entries[3], result: entries[4]},
    {kind: 'entry', entry: entries[5]},
  ]);
});

test('missing or standalone results and live output stay separate', () => {
  const entries = [
    {kind: 'tool_output', text: '[OUT] running'},
    {kind: 'tool_call', text: 'unfinished'},
    {kind: 'tool_call', text: 'next'},
    {kind: 'tool_result', text: 'done'},
    {kind: 'tool_error', text: 'orphaned'},
  ];
  assert.deepEqual(conversationRows(entries), [
    {kind: 'entry', entry: entries[0]},
    {kind: 'tool', call: entries[1], result: null},
    {kind: 'tool', call: entries[2], result: entries[3]},
    {kind: 'tool', call: null, result: entries[4]},
  ]);
});

test('task evidence separates complete, partial and failed tools within one user turn', () => {
  const rows=conversationRows([
    {kind:'user',text:'first'},
    {kind:'tool_call',text:'old'},
    {kind:'tool_result',text:'ok'},
    {kind:'user',text:'second'},
    {kind:'tool_call',text:'storage'},
    {kind:'tool_result',text:'{"status":"complete"}'},
    {kind:'tool_call',text:'network'},
    {kind:'tool_result',text:'executed_command=foo\nstatus=partial exit=Some(1)\nstdout:\nhello'},
    {kind:'tool_call',text:'file'},
    {kind:'tool_error',text:'denied'},
    {kind:'assistant',text:'some results'},
  ]);
  assert.deepEqual(taskEvidence(rows,rows.length-1),{completed:['storage'],partial:['network'],failed:['file'],pending:0});
});
