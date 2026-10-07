import test from 'node:test';
import assert from 'node:assert/strict';
import {optionalGroups,canUseToolPrompt,availabilityLabel} from '../src/toolAvailability.ts';

test('groups follow runtime inventory, including future groups',()=>{
  const tools=[{name:'read',enabled:true},{name:'one',group:'future',enabled:true},{name:'two',group:'future',enabled:false},{name:'dex',group:'jadx',enabled:false}];
  const groups=optionalGroups(tools);
  assert.deepEqual(groups.map(group=>group.id),['future','jadx']);
  assert.equal(groups[0].members.length,2);
});

test('configuration and discovered availability remain independent',()=>{
  assert.equal(canUseToolPrompt({enabled:true,available:false}),false);
  assert.equal(canUseToolPrompt({enabled:false,available:true}),false);
  assert.equal(canUseToolPrompt({enabled:true,available:true}),true);
  assert.equal(availabilityLabel({available:false}),'当前环境不可用');
  assert.equal(availabilityLabel({available:true}),'当前环境可用');
  assert.equal(availabilityLabel({}),'可用性未报告');
});
