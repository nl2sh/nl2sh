import type {ProtocolConnections} from './types';

export function connectionStatus(info:ProtocolConnections):string{
  if(info.state==='unknown')return '无法确认协议状态';
  if(info.state==='stopped')return '协议服务未启动';
  return info.transport==='http'?'HTTP 协议进程运行中':'本地 stdio MCP 进程运行中';
}
export function hasHttpEndpoint(info:ProtocolConnections):boolean{
  return info.state==='running'&&info.transport==='http'&&Boolean(info.mcp_url&&info.a2a_url&&info.agent_card_url);
}
export function httpMcpConfig(info:ProtocolConnections):string{
  const url=hasHttpEndpoint(info)?info.mcp_url:`${info.default_http_origin}/mcp`;
  return `[mcp_servers.nl2sh]\nurl = ${JSON.stringify(url)}\n${info.token&&hasHttpEndpoint(info)?`http_headers = { Authorization = ${JSON.stringify(`Bearer ${info.token}`)} }`:`bearer_token_env_var = ${JSON.stringify(info.token_env)}`}\ntool_timeout_sec = 210`;
}
export function stdioMcpConfig(info:ProtocolConnections):string{
  return `[mcp_servers.nl2sh]\ncommand = "nl2sh"\nargs = ${JSON.stringify(['--config',info.config_path,'protocol','stdio'])}`;
}
export const a2aMessage=JSON.stringify({jsonrpc:'2.0',id:'request-1',method:'SendMessage',params:{message:{messageId:'unique-message-1',role:'ROLE_USER',parts:[{text:'读取设备环境并总结'}]},configuration:{returnImmediately:true}}},null,2);
