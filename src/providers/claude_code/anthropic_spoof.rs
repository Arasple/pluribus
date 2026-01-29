//! Anthropic Claude Code 伪装模块
//!
//! 在 system 提示词开头注入 Claude Code 官方身份标识，并处理工具名称映射

use serde_json::Value;

/// Claude Code 官方 system 提示词
const CLAUDE_CODE_SYSTEM_PROMPT: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

/// Claude Code 身份前缀（用于检测是否已包含官方身份）
const CLAUDE_CODE_IDENTITY_PREFIX: &str = "You are Claude Code";

/// 未知工具前缀
const UNKNOWN_TOOL_PREFIX: &str = "mcp_";

/// OpenCode → Claude Code 工具名称映射
const TOOL_MAPPINGS: &[(&str, &str)] = &[
    ("bash", "Bash"),
    ("edit", "Edit"),
    ("glob", "Glob"),
    ("grep", "Grep"),
    ("question", "AskUserQuestion"),
    ("read", "Read"),
    ("skill", "Skill"),
    ("task", "Task"),
    ("todoread", "TodoRead"),
    ("todowrite", "TodoWrite"),
    ("websearch", "WebSearch"),
    ("codesearch", "CodeSearch"),
    ("webfetch", "WebFetch"),
    ("write", "Write"),
];

/// 伪装 system 提示词
///
/// 在 system 数组开头注入官方身份标识
pub fn spoof_system(body: &mut Value) -> bool {
    let Some(obj) = body.as_object_mut() else {
        return false;
    };

    // 如果没有 "system" 字段，创建空数组
    if !obj.contains_key("system") {
        obj.insert("system".to_string(), Value::Array(vec![]));
    }

    let Some(system_arr) = obj.get_mut("system").and_then(|s| s.as_array_mut()) else {
        return false;
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

    let is_third_party_client = system_arr.iter().any(|item| {
        item.get("text")
            .and_then(|text| text.as_str())
            .is_some_and(|text| text.contains("opencode") || text.contains("OpenCode"))
    });

    is_third_party_client
}

/// 将第三方工具名映射为 Claude Code 工具名
fn map_tool_name(name: &str) -> String {
    TOOL_MAPPINGS
        .iter()
        .find(|(from, _)| *from == name)
        .map(|(_, to)| to.to_string())
        .unwrap_or_else(|| format!("{}{}", UNKNOWN_TOOL_PREFIX, name))
}

/// 将 Claude Code 工具名还原为第三方工具名
fn unmap_tool_name(name: &str) -> String {
    TOOL_MAPPINGS
        .iter()
        .find(|(_, to)| *to == name)
        .map(|(from, _)| from.to_string())
        .unwrap_or_else(|| {
            name.strip_prefix(UNKNOWN_TOOL_PREFIX)
                .unwrap_or(name)
                .to_string()
        })
}

/// 映射请求体中的工具名称（第三方 → Claude Code）
pub fn map_request_tools(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };

    // 映射 tools 定义
    if let Some(tools) = obj.get_mut("tools").and_then(|t| t.as_array_mut()) {
        for tool in tools {
            if let Some(name) = tool
                .get_mut("name")
                .and_then(|n| n.as_str())
                .map(map_tool_name)
            {
                tool["name"] = Value::String(name);
            }
        }
    }

    // 映射 messages 中的 tool_use blocks
    if let Some(messages) = obj.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages {
            let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) else {
                continue;
            };
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    if let Some(name) = block
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(map_tool_name)
                    {
                        block["name"] = Value::String(name);
                    }
                }
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    if let Some(name) = block
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(map_tool_name)
                    {
                        block["name"] = Value::String(name);
                    }
                }
            }
        }
    }
}

/// 还原响应体中的工具名称（Claude Code → 第三方）
pub fn unmap_response_tools(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };

    // 还原 content 中的 tool_use blocks
    if let Some(content) = obj.get_mut("content").and_then(|c| c.as_array_mut()) {
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                if let Some(name) = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(unmap_tool_name)
                {
                    block["name"] = Value::String(name);
                }
            }
        }
    }
}

/// 还原流式响应中的工具名称（content_block_start 事件）
pub fn unmap_stream_tool(event_data: &mut Value) {
    // content_block_start: { type, index, content_block: { type: "tool_use", name, id } }
    if let Some(block) = event_data.get_mut("content_block") {
        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
            if let Some(name) = block
                .get("name")
                .and_then(|n| n.as_str())
                .map(unmap_tool_name)
            {
                block["name"] = Value::String(name);
            }
        }
    }
}
