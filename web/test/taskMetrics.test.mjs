import assert from 'node:assert/strict';
import test from 'node:test';
import {liveTaskMetrics,formatDuration} from '../src/taskMetrics.ts';

const task={timing_available:true,steps:3,tool_calls:4,total_ms:10000,model_ms:5000,tool_ms:3000,waiting_ms:1000};
test('running timing advances only the active phase and preserves the source snapshot',()=>{
  for(const [activity,field] of [['thinking','model_ms'],['tool','tool_ms'],['waiting','waiting_ms'],['background','waiting_ms']]){
    const snapshot={task,busy:true,activity,received_at_ms:1000};
    const result=liveTaskMetrics(snapshot,3500);
    assert.equal(result.total_ms,12500);
    for(const name of ['model_ms','tool_ms','waiting_ms'])assert.equal(result[name],task[name]+(name===field?2500:0));
  }
  assert.equal(task.total_ms,10000);
});
test('finished or cancelled timing stays frozen and legacy snapshots stay unknown',()=>{
  assert.deepEqual(liveTaskMetrics({task,busy:false,activity:'idle',received_at_ms:1000},10000),task);
  assert.equal(liveTaskMetrics({busy:false},1000),undefined);
  assert.equal(liveTaskMetrics({task,busy:true,activity:'cancelling',received_at_ms:1000},2000).model_ms,5000);
  assert.equal(formatDuration(1250),'1.3s');
  assert.equal(formatDuration(61250),'1分 1.3s');
});
