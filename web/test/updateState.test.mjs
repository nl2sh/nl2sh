import assert from 'node:assert/strict';
import test from 'node:test';
import {hasUpdate,updatePercent,updateStage} from '../src/updateState.ts';
const result={current:'1.1.0',latest:'1.2.0',can_install:true,ownership:{owner:'standalone',message:''}};
const job={busy:true,version:'1.2.0',error:null,progress:{stage:'downloading',downloaded:25,total:100}};
test('update badge remembers exactly the skipped version and stops after installation',()=>{
  assert.equal(hasUpdate(result,null,''),true);
  assert.equal(hasUpdate(result,null,'1.2.0'),false);
  assert.equal(hasUpdate(result,null,'1.1.1'),true);
  assert.equal(hasUpdate({...result,latest:null},null,''),false);
  assert.equal(hasUpdate(result,{...job,busy:false,progress:{...job.progress,stage:'complete'}},''),false);
});
test('download progress uses real byte counters and completion still requires a restart',()=>{
  assert.equal(updatePercent(job),25);
  assert.equal(updatePercent({...job,progress:{...job.progress,total:0}}),null);
  assert.equal(updatePercent({...job,progress:{...job.progress,downloaded:101}}),100);
  assert.match(updateStage({...job,error:'network failure'}),/更新失败/);
  assert.match(updateStage({...job,progress:{...job.progress,stage:'verifying'}}),/签名、摘要和设备架构/);
  assert.match(updateStage({...job,busy:false,progress:{...job.progress,stage:'complete'}}),/1.2.0.*重启/);
});
