import assert from 'node:assert/strict';
import test from 'node:test';
import {parseChart} from '../src/chartData.ts';
import {conversationRows} from '../src/conversation.ts';

const call = {kind: 'tool_call', text: 'create_chart'};
const chart = {chart_type: 'bar', title: '容量', source: 'android_storage result', unit: 'GiB', labels: ['应用', '媒体'], values: [2.5, 4]};

test('completed and restored tool rows expose chart data', () => {
  const entries = [call, {kind: 'tool_result', text: JSON.stringify(chart)}];
  const rows = conversationRows(entries);
  assert.equal(rows[0].kind, 'tool');
  assert.deepEqual(parseChart(rows[0].call, rows[0].result), chart);
});

test('chart rendering requires matching successful tool output and bounded numbers', () => {
  assert.equal(parseChart({kind: 'tool_call', text: 'read_file'}, {kind: 'tool_result', text: JSON.stringify(chart)}), null);
  assert.equal(parseChart(call, {kind: 'tool_error', text: JSON.stringify(chart)}), null);
  assert.equal(parseChart(call, {kind: 'tool_result', text: JSON.stringify({...chart, values: [1, -2]})}), null);
  assert.equal(parseChart(call, {kind: 'tool_result', text: JSON.stringify({...chart, values: [1]})}), null);
  assert.equal(parseChart(call, {kind: 'tool_result', text: 'truncated JSON'}), null);
});
