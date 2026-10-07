import test from 'node:test';
import assert from 'node:assert/strict';
import {runtimeComponents,updateGuidance} from '../src/runtimeStatus.ts';
test('runtime recommendations require a verified manifest and distinguish drift',()=>{
  const info={runtime_policy:{status:'unsigned_source_build',manifest:{android_bridge:{version:'9'}}},capabilities:{bridge:{app_version:'1'}}};
  assert.equal(runtimeComponents(info)[0].expected,'未知');
  assert.equal(runtimeComponents(info)[0].drift,false);
  info.runtime_policy.status='verified';
  assert.equal(runtimeComponents(info)[0].drift,true);
  assert.equal(runtimeComponents(info)[1].installed,'未发现');
});
test('installation owner selects its actual updater',()=>{
  assert.match(updateGuidance('nl2sh-helper'),/Helper 中升级/);
  assert.match(updateGuidance('termux-apt'),/pkg upgrade/);
  assert.match(updateGuidance('standalone'),/nl2sh update/);
  assert.match(updateGuidance('unknown'),/无法验证/);
});
