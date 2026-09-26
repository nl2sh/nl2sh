import assert from 'node:assert/strict';
import test from 'node:test';
import {displayTerminalResponse} from '../src/terminalOutput.ts';

test('terminal output decodes JSON newlines and keeps status separate',()=>{
  const response=JSON.stringify({stdout:'first\nsecond\n',stderr:'warning\nnext\n',exit_code:2,timed_out:false,interrupted:false});
  assert.equal(displayTerminalResponse(response),'标准输出：\nfirst\nsecond\n标准错误：\nwarning\nnext\n退出码：2');
});

test('terminal errors remain readable without a result object',()=>{
  assert.equal(displayTerminalResponse('error: command rejected'),'error: command rejected');
  assert.equal(displayTerminalResponse('{"other":"value"}'),'{"other":"value"}');
  assert.equal(displayTerminalResponse(JSON.stringify({stdout:'',stderr:'',exit_code:0,timed_out:false,interrupted:false})),'无输出\n退出码：0');
});
