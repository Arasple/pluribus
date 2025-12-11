# Pluribus

[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Pluribus 为 Claude Code 订阅用户提供统一的 Anthropic Messages API 接口。

### 主要特性

- **精确模拟** - 完整复刻 Claude Code 官方客户端的请求特征，包括 User-Agent、请求头和 Beta 特性标识
- **OAuth 认证** - 支持标准 OAuth 2.0 PKCE 流程，安全管理多个账号
- **自动刷新** - Token 过期前自动续期，无需手动干预
- **配额监控** - 实时跟踪 5 小时 / 7 天窗口的 Rate Limit 状态

> 注意：本项目专注模拟客户端行为，将订阅服务转 API 功能。不包含用量统计，API 密钥分发等功能

> 使用本项目的一切风险由用户自行承担。

## 快速开始

### 安装

```bash
git clone https://github.com/Arasple/pluribus.git
cd pluribus
cargo build --release
```

### 配置

创建 `.env` 文件：

```bash
cp .env.example .env
```

编辑必要参数：

```env
PLURIBUS_SECRET=your-random-secret-key
```

### 登录

```bash
# 添加第一个账号
pluribus login claude-code

# 可选：添加更多账号并命名
pluribus login claude-code --name work
pluribus login claude-code --name personal
```

### 启动服务

```bash
pluribus serve
```

服务默认监听 `http://0.0.0.0:8080`。

## 使用

### 发送请求

完全兼容 Anthropic Messages API：

```bash
curl http://localhost:8080/anthropic/v1/messages \
  -H "Authorization: Bearer your-secret-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-5",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### 健康检查

```bash
curl http://localhost:8080/health
```

返回服务状态和所有账号的配额信息。

### 测试

```bash
pluribus test
```

## API 路由

- `POST /anthropic/v1/messages` - Messages API 代理
- `GET /health` - 健康检查和配额状态

支持流式和非流式请求。当配置多个账号时，请求会按顺序轮询分发。

## 配置说明

### 环境变量

- `PLURIBUS_HOST` - 监听地址（默认：0.0.0.0）
- `PLURIBUS_PORT` - 监听端口（默认：8080）
- `PLURIBUS_SECRET` - API 访问密钥（必需）

### 账号配置

账号信息存储在 `./providers/*.toml`：

```toml
type = "claude_code"

[oauth]
access_token = "..."
refresh_token = "..."
expires_at = 1234567890000
scopes = ["user:inference", "user:sessions:claude_code"]
```

## 架构

```
Client
  ↓
Pluribus Gateway
  ├─ 认证中间件
  ├─ 请求路由
  └─ Token 管理
  ↓
Provider Pool (轮询)
  ├─ Account 1
  ├─ Account 2
  └─ Account N
  ↓
Anthropic API
```

核心模块：

- `gateway` - HTTP 服务器
- `providers` - 账号管理和 API 调用
- `commands` - CLI 实现
- `config` - 配置加载

## 免责声明

**本项目仅供学习和技术研究使用。**

使用本项目时，你需要：
- 拥有合法的 Claude Code 订阅
- 遵守 [Anthropic 服务条款](https://console.anthropic.com/legal/terms)
- 遵守 [Anthropic 使用政策](https://console.anthropic.com/legal/aup)

作者不对使用本项目产生的任何后果承担责任。请合理使用，不要滥用 API 或违反服务条款。

## 许可证

MIT License - 详见 [LICENSE](LICENSE)
