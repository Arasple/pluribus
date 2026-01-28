//! Anthropic Claude Code 伪装模块
//!
//! 绕过 Anthropic Claude Code 相关检测：
//! - 伪装 system 提示词：替换 OpenCode -> Claude Code，注入官方身份标识
//! - 伪装 tool 名称：映射 tool 名称，响应时还原

use serde_json::Value;

/// Claude Code 官方 system 提示词
const CLAUDE_CODE_SYSTEM_PROMPT: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

/// Claude Code 身份前缀（用于检测是否已包含官方身份）
const CLAUDE_CODE_IDENTITY_PREFIX: &str = "You are Claude Code";

/// 伪装 system 提示词
///
/// 在 system 数组开头注入官方身份标识
pub fn spoof_system(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };

    // 如果没有 "system" 字段，创建空数组
    if !obj.contains_key("system") {
        obj.insert("system".to_string(), Value::Array(vec![]));
    }

    let Some(system_arr) = obj.get_mut("system").and_then(|s| s.as_array_mut()) else {
        return;
    };

    // 确保首个 block 为官方身份标识
    let has_identity = system_arr
        .first()
        .and_then(|item| item.get("text"))
        .and_then(|text| text.as_str())
        .is_some_and(|text| text.starts_with(CLAUDE_CODE_IDENTITY_PREFIX));

    if has_identity {
        // 已有身份 block，规范化文本
        if let Some(text) = system_arr[0].get_mut("text") {
            *text = Value::String(CLAUDE_CODE_SYSTEM_PROMPT.to_string());
        }
    } else {
        // 缺少身份 block，注入
        system_arr.insert(
            0,
            serde_json::json!({
                "text": CLAUDE_CODE_SYSTEM_PROMPT,
                "type": "text"
            }),
        );
    }
}

/// 默认前缀
const DEFAULT_PREFIX: &str = "cc_";

/// Claude Code 官方工具名称
const CC_TOOLS: &[&str] = &[
    "AskUserQuestion",
    "Bash",
    "Edit",
    "EnterPlanMode",
    "ExitPlanMode",
    "Glob",
    "Grep",
    "NotebookEdit",
    "Read",
    "Skill",
    "Task",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskOutput",
    "TaskStop",
    "TaskUpdate",
    "WebFetch",
    "WebSearch",
    "Write",
];

/// 第三方客户端 → Claude Code 名称映射
const MAPPINGS: &[(&str, &str)] = &[
    ("bash", "Bash"),
    ("edit", "Edit"),
    ("glob", "Glob"),
    ("grep", "Grep"),
    ("question", "AskUserQuestion"),
    ("read", "Read"),
    ("skill", "Skill"),
    ("task", "Task"),
    ("todowrite", "TodoWrite"),
    ("webfetch", "WebFetch"),
    ("write", "Write"),
];

/// 伪装请求中的 tool 名称
///
/// 处理：
/// 1. tools 数组中的 tool 定义
/// 2. messages 中的 tool_use 块
///
/// 返回是否发生名称变换
pub fn spoof_tools(mut request: Value) -> (Value, bool) {
    let obj = match request.as_object_mut() {
        Some(obj) => obj,
        None => return (request, false),
    };
    let mut changed = false;

    // 处理 tools 数组
    if let Some(tools) = obj.get_mut("tools").and_then(|t| t.as_array_mut()) {
        for tool in tools {
            if transform_name(tool, to_spoofed) {
                changed = true;
            }
        }
    }

    // 处理 messages 中的 tool_use 块
    if let Some(messages) = obj.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages {
            if let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
                for block in content {
                    if is_tool_use_block(block) && transform_name(block, to_spoofed) {
                        changed = true;
                    }
                }
            }
        }
    }

    (request, changed)
}

/// 还原响应中的 tool 名称
///
/// 处理 content 数组中的 tool_use 块
pub fn restore_tools(response: &mut Value) {
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
pub fn restore_tools_text(text: &str) -> String {
    let mut result = text.to_string();

    // 还原显式映射
    for (original, spoofed) in MAPPINGS {
        let pattern = format!(r#""name"\s*:\s*"{}""#, regex::escape(spoofed));
        let replacement = format!(r#""name": "{}""#, original);
        if let Ok(re) = regex::Regex::new(&pattern) {
            result = re.replace_all(&result, replacement.as_str()).to_string();
        }
    }

    // 还原 cc_ 前缀（跳过已由显式映射处理的）
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
fn transform_name(item: &mut Value, transformer: fn(&str) -> String) -> bool {
    let obj = match item.as_object_mut() {
        Some(obj) => obj,
        None => return false,
    };

    if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
        let new_name = transformer(name);
        if new_name != name {
            obj.insert("name".to_string(), Value::String(new_name));
            return true;
        }
    }
    false
}

/// 将原始名称转换为伪装名称
fn to_spoofed(name: &str) -> String {
    // 显式映射
    for (original, spoofed) in MAPPINGS {
        if name == *original {
            return spoofed.to_string();
        }
    }

    // 已是 CC 工具名称
    if CC_TOOLS.contains(&name) {
        return name.to_string();
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
    // 显式映射
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
