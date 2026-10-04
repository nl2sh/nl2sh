# 完整工具参数目录

本页从 Rust 注册表与派生 JSON Schema 生成，包含已关闭的可选工具和 ima 能力。名称/类型/参数枚举是协议原文；具体可用项用 `nl2sh bridge tools` 或 Web 工具页核对。关闭项不能直接调用，bridge 不包含长期监听器操作。启用不等于批准，风险和平台支持见 [工具指南](../tools/index.md)。

[下载机器可读 Schema](../../assets/tool-schemas.json)。Schema 中的英文描述来自模型协议，中文行为说明列在各工具下。

<!-- generated:start -->

## `agent_memory`

get/list 读便签，set/delete/clear 写入，写操作需确认。

```json
{
  "$defs": {
    "AgentMemoryAction": {
      "description": "Supported private Agent notebook operations exposed in the tool schema.",
      "oneOf": [
        {
          "const": "get",
          "description": "Read one value by key.",
          "type": "string"
        },
        {
          "const": "list",
          "description": "List all stored keys and values.",
          "type": "string"
        },
        {
          "const": "set",
          "description": "Create or replace one value after confirmation.",
          "type": "string"
        },
        {
          "const": "delete",
          "description": "Delete one key after confirmation.",
          "type": "string"
        },
        {
          "const": "clear",
          "description": "Delete all entries after confirmation.",
          "type": "string"
        }
      ]
    }
  },
  "additionalProperties": false,
  "properties": {
    "action": {
      "$ref": "#/$defs/AgentMemoryAction"
    },
    "key": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    },
    "value": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "action"
  ],
  "type": "object"
}
```

## `analyze_audio`

确定性 WAV/PCM DSP，缺少 PCM 元数据询问用户，不猜测。

```json
{
  "$defs": {
    "RawSampleFormat": {
      "enum": [
        "s16le",
        "s24le",
        "s32le",
        "f32le"
      ],
      "type": "string"
    }
  },
  "additionalProperties": false,
  "properties": {
    "channels": {
      "default": null,
      "format": "uint16",
      "maximum": 65535,
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "path": {
      "type": "string"
    },
    "sample_format": {
      "anyOf": [
        {
          "$ref": "#/$defs/RawSampleFormat"
        },
        {
          "type": "null"
        }
      ],
      "default": null
    },
    "sample_rate": {
      "default": null,
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

## `android.find_node`

按精确文字或 bounds 查找当前唯一节点。

```json
{
  "additionalProperties": false,
  "properties": {
    "bounds": {
      "default": null,
      "description": "Exact current node bounds for a node tap.",
      "type": [
        "string",
        "null"
      ]
    },
    "text": {
      "default": null,
      "description": "Visible text or content description to match.",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [],
  "type": "object"
}
```

## `android.input_text`

向焦点控件追加文字，Unicode 需 companion/键盘；replace 仅输入法。

```json
{
  "$defs": {
    "TextWriteMode": {
      "description": "How one Unicode write treats the text already in the focused control.\n\nAlso the model-visible `android.input_text` mode: clearing a field needs the companion input\nmethod, because the shell and accessibility paths can only insert characters.",
      "oneOf": [
        {
          "const": "append",
          "description": "Insert the text at the current cursor position.",
          "type": "string"
        },
        {
          "const": "replace",
          "description": "Clear the control first, then write the text.",
          "type": "string"
        }
      ]
    }
  },
  "additionalProperties": false,
  "properties": {
    "mode": {
      "anyOf": [
        {
          "$ref": "#/$defs/TextWriteMode"
        },
        {
          "type": "null"
        }
      ],
      "description": "Append to the focused control, or clear it first through the nl2sh keyboard."
    },
    "text": {
      "description": "Visible text or content description to match.",
      "type": "string"
    }
  },
  "required": [
    "text"
  ],
  "type": "object"
}
```

## `android.launch_app`

确认后启动验证过的包。

```json
{
  "additionalProperties": false,
  "properties": {
    "package": {
      "description": "Package name for launch or stop.",
      "type": "string"
    }
  },
  "required": [
    "package"
  ],
  "type": "object"
}
```

## `android.press_back`

确认后返回。

```json
{
  "additionalProperties": false,
  "properties": {},
  "required": [],
  "type": "object"
}
```

## `android.press_enter`

确认后按 Enter。

```json
{
  "additionalProperties": false,
  "properties": {},
  "required": [],
  "type": "object"
}
```

## `android.press_home`

确认后回到桌面。

```json
{
  "additionalProperties": false,
  "properties": {},
  "required": [],
  "type": "object"
}
```

## `android.read_screen`

返回有界屏幕图片，不保留文件。

```json
{
  "additionalProperties": false,
  "properties": {},
  "required": [],
  "type": "object"
}
```

## `android.screen_dump`

读取当前有界 UI 树。

```json
{
  "additionalProperties": false,
  "properties": {},
  "required": [],
  "type": "object"
}
```

## `android.screenshot`

有界屏幕图片；显式持久 PNG 路径需确认。

```json
{
  "additionalProperties": false,
  "properties": {
    "path": {
      "default": null,
      "description": "Absolute screenshot destination.",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [],
  "type": "object"
}
```

## `android.scroll`

确认后滚动，可自动计算坐标。

```json
{
  "$defs": {
    "ScrollDirection": {
      "description": "Direction the screen content should move.",
      "enum": [
        "down",
        "up"
      ],
      "type": "string"
    }
  },
  "additionalProperties": false,
  "properties": {
    "direction": {
      "anyOf": [
        {
          "$ref": "#/$defs/ScrollDirection"
        },
        {
          "type": "null"
        }
      ],
      "description": "Content scroll direction when coordinates are omitted."
    },
    "duration_ms": {
      "default": null,
      "description": "Swipe duration in milliseconds.",
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "end_x": {
      "default": null,
      "description": "Swipe end horizontal coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "end_y": {
      "default": null,
      "description": "Swipe end vertical coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "x": {
      "default": null,
      "description": "Start or tap horizontal coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "y": {
      "default": null,
      "description": "Start or tap vertical coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [],
  "type": "object"
}
```

## `android.stop_app`

确认后强制停止验证过的包。

```json
{
  "additionalProperties": false,
  "properties": {
    "package": {
      "description": "Package name for launch or stop.",
      "type": "string"
    }
  },
  "required": [
    "package"
  ],
  "type": "object"
}
```

## `android.swipe`

确认后在坐标间滑动。

```json
{
  "additionalProperties": false,
  "properties": {
    "duration_ms": {
      "default": null,
      "description": "Swipe duration in milliseconds.",
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "end_x": {
      "description": "Swipe end horizontal coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": "integer"
    },
    "end_y": {
      "description": "Swipe end vertical coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": "integer"
    },
    "x": {
      "description": "Start or tap horizontal coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": "integer"
    },
    "y": {
      "description": "Start or tap vertical coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": "integer"
    }
  },
  "required": [
    "x",
    "y",
    "end_x",
    "end_y"
  ],
  "type": "object"
}
```

## `android.tap`

确认后点击坐标。

```json
{
  "additionalProperties": false,
  "properties": {
    "x": {
      "description": "Start or tap horizontal coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": "integer"
    },
    "y": {
      "description": "Start or tap vertical coordinate.",
      "format": "uint32",
      "minimum": 0,
      "type": "integer"
    }
  },
  "required": [
    "x",
    "y"
  ],
  "type": "object"
}
```

## `android.tap_node`

确认后复核并点击精确 bounds 唯一节点。

```json
{
  "additionalProperties": false,
  "properties": {
    "bounds": {
      "description": "Exact current node bounds for a node tap.",
      "type": "string"
    }
  },
  "required": [
    "bounds"
  ],
  "type": "object"
}
```

## `android.tap_text`

确认后复核并点击精确文字唯一节点。

```json
{
  "additionalProperties": false,
  "properties": {
    "text": {
      "description": "Visible text or content description to match.",
      "type": "string"
    }
  },
  "required": [
    "text"
  ],
  "type": "object"
}
```

## `android.wait_text`

最多等待十秒精确可见文字。

```json
{
  "additionalProperties": false,
  "properties": {
    "text": {
      "description": "Visible text or content description to match.",
      "type": "string"
    },
    "timeout_ms": {
      "default": null,
      "description": "Maximum wait time in milliseconds.",
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "text"
  ],
  "type": "object"
}
```

## `android_clipboard`

读取剪贴板，或确认后写入有界文本。

```json
{
  "additionalProperties": false,
  "properties": {
    "text": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `android_connectivity`

验证公网主机并汇总有界连接性证据。

```json
{
  "additionalProperties": false,
  "properties": {
    "host": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `android_content_query`

有界只读 Content URI 查询，不写入。

```json
{
  "$defs": {
    "ProjectionArg": {
      "anyOf": [
        {
          "description": "Comma- or colon-delimited projection used by older callers.",
          "type": "string"
        },
        {
          "description": "Preferred structured list of projection columns.",
          "items": {
            "type": "string"
          },
          "type": "array"
        }
      ],
      "description": "Backward-compatible content-provider projection accepted as text or columns."
    }
  },
  "additionalProperties": false,
  "properties": {
    "projection": {
      "anyOf": [
        {
          "$ref": "#/$defs/ProjectionArg"
        },
        {
          "type": "null"
        }
      ]
    },
    "uri": {
      "type": "string"
    },
    "where_clause": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "uri"
  ],
  "type": "object"
}
```

## `android_crash_report`

有界 Crash/ANR 证据。

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "package": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `android_doze`

DeviceIdle 与白名单证据。

```json
{
  "additionalProperties": false,
  "type": "object"
}
```

## `android_dumpsys`

受限只读 dumpsys 服务查询。

```json
{
  "additionalProperties": false,
  "properties": {
    "arguments": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    },
    "service": {
      "type": "string"
    }
  },
  "required": [
    "service"
  ],
  "type": "object"
}
```

## `android_logcat`

有界 logcat 快照与受限过滤。

```json
{
  "additionalProperties": false,
  "properties": {
    "filter": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    },
    "lines": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `android_media_control`

读取媒体状态，或确认后改变播放/音量。

```json
{
  "additionalProperties": false,
  "properties": {
    "action": {
      "type": "string"
    },
    "level": {
      "default": null,
      "format": "uint8",
      "maximum": 255,
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "action"
  ],
  "type": "object"
}
```

## `android_media_query`

有界 MediaStore 图片、视频或音频元数据。

```json
{
  "additionalProperties": false,
  "properties": {
    "created_after_epoch_secs": {
      "default": null,
      "format": "uint64",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "limit": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "media_type": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `android_netstats`

有界网络流量账目。

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "package": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `android_notification`

有界结构化通知快照。

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "package": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `android_permission_audit`

审计指定包或有界应用集合的权限与 AppOps。

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "package": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `android_settings`

读取/列出 system/secure/global 设置，不写入。

```json
{
  "additionalProperties": false,
  "properties": {
    "key": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    },
    "namespace": {
      "type": "string"
    }
  },
  "required": [
    "namespace"
  ],
  "type": "object"
}
```

## `android_storage`

文件系统容量及有界应用存储证据。

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "package": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `android_thermal_power`

汇总电池、温控、电源和 DeviceIdle。

```json
{
  "additionalProperties": false,
  "type": "object"
}
```

## `android_wifi_eth`

汇总 Wi-Fi、以太网、IP、信号与路由。

```json
{
  "additionalProperties": false,
  "type": "object"
}
```

## `apply_patch`

唯一旧文本替换或新建文件，先展示 diff 并确认。

```json
{
  "additionalProperties": false,
  "properties": {
    "new_text": {
      "description": "Replacement text.",
      "type": "string"
    },
    "old_text": {
      "description": "Exact text that must occur once; empty creates a new empty/non-empty file.",
      "type": "string"
    },
    "path": {
      "description": "Absolute or process-base-relative target file.",
      "type": "string"
    }
  },
  "required": [
    "path",
    "old_text",
    "new_text"
  ],
  "type": "object"
}
```

## `capture_android_screen`

确认后保存屏幕 PNG。

```json
{
  "additionalProperties": false,
  "properties": {
    "path": {
      "type": "string"
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

## `create_chart`

校验并呈现已取得数值的柱状/折线/饼图，不采集或证明统计数据。

```json
{
  "$defs": {
    "ChartType": {
      "description": "Supported presentation types.",
      "oneOf": [
        {
          "const": "bar",
          "description": "Horizontal comparison bars.",
          "type": "string"
        },
        {
          "const": "line",
          "description": "Ordered values joined by a line.",
          "type": "string"
        },
        {
          "const": "pie",
          "description": "Parts of a nonzero whole.",
          "type": "string"
        }
      ]
    }
  },
  "additionalProperties": false,
  "properties": {
    "chart_type": {
      "$ref": "#/$defs/ChartType",
      "description": "Chart kind supported by both browser and text fallback."
    },
    "labels": {
      "description": "Category labels in display order.",
      "items": {
        "type": "string"
      },
      "type": "array"
    },
    "source": {
      "description": "Label describing where the numbers came from.",
      "type": "string"
    },
    "title": {
      "description": "Short heading for the chart.",
      "type": "string"
    },
    "unit": {
      "default": "",
      "description": "Optional unit shown next to values.",
      "type": "string"
    },
    "values": {
      "description": "One value for each label.",
      "items": {
        "format": "double",
        "type": "number"
      },
      "type": "array"
    }
  },
  "required": [
    "chart_type",
    "title",
    "source",
    "labels",
    "values"
  ],
  "type": "object"
}
```

## `decompile_apk_class`

通过验证过的 Android DEX helper/app_process 反编译精确单类，需强确认。

```json
{
  "additionalProperties": false,
  "properties": {
    "class_name": {
      "description": "Exact dotted class name, such as com.example.MainActivity.",
      "type": "string"
    },
    "path": {
      "description": "Path to an existing local APK file.",
      "type": "string"
    }
  },
  "required": [
    "path",
    "class_name"
  ],
  "type": "object"
}
```

## `download_url`

下载公网有界资源，确认后原子写入。

```json
{
  "additionalProperties": false,
  "properties": {
    "max_bytes": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "path": {
      "type": "string"
    },
    "url": {
      "type": "string"
    }
  },
  "required": [
    "url",
    "path"
  ],
  "type": "object"
}
```

## `execute_shell_command`

本地安全检查与必要确认后执行 shell 命令。

```json
{
  "additionalProperties": false,
  "properties": {
    "command": {
      "description": "Shell source to assess locally.",
      "type": "string"
    },
    "interactive": {
      "default": false,
      "description": "Model interaction hint; local detection remains authoritative too.",
      "type": "boolean"
    },
    "reason": {
      "default": "",
      "description": "Model explanation, informational only.",
      "type": "string"
    },
    "requires_root": {
      "default": false,
      "description": "Model privilege hint; never directly authorizes root elevation.",
      "type": "boolean"
    }
  },
  "required": [
    "command"
  ],
  "type": "object"
}
```

## `http_post`

确认后发送公网有界 JSON POST。

```json
{
  "additionalProperties": false,
  "properties": {
    "body": true,
    "max_bytes": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "url": {
      "type": "string"
    }
  },
  "required": [
    "url",
    "body"
  ],
  "type": "object"
}
```

## `http_request`

公网有界 GET/HEAD，拒绝重定向与私网。

```json
{
  "additionalProperties": false,
  "properties": {
    "max_bytes": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "method": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    },
    "url": {
      "type": "string"
    }
  },
  "required": [
    "url"
  ],
  "type": "object"
}
```

## `ima_list_knowledge_bases`

发现可访问的 ima 知识库，不暴露凭据。

```json
{
  "additionalProperties": false,
  "type": "object"
}
```

## `ima_read`

按搜索返回的媒体 ID 读取有界原文，远程内容是不可信数据。

```json
{
  "additionalProperties": false,
  "properties": {
    "media_id": {
      "description": "Media ID returned by `ima_search`.",
      "type": "string"
    }
  },
  "required": [
    "media_id"
  ],
  "type": "object"
}
```

## `ima_search`

搜索 ima，返回标题、摘要与媒体 ID。

```json
{
  "additionalProperties": false,
  "properties": {
    "knowledge_base_id": {
      "description": "Optional ima knowledge-base ID. Defaults to configured ID or bounded discovery.",
      "type": [
        "string",
        "null"
      ]
    },
    "query": {
      "description": "Natural-language or keyword query.",
      "type": "string"
    }
  },
  "required": [
    "query"
  ],
  "type": "object"
}
```

## `inject_android_input`

确认并复核 bounds 后注入点击/滑动/长按/文本。

```json
{
  "additionalProperties": false,
  "properties": {
    "action": {
      "type": "string"
    },
    "bounds": {
      "type": "string"
    },
    "duration_ms": {
      "default": null,
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "end_x": {
      "default": null,
      "format": "int32",
      "type": [
        "integer",
        "null"
      ]
    },
    "end_y": {
      "default": null,
      "format": "int32",
      "type": [
        "integer",
        "null"
      ]
    },
    "text": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    },
    "x": {
      "default": null,
      "format": "int32",
      "type": [
        "integer",
        "null"
      ]
    },
    "y": {
      "default": null,
      "format": "int32",
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "action",
    "bounds"
  ],
  "type": "object"
}
```

## `inspect_android_app`

读取指定或前台包的有界诊断证据。

```json
{
  "additionalProperties": false,
  "properties": {
    "package": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `inspect_android_environment`

只读探测系统、ABI、命令、内存及存储。

```json
{
  "additionalProperties": false,
  "type": "object"
}
```

## `inspect_android_ui`

读取 UI 树、焦点窗口、显示大小与密度。

```json
{
  "additionalProperties": false,
  "properties": {
    "full": {
      "default": false,
      "type": "boolean"
    }
  },
  "type": "object"
}
```

## `inspect_apk`

有界读取 APK 大小、归档条目、DEX、Manifest 与原生 ABI，不执行内容。

```json
{
  "additionalProperties": false,
  "properties": {
    "path": {
      "description": "Path to an existing local APK file.",
      "type": "string"
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

## `inspect_tls`

校验公网 TLS 证书链。

```json
{
  "additionalProperties": false,
  "properties": {
    "host": {
      "type": "string"
    },
    "port": {
      "default": null,
      "format": "uint16",
      "maximum": 65535,
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "host"
  ],
  "type": "object"
}
```

## `judge_audio_quality`

使用当前任务缓存的真实音频特征判断音质。

```json
{
  "additionalProperties": false,
  "properties": {
    "analysis_path": {
      "default": null,
      "description": "Exact path of a completed analysis cached by the current Agent task.",
      "type": [
        "string",
        "null"
      ]
    },
    "features": {
      "default": null
    },
    "purpose": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `list_android_apps`

有界列出应用包名、APK 路径及 UID。

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "scope": {
      "default": null,
      "type": [
        "string",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `list_apk_entries`

按可选字面路径前缀列出有界 ZIP 条目，不解压。

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "default": 50,
      "description": "Maximum returned entries, from 1 to 200.",
      "format": "uint",
      "minimum": 0,
      "type": "integer"
    },
    "path": {
      "description": "Path to an existing local APK file.",
      "type": "string"
    },
    "prefix": {
      "default": "",
      "description": "Optional literal ZIP entry prefix.",
      "type": "string"
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

## `list_dex_classes`

读取有界 DEX 类名索引，可按字面类名过滤。

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "default": 50,
      "description": "Maximum returned classes, from 1 to 200.",
      "format": "uint",
      "minimum": 0,
      "type": "integer"
    },
    "path": {
      "description": "Path to an existing local APK file.",
      "type": "string"
    },
    "query": {
      "default": "",
      "description": "Optional literal substring of the dotted class name.",
      "type": "string"
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

## `list_dir`

不依赖 shell 列出有界直接子项。

```json
{
  "additionalProperties": false,
  "properties": {
    "path": {
      "description": "Absolute or process-base-relative directory path, or `.`.",
      "type": "string"
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

## `read_file`

有界 UTF-8 文件读取，支持绝对/父目录/符号链接。

```json
{
  "additionalProperties": false,
  "properties": {
    "path": {
      "description": "Absolute or process-base-relative file path.",
      "type": "string"
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

## `search_text`

有界递归字面文本搜索，跟随符号链接并检测循环，无工作区限制。

```json
{
  "additionalProperties": false,
  "properties": {
    "path": {
      "default": ".",
      "description": "Absolute or process-base-relative file or directory path.",
      "type": "string"
    },
    "query": {
      "description": "Literal text to search for.",
      "type": "string"
    }
  },
  "required": [
    "query"
  ],
  "type": "object"
}
```

## `tailcat_check`

检查已配置程序及版本，不下载。

```json
{
  "additionalProperties": false,
  "type": "object"
}
```

## `tailcat_install`

校验摘要、ABI 与版本后原子安装固定官方版本，需确认。

```json
{
  "additionalProperties": false,
  "type": "object"
}
```

## `tailcat_receive`

在现有目录启动受管文件接收箱，需确认。

```json
{
  "additionalProperties": false,
  "properties": {
    "directory": {
      "type": "string"
    }
  },
  "required": [
    "directory"
  ],
  "type": "object"
}
```

## `tailcat_receive_stream`

启动一次原始流接收并保存新文件，需确认。

```json
{
  "additionalProperties": false,
  "properties": {
    "path": {
      "type": "string"
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

## `tailcat_send_file`

发送现有文件；stream 用于原始接收，copy 需 scp，强确认。

```json
{
  "$defs": {
    "SendMode": {
      "enum": [
        "stream",
        "copy"
      ],
      "type": "string"
    }
  },
  "additionalProperties": false,
  "properties": {
    "address": {
      "type": "string"
    },
    "mode": {
      "$ref": "#/$defs/SendMode"
    },
    "path": {
      "type": "string"
    }
  },
  "required": [
    "path",
    "address",
    "mode"
  ],
  "type": "object"
}
```

## `tailcat_serve`

把 Tailcat 连接转发到已有 localhost TCP 服务；参数是目标端口，不重复绑定，无需停止服务；缺少程序时先确认安装，共享需强确认。

```json
{
  "additionalProperties": false,
  "properties": {
    "port": {
      "description": "Destination port of an existing localhost TCP service; keep that service running.",
      "format": "uint16",
      "maximum": 65535,
      "minimum": 0,
      "type": "integer"
    }
  },
  "required": [
    "port"
  ],
  "type": "object"
}
```

## `tailcat_status`

读取当前进程的受管监听器状态。

```json
{
  "additionalProperties": false,
  "type": "object"
}
```

## `tailcat_stop`

确认后停止当前进程监听器。

```json
{
  "additionalProperties": false,
  "type": "object"
}
```

## `top_android_apps`

按驻留内存排序有界进程快照。

```json
{
  "additionalProperties": false,
  "properties": {
    "limit": {
      "default": null,
      "format": "uint",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "type": "object"
}
```

## `view_screenshot`

为下次模型请求附加 PNG/JPEG/WebP，有界缩放。

```json
{
  "additionalProperties": false,
  "properties": {
    "path": {
      "type": "string"
    }
  },
  "required": [
    "path"
  ],
  "type": "object"
}
```

<!-- generated:end -->
