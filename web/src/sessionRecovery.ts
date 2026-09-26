import type {Session} from './types';

export function availableSession(current:string, sessions:Session[]):string {
  return sessions.some(session=>session.id===current)?current:sessions[0]?.id||'';
}
