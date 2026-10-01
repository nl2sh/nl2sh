import assert from 'node:assert/strict';
import test from 'node:test';
import {clampPanelWidth} from '../src/panelLayout.ts';

test('desktop panels keep at least 480 pixels for the chat',()=>{
  assert.equal(clampPanelWidth(620,1280),620);
  assert.equal(clampPanelWidth(620,900),362);
  assert.equal(clampPanelWidth(100,900),230);
});

test('narrow screens size the overlay to the space beside the activity bar',()=>{
  assert.equal(clampPanelWidth(620,390),342);
  assert.equal(clampPanelWidth(350,320),272);
});
