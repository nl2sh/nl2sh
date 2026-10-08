import type {Snapshot,TaskMetrics} from './types.ts';

export function formatDuration(milliseconds:number):string {
  const seconds=Math.max(0,milliseconds)/1000;
  if(seconds<60)return `${seconds.toFixed(1)}s`;
  return `${Math.floor(seconds/60)}分 ${(seconds%60).toFixed(1)}s`;
}

export function liveTaskMetrics(state:Snapshot,now:number):TaskMetrics|undefined {
  if(!state.task)return undefined;
  const task={...state.task};
  if(!state.busy)return task;
  const elapsed=Math.max(0,now-(state.received_at_ms??now));
  task.total_ms+=elapsed;
  if(state.activity==='thinking')task.model_ms+=elapsed;
  else if(state.activity==='tool')task.tool_ms+=elapsed;
  else if((state.activity==='waiting'||state.activity==='background'))task.waiting_ms+=elapsed;
  return task;
}
