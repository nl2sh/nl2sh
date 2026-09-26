import assert from 'node:assert/strict';
import test from 'node:test';
import {availableSession} from '../src/sessionRecovery.ts';

test('reconnect selects a saved session when the active one disappeared', () => {
  const saved=[{id:'saved',title:'已保存',turns:1,busy:false,pending:false,updated:1}];
  assert.equal(availableSession('lost-running',saved),'saved');
  assert.equal(availableSession('saved',saved),'saved');
  assert.equal(availableSession('lost-running',[]),'');
});
