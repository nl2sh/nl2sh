export const examples=[
  {title:'查看手机存储',description:'看看还剩多少空间',prompt:'查看这台 Android 设备的存储空间，说明总量、已用和剩余空间。只查看，不修改。使用图表展示。'},
  {title:'查看系统信息',description:'了解系统版本和内存',prompt:'查看这台 Android 设备的系统版本、CPU 架构和内存情况。只查看，不修改。'},
  {title:'查看应用',description:'列出已安装应用',prompt:'列出这台 Android 设备已安装的应用及版本信息。只查看，不修改。'},
];

export function errorGuidance(value:string):string{
  const error=value.toLowerCase();
  if(/401|unauthorized|invalid.*key|authentication/.test(error))return '模型服务未接受密钥。打开“快速开始”，检查服务商与 API Key。';
  if(/429|rate.limit|quota|insufficient.balance/.test(error))return '服务暂时限制请求或额度不足。稍后重试，并到服务商查看额度。';
  if(/timeout|timed out|超时/.test(error))return '等待时间已到。检查设备网络和代理设置后重试。';
  if(/network|connect|dns|resolve|连接/.test(error))return '无法连接服务。检查设备网络、服务地址和代理设置。';
  if(/not found|no such file|command not found|不存在/.test(error))return '目标文件或设备命令未找到。检查名称与路径，也可以换一种说法重试。';
  return '操作未完成。查看错误详情，再尝试修改问题或设置。';
}
