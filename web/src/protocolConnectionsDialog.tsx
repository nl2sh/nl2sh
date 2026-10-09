import {useEffect,useState} from 'preact/hooks';
import {api} from './api';
import {copyCode} from './codeCopy';
import {useModalFocus} from './modalFocus';
import {a2aMessage,connectionStatus,hasHttpEndpoint,httpMcpConfig,stdioMcpConfig} from './protocolConnections';
import type {ProtocolConnections} from './types';

function CopyValue({label,value}:{label:string;value:string}){
  const[notice,setNotice]=useState('');
  useEffect(()=>setNotice(''),[value]);
  return <div class="connection-value"><div class="connection-value-heading"><strong>{label}</strong><button type="button" onClick={async()=>{try{await copyCode(value);setNotice('已复制')}catch{setNotice('复制失败，请手动选择文字')}}}>复制</button></div><pre>{value}</pre><small role="status">{notice}</small></div>;
}

export function ProtocolConnectionsDialog({close}:{close:()=>void}){
  const dialog=useModalFocus(close);
  const[info,setInfo]=useState<ProtocolConnections|null>(null),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const refresh=async()=>{
    setBusy(true);setError('');
    try{setInfo(await api.connections())}catch(e){setInfo(null);setError(String(e))}finally{setBusy(false)}
  };
  useEffect(()=>{void refresh()},[]);
  const active=info&&hasHttpEndpoint(info);
  return <div class="backdrop"><div ref={dialog} tabIndex={-1} class="modal protocol-connections" role="dialog" aria-modal="true" aria-label="MCP / A2A 连接"><div class="modal-heading"><h2>MCP / A2A 连接</h2><button onClick={close}>关闭</button></div>
    <p>外部 Agent 直接连接设备。MCP 调用设备工具；A2A 向设备 Agent 委派任务，需配置模型。</p>
    <div class="overview-heading"><strong>{info?connectionStatus(info):busy?'正在读取连接信息…':'连接信息不可用'}</strong><button disabled={busy} onClick={refresh}>刷新状态</button></div>
    {error&&<p class="bad" role="alert">连接信息读取失败：{error}</p>}
    {info&&<>
      {info.version&&<p>nl2sh v{info.version} · MCP 2025-11-25 · A2A 1.0</p>}
      <p class="muted">在配置页开启“随 Web / TUI 服务启动 MCP / A2A”后，重启当前入口即可一并启动。这里显示同 UID、同配置的协议进程状态，客户端还需确认网络可达。</p>
      <section><h3>HTTP MCP / A2A</h3><p>{active?'以下为当前服务的公告地址。':'当前没有已确认的 HTTP 地址；以下仅为本机回环示例。默认启动后，刷新查看自动获取的设备 IP 地址。'}</p>
        <CopyValue label={active?'MCP · Streamable HTTP':'MCP · 默认示例'} value={active?info.mcp_url!:`${info.default_http_origin}/mcp`}/>
        <CopyValue label={active?'A2A · JSON-RPC 1.0':'A2A · 默认示例'} value={active?info.a2a_url!:`${info.default_http_origin}/a2a`}/>
        <CopyValue label={active?'Agent Card · 公开发现':'Agent Card · 默认示例'} value={active?info.agent_card_url!:`${info.default_http_origin}/.well-known/agent-card.json`}/>
        <p>HTTP 请求使用 <code>Authorization: Bearer &lt;token&gt;</code>。未设置 <code>{info.token_env}</code> 时，设备启动命令自动生成令牌并在启动终端打印；随后台服务启动时写入配置相邻的私有 config.service/service.log。把该值交给外部 Agent 或写入客户端同名环境变量。设置设备端变量可复用固定令牌（32–256 个可打印 ASCII 字符）。当前运行令牌可在下方复制，连接配置包含鉴权信息。</p>
        {active&&info.token&&<CopyValue label="Authorization · Bearer 令牌" value={`Bearer ${info.token}`}/>}
        <CopyValue label="设备另一个终端 · 默认 HTTP 启动命令（自动 IP）" value={info.http_command}/>
        <p>默认监听 0.0.0.0:8765、允许 HTTP，并自动获取设备 IPv4，通常无需设置 advertised-url。127.0.0.1 仅用于同设备客户端；仅本机使用时可加 --host 127.0.0.1。多网卡、VPN 或 HTTPS 代理可用 advertised-url 覆盖公告地址。HTTP 明文传输令牌，远程推荐 HTTPS。</p>
        <CopyValue label="可选：多网卡 / VPN 公告地址覆盖模板（替换 DEVICE_IP）" value={info.network_command}/>
        <details><summary>MCP 客户端配置{active?'':'（默认本机示例）'}</summary><CopyValue label="支持此格式的 MCP 客户端 · TOML" value={httpMcpConfig(info)}/><p>路径为 /mcp，传输为 Streamable HTTP；客户端令牌变量与设备服务一致。运行 stdio 前先停止同状态目录的 HTTP 服务。</p></details>
        <details><summary>A2A 调用示例</summary><p>向 A2A 地址 POST JSON，携带 Content-Type: application/json、A2A-Version: 1.0 和 Bearer 鉴权。每次发送更换 messageId；保存 task.id，使用 GetTask 查询异步结果。</p><CopyValue label="SendMessage 请求体" value={a2aMessage}/></details>
      </section>
      <section><h3>本地 MCP · stdio</h3><p>仅供与 nl2sh 位于同一设备的客户端启动；不需要 Bearer 令牌。stdout 专用于 MCP。每个状态目录只能运行一个协议进程，HTTP 与 stdio 不能同时使用该目录。</p><CopyValue label="本地进程命令" value={info.stdio_command}/><details><summary>本地 MCP 客户端配置</summary><CopyValue label="支持此格式的 MCP 客户端 · TOML" value={stdioMcpConfig(info)}/><p>如果 nl2sh 不在 PATH 中，将 command 改为该设备上的可执行文件绝对路径。</p></details></section>
      <section><h3>设备本地审批</h3><p>修改默认等待同 UID、同配置的设备交互终端批准。此页面不能批准协议任务；待决请求 120 秒过期，危险操作需要强确认。</p><CopyValue label="查看待决审批" value={info.approvals_command}/><p>在设备终端用 <code>protocol approve REQUEST_ID</code> 决定一次审批。</p></section>
    </>}
  </div></div>;
}
