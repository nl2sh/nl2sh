import test from 'node:test';
import assert from 'node:assert/strict';
import {connectionStatus,hasHttpEndpoint,httpMcpConfig,stdioMcpConfig} from '../src/protocolConnections.ts';

const stopped={config_path:"/device/config with ' quote.toml",state:'stopped',transport:null,mcp_url:null,a2a_url:null,agent_card_url:null,default_http_origin:'http://127.0.0.1:8765',token_env:'NL2SH_PROTOCOL_TOKEN'};
test('stopped and stdio services offer explicit default examples rather than active HTTP',()=>{
  assert.equal(connectionStatus(stopped),'协议服务未启动');
  assert.equal(hasHttpEndpoint(stopped),false);
  assert.match(httpMcpConfig(stopped),/127\.0\.0\.1:8765\/mcp/);
  const stdio={...stopped,state:'running',transport:'stdio'};
  assert.equal(connectionStatus(stdio),'本地 stdio MCP 进程运行中');
  assert.equal(hasHttpEndpoint(stdio),false);
  const config=stdioMcpConfig(stdio);
  assert.deepEqual(JSON.parse(config.split('args = ')[1]),['--config',stopped.config_path,'protocol','stdio']);
});
test('HTTP client configuration preserves the advertised custom HTTPS endpoint',()=>{
  const info={...stopped,state:'running',transport:'http',mcp_url:'https://agent.example:9443/mcp',a2a_url:'https://agent.example:9443/a2a',agent_card_url:'https://agent.example:9443/.well-known/agent-card.json'};
  assert.equal(hasHttpEndpoint(info),true);
  assert.match(httpMcpConfig(info),/https:\/\/agent\.example:9443\/mcp/);
  assert.match(httpMcpConfig(info),/bearer_token_env_var = "NL2SH_PROTOCOL_TOKEN"/);
  assert.equal(hasHttpEndpoint({...info,state:'unknown'}),false);
});
