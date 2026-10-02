import type {ToolInfo} from './types';

const names:Record<string,string>={
  execute_shell_command:'运行设备命令',read_file:'读取文件',list_dir:'浏览目录',search_text:'搜索文件内容',apply_patch:'修改文件',create_chart:'制作图表',
  analyze_audio:'分析音频',judge_audio_quality:'判断音质',inspect_android_app:'检查应用',inspect_android_environment:'查看设备环境',list_android_apps:'列出应用',top_android_apps:'查看资源占用应用',android_dumpsys:'读取系统服务信息',android_logcat:'查看系统日志',android_settings:'查看系统设置',android_content_query:'查询设备数据',
  http_request:'读取网页',download_url:'下载文件',inspect_android_ui:'查看当前界面',capture_android_screen:'保存屏幕截图',view_screenshot:'查看截图内容',inject_android_input:'操作屏幕',http_post:'提交 JSON 请求',android_notification:'查看通知',android_crash_report:'查看崩溃记录',android_thermal_power:'查看电池与温度',android_netstats:'查看网络流量',android_storage:'查看存储',android_wifi_eth:'查看网络状态',android_doze:'查看休眠状态',android_permission_audit:'检查应用权限',android_clipboard:'读取或设置剪贴板',android_media_control:'查看或控制媒体',android_media_query:'查找媒体文件',agent_memory:'查看或更新便签',android_connectivity:'检查网络连通性',inspect_tls:'检查网站证书',
  inspect_apk:'检查 APK',list_apk_entries:'列出 APK 内容',list_dex_classes:'列出 DEX 类',decompile_apk_class:'反编译 APK 类',tailcat_check:'检查 Tailcat',tailcat_install:'安装 Tailcat',tailcat_receive:'接收文件',tailcat_receive_stream:'接收原始数据',tailcat_send_file:'发送文件',tailcat_serve:'共享本地端口',tailcat_status:'查看 Tailcat 状态',tailcat_stop:'停止 Tailcat',
  ima_list_knowledge_bases:'列出知识库',ima_search:'搜索知识库',ima_read:'读取知识库内容',
};
const purposes:Record<string,string>={
  execute_shell_command:'在安全检查和必要确认后运行设备命令。',read_file:'读取有大小限制的文本文件。',list_dir:'列出目录中的文件和子目录。',search_text:'在文件中搜索指定文字。',apply_patch:'预览并确认后修改或创建文件。',create_chart:'把已有数值显示为图表，不采集数据。',
  analyze_audio:'分析 WAV 或原始 PCM 音频特征。',judge_audio_quality:'依据已完成的分析判断音质。',inspect_android_app:'查看指定或当前应用的信息。',inspect_android_environment:'查看 Android 版本、架构、内存和存储。',list_android_apps:'列出已安装应用。',top_android_apps:'查看应用的内存占用快照。',android_dumpsys:'读取指定系统服务的诊断信息。',android_logcat:'读取有界的系统日志片段。',android_settings:'读取系统设置。',android_content_query:'读取指定内容提供者的数据。',
  http_request:'读取公网网页或接口。',download_url:'确认后下载公网文件。',inspect_android_ui:'查看当前界面的可操作控件。',capture_android_screen:'确认后保存屏幕截图。',view_screenshot:'让模型查看已有截图。',inject_android_input:'确认后点击、滑动或输入文字。',http_post:'确认后向公网接口发送 JSON。',android_notification:'查看当前通知。',android_crash_report:'查看崩溃与无响应记录。',android_thermal_power:'查看电池、温度与电源状态。',android_netstats:'查看网络流量统计。',android_storage:'查看设备和应用存储占用。',android_wifi_eth:'查看 Wi-Fi、有线网络与路由状态。',android_doze:'查看设备休眠状态。',android_permission_audit:'检查应用权限和 AppOps。',android_clipboard:'读取或确认后修改剪贴板。',android_media_control:'查看或确认后控制媒体播放。',android_media_query:'查找图片、视频或音频元数据。',agent_memory:'读取或确认后更新私有便签。',android_connectivity:'检查指定公网主机的连通性。',inspect_tls:'检查公网主机的 TLS 证书。',
  inspect_apk:'查看 APK 结构。',list_apk_entries:'查看 APK 内的文件。',list_dex_classes:'列出 DEX 类名。',decompile_apk_class:'强确认后反编译一个类。',tailcat_check:'检查可执行文件和版本。',tailcat_install:'确认后按设备架构下载并校验官方版本，再安装到配置路径。',tailcat_receive:'确认后启动文件接收箱，显示地址。',tailcat_receive_stream:'确认后把一段原始数据保存为新文件。',tailcat_send_file:'强确认后以原始流或文件复制模式发送。',tailcat_serve:'强确认后共享本地 TCP 端口。',tailcat_status:'查看当前接收或服务状态。',tailcat_stop:'确认后停止接收或服务。',
  ima_list_knowledge_bases:'列出可用知识库。',ima_search:'搜索知识库内容。',ima_read:'读取知识库原文。',
};
const categories:Record<string,string>={Shell:'命令',File:'文件',Audio:'音频',Android:'设备',Network:'网络',Memory:'便签',Chart:'图表',Knowledge:'知识库'};
const prompts:Record<string,string>={
  inspect_android_environment:'查看这台设备的 Android 版本、架构、内存和存储，只查看。',
  android_storage:'查看这台设备的存储总量、已用和剩余空间，只查看。',
  android_crash_report:'查看最近的应用崩溃和无响应记录，只查看。',
  android_wifi_eth:'查看当前 Wi-Fi、有线网络和 IP 状态，只查看。',
  android_notification:'概述当前设备通知，只查看。',
  android_thermal_power:'查看电池电量、温度和省电状态，只查看。',
  inspect_android_ui:'查看当前界面有哪些可操作控件，只查看。',
  inspect_tls:'检查 example.com 的 TLS 证书链和有效期，只查看。',
  agent_memory:'列出当前保存的便签，只查看。',
  create_chart:'把刚才得到的数值绘制成图表，并标明数据来源。',
  ima_search:'在已配置的知识库中搜索相关资料，只查看。',
  tailcat_check:'检查 tailcat 是否可用，查看版本。',tailcat_install:'安装对应设备架构的官方 tailcat，先让我确认下载来源和目标路径。',tailcat_receive:'在 /sdcard/Download 启动 tailcat 文件接收箱，并显示地址。',tailcat_status:'查看 tailcat 当前接收或共享状态。',
};

export function toolName(tool:ToolInfo):string{return names[tool.name]||tool.name.replaceAll('_',' ')}
export function toolPurpose(tool:ToolInfo):string{return purposes[tool.name]||tool.description}
export function toolCategory(tool:ToolInfo):string{return categories[tool.category]||tool.category}
export function toolRisk(tool:ToolInfo):string{
  if(tool.risk==='DynamicShell')return '按实际命令评估';
  if(tool.risk==='ReadOnly')return '只读或按具体操作确认';
  if(tool.risk==='Mutating')return '需要确认';
  return '需要强确认';
}
export function toolPrompt(tool:ToolInfo):string{return prompts[tool.name]||`请帮我${toolName(tool)}；先说明需要我提供哪些具体信息。`}
