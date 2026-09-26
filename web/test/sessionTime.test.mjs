import assert from 'node:assert/strict';
import {test} from 'node:test';
import {formatSessionCreated} from '../src/sessionTime.ts';

test('recent session creation time changes from minutes to hours',()=>{
  const now=2_000_000_000_000;
  assert.equal(formatSessionCreated(now/1000-20,now),'刚刚');
  assert.equal(formatSessionCreated(now/1000-61,now),'1 分钟前');
  assert.equal(formatSessionCreated(now/1000-7200,now),'2 小时前');
});

test('sessions at least one day old show a local date and time',()=>{
  const now=2_000_000_000_000;
  assert.match(formatSessionCreated(now/1000-86400,now),/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
});
