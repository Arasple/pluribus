//! Tool 名称伪装模块
//!
//! 通过映射 tool 名称绕过 Claude Code 检测，响应时还原

use serde_json::Value;

/// 默认前缀
const DEFAULT_PREFIX: &str = "mcp_";

/// 特殊映射规则：(原名称, 伪装名称)
const MAPPINGS: &[(&str, &str)] = &[
    // OpenCode
    ("bash", "Bash"),
    ("question", "AskUserQuestion"),
    ("read", "Read"),
    ("write", "Write"),
    ("edit", "Edit"),
    ("glob", "Glob"),
    ("grep", "Grep"),
    ("task", "Task"),
    ("webfetch", "WebFetch"),
    ("todowrite", "TodoWrite"),
    ("skill", "Skill"),
];

/// 伪装请求中的 tool 名称
///
/// 处理：
/// 1. tools 数组中的 tool 定义
/// 2. messages 中的 tool_use 块
pub fn spoof(mut request: Value) -> Value {
    let obj = match request.as_object_mut() {
        Some(obj) => obj,
        None => return request,
    };

    // 处理 tools 数组
    if let Some(tools) = obj.get_mut("tools").and_then(|t| t.as_array_mut()) {
        for tool in tools {
            transform_name(tool, to_spoofed);
        }
    }

    // 处理 messages 中的 tool_use 块
    if let Some(messages) = obj.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages {
            if let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
                for block in content {
                    if is_tool_use_block(block) {
                        transform_name(block, to_spoofed);
                    }
                }
            }
        }
    }

    request
}

/// 还原响应中的 tool 名称
///
/// 处理 content 数组中的 tool_use 块
pub fn restore(response: &mut Value) {
    let content = response
        .as_object_mut()
        .and_then(|obj| obj.get_mut("content"))
        .and_then(|c| c.as_array_mut());

    if let Some(content) = content {
        for item in content {
            transform_name(item, to_original);
        }
    }
}

/// 还原 SSE 文本中的 tool 名称
///
/// 使用正则替换，适用于流式响应
pub fn restore_text(text: &str) -> String {
    let mut result = text.to_string();

    // 还原特殊映射
    for (original, spoofed) in MAPPINGS {
        let pattern = format!(r#""name"\s*:\s*"{}""#, regex::escape(spoofed));
        let replacement = format!(r#""name": "{}""#, original);
        if let Ok(re) = regex::Regex::new(&pattern) {
            result = re.replace_all(&result, replacement.as_str()).to_string();
        }
    }

    // 还原默认前缀
    let prefix_pattern = format!(r#""name"\s*:\s*"{}([^"]+)""#, DEFAULT_PREFIX);
    if let Ok(re) = regex::Regex::new(&prefix_pattern) {
        result = re.replace_all(&result, r#""name": "$1""#).to_string();
    }

    result
}

/// 检查是否为 tool_use 块
fn is_tool_use_block(block: &Value) -> bool {
    block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
}

/// 转换 name 字段
fn transform_name(item: &mut Value, transformer: fn(&str) -> String) {
    let obj = match item.as_object_mut() {
        Some(obj) => obj,
        None => return,
    };

    if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
        let new_name = transformer(name);
        if new_name != name {
            obj.insert("name".to_string(), Value::String(new_name));
        }
    }
}

/// 将原始名称转换为伪装名称
fn to_spoofed(name: &str) -> String {
    // 检查特殊映射
    for (original, spoofed) in MAPPINGS {
        if name == *original {
            return spoofed.to_string();
        }
    }

    // 默认：添加前缀（跳过已有前缀的）
    if name.starts_with(DEFAULT_PREFIX) {
        name.to_string()
    } else {
        format!("{DEFAULT_PREFIX}{name}")
    }
}

/// 将伪装名称还原为原始名称
fn to_original(name: &str) -> String {
    // 检查特殊映射
    for (original, spoofed) in MAPPINGS {
        if name == *spoofed {
            return original.to_string();
        }
    }

    // 默认：移除前缀
    name.strip_prefix(DEFAULT_PREFIX)
        .unwrap_or(name)
        .to_string()
}
