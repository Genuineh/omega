---
audience: developers
content_revision: 96
created: 2026-03-18
generation_id: gen_000013_r000096
last_verified_commit: N/A
level: intermediate
owner: omega-team
projection_version: 13
source_doc_id: "guide:docs-guide-omega-dev-guide"
status: active
updated: 2026-04-13
---

# Omega 开发指南

## Overview

本指南用于快速进入当前可运行的 Omega 工作区。它聚焦当前真实主路径、常用命令、配置入口和文档导航，不重复展开已经迁入规格文档的设计细节。

阅读完本指南后，你应能完成四件事：
- 找到当前应阅读的文档入口
- 构建、测试并运行当前应用
- 理解主路径上的 crate 分层
- 知道配置、日志和排障入口在哪里

`docs/TODO.md` 现在只保留 open work 和 current baseline；如果你在找已完成里程碑的完整背景，请改看对应 spec 或其 changelog。

## Prerequisites

- Rust toolchain 已安装并可运行 `cargo`
- 了解 Rust 基础语法
- 熟悉命令行操作
- 若需连真实 provider，准备好对应 API key

## Getting Started

### Step 1: 构建工作区

```bash
cargo build
```

### Step 2: 运行测试

```bash
cargo test
```

### Step 3: 启动当前应用入口

```bash
cargo run -p omega-app
```

如果你要启用文档/检索后端：

```bash
cargo run -p omega-app --features document-backend
```

### Step 4: 进入文档入口

先读以下文档，再进入具体模块：
- `docs/TODO.md`
- `docs/README.md`
- `docs/specs/omega-agent-spec.md`
- `docs/specs/omega-agent-impl-plan.md`

## 当前工作区结构

当前 workspace 有 28 个 crate。日常开发不需要记住所有 crate 的细枝末节，先按主路径和支撑层理解即可。

### 主运行路径

| Crate | 当前职责 |
|-------|----------|
| `omega-app` | 唯一应用入口，负责 bootstrap、config、provider wiring 和 runtime policy |
| `omega-project` | project 检测、active project 选择、project-bound context ownership 与 session catalog |
| `omega-session` | 会话运行态编排、workflow turn 驱动、runtime-visible 更新归一 |
| `omega-core` | agent loop、工具分发和底层执行模型 |
| `omega-workflow` | workflow 定义、step 配置与运行策略 |
| `omega-tui` | 纯 UI shell、输入/渲染/事件循环 |
| `omega-observability` | tracing bootstrap、日志 sink 和文件日志策略 |
| `omega-client` | provider transport、Anthropic-compatible 抽象与 streaming |

### 上下文与内容层

| Crate | 当前职责 |
|-------|----------|
| `omega-context` | 对外统一上下文 facade，组装 prompt-facing context、document/memory recall 与 session-ledger history hits |
| `omega-project` | project root、session catalog、`/project` command ownership，以及 `.omega/` / `.omega-state/` layout ownership |
| `omega-memory` | repo-wide 长期记忆 archive、summary ranking 与 compaction |
| `omega-document` | 文件治理、索引、检索与文档工具后端 |
| `omega-todo` | todo 工具与快照模型 |
| `omega-tasks` | 持久化任务层预留 |

### 工具与扩展层

| Crate | 当前职责 |
|-------|----------|
| `omega-tools` | 工具 contract 与 dispatch 边界 |
| `omega-tools-builtin` | 内置 repo inspection / edit / batch / bash 工具 |
| `omega-skills` | skill 目录加载与 prompt 注入 |
| `omega-subagent` | subagent 基础设施 |
| `omega-background` | 后台任务能力预留 |
| `omega-message` | runtime-visible message 能力预留 |
| `omega-team` | team / delegation 能力预留 |
| `omega-worktree` | 隔离执行与 worktree 能力预留 |

### 支撑与 UI 配套层

| Crate | 当前职责 |
|-------|----------|
| `omega-theme` | TUI 主题与视觉令牌 |
| `omega-keymap` | keymap 与 `.omega` 键位配置 |
| `omega-hooks` | step lifecycle hook 相关能力 |
| `omega-compression` | session ledger load/search、预算裁剪与后续历史压缩入口 |
| `omega-test-support` | 脚本化测试支撑 |

## 常见任务

### 运行当前入口

```bash
cargo run -p omega-app
```

### 运行文档/检索后端入口

```bash
cargo run -p omega-app --features document-backend
```

### 运行定向测试

```bash
cargo test -p omega-client
cargo test -p omega-project
cargo test -p omega-session
cargo test -p omega-tui
cargo test-document-backend
cargo test-document-commands
```

`cargo test -p omega-app` 现在默认不再拉起 `document-backend`，用于日常快速回归；`cargo test-document-backend` 只覆盖文档存储/keyword+semantic+hybrid retrieval 后端，`cargo test-document-commands` 单独覆盖 `omega-session` 的 `/document` command 集成。feature-enabled session 文档测试默认强制 mock embedding backend，避免真实模型下载与 `.fastembed_cache` 污染工作树。

当前 project system 基线已经完整落地：repo-local Omega 布局现已拆成 `.omega/` config/source root 与 `.omega-state/` runtime state root。`/document` 默认绑定 active project root，而 `/project list|switch|info|sessions|knowledge|delete` 提供显式运维入口；`/project switch` 不只更新 cwd/dispatcher，也会同步重绑 repo-scoped skills、hooks 和 tool surface。TUI 除底部 project badge 外，Sidebar 现也有正式 `Project` panel，并可通过键盘或鼠标打开统一 project detail overlay。

### 管理 session 与恢复上下文

当前 session artifacts 全部保存在仓库内的 `.omega-state/sessions/`，并已切到 per-session 目录 + canonical ledger：

| 文件 | 用途 |
|------|------|
| `<session-id>/session.json` | session catalog entry，记录 title、status、turn 计数和 resume-ready 元数据 |
| `<session-id>/session.context.jsonl` | canonical context ledger，统一承载 working-set snapshot 与 replay/history records |

TUI 中的默认入口已经切到 overlay-first：

```text
/session
/session list
/session resume
```

这些入口会直接打开 session picker，而不是把列表继续打印到 Response。当前快捷键约定如下：

- `Enter`: 打开所选 session 详情
- `Ctrl-R`: resume 所选 session
- `Ctrl-A`: archive 所选 session
- `Ctrl-D`: 先弹 confirm overlay，再删除所选 session artifacts
- `Ctrl-N`: 新建 session

如果需要精确或脚本化调用，仍保留 direct command 形态：

```text
/session info <session-id>
/session resume <session-id>
/session archive <session-id>
/session delete <session-id>
```

应用启动现在默认停在 runtime-only `Unbound` 状态：不会自动创建 session，也不会自动恢复上次 active session。首条真实用户消息会 lazy-create 并绑定一个新 session；显式 `/session new` 也会创建并绑定新 session；显式 `/session resume` 才会绑定并恢复旧 session。`project record.active_session_id` 现在只作为最近选择提示和 picker 排序线索，不再驱动启动自动恢复。

session restore 不会重放旧 workflow step 或 tool run。当前恢复链路已经改为通过 `omega-compression` 读取 `session.context.jsonl` 的 projection：默认按 `400k` token budget 做近期优先装载，并为 TUI hydration 投影可见 replay records；老的 `<session-id>.snapshot.json` / `<session-id>.log.jsonl` 若存在，会在首次读取时自动迁移到 canonical ledger。实际 prompt assembly 现在也会消费两类 ledger-derived surface：一类是 checkpoint / truncation 的 `<session_ledger_context>` note，另一类是基于 canonical ledger historical search 命中的 `<session_history_hits>`。与之对应，`omega-memory` 的 archive query 已退回 repo-wide long-term recall aid，不再承担 session-local history 的主链路语义。恢复后的 notice 与 direct `/session resume <id>` 结果会显式展示 `recent records / compression summaries / search hits` 的装配统计；`/session info <session-id>` 也会显示 canonical ledger 是否存在、record/replay/snapshot/checkpoint 计数与最新 checkpoint 摘要。后续 recall/rerank 质量与更深层 checkpoint retrieval 继续作为独立 follow-up 推进，不再属于 Task 17 baseline。

### 格式化与静态检查

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

### 查看当前优先级与文档入口

```bash
sed -n '1,120p' docs/TODO.md
sed -n '1,160p' docs/README.md
```

## 配置入口

| 文件 | 用途 |
|------|------|
| `.omega/model.toml` | provider、节流与模型相关配置 |
| `.omega/env.toml` | 仓库级环境变量注入 |
| `.omega/workflows/root.toml` | root workflow 路由配置 |
| `.omega/workflows/*.toml` | child workflow 定义 |
| `.omega/theme.toml` | TUI 主题覆盖 |

运行时生成状态默认位于 `.omega-state/`，其中常见目录包括：

| 路径 | 用途 |
|------|------|
| `.omega-state/project.json` | generated project metadata snapshot |
| `.omega-state/sessions/` | session catalog entries 与 canonical ledgers |
| `.omega-state/memory/` | archived turns 与 observations |
| `.omega-state/store/` | document manifest、tantivy、LanceDB、store version/history |
| `.omega-state/hooks/` | compiled hook artifacts |

## 当前架构约束

- `omega-app` 拥有应用 bootstrap 与 runtime message policy。
- `omega-project` 是 project root、session catalog 和 project-bound context ownership 的唯一上层入口。
- `omega-session` 是 TUI shell 与 agent runtime 之间的编排边界。
- `omega-tui` 只负责 UI，不承载会话编排或 observability bootstrap。
- 上下文主路径通过 `omega-context` 暴露；session/app/tool factory 不应直接绕过 `omega-project` 去隐式持有 repo-scoped `omega-context` 实例。
- 当前 runtime 主链路以 `docs/specs/omega-runtime-message-pipeline.md` 为准；`RuntimeUiEnvelope` 只保留 compat baseline。

## 日志与排障

Omega 使用 `tracing` 输出两类日志：
- TUI 内的 `Activity & Logs` 面板
- `~/.omega/logs/omega-YYYY-MM-DD.jsonl` 下的 JSONL 文件

常用环境变量：

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `OMEGA_LOG` | 日志级别 (`trace/debug/info/warn/error`) | `info` |
| `OMEGA_LOG_DIR` | JSONL 日志目录 | `~/.omega/logs` |
| `OMEGA_LOG_FILE` | 是否启用文件日志 | `true` |

常见排障动作：
- provider 交互异常时先用 `OMEGA_LOG=debug cargo run -p omega-app`
- 如果要排查 `/document`、`search_codebase` 或 hybrid retrieval，请改用 `OMEGA_LOG=debug cargo run -p omega-app --features document-backend`
- 如果怀疑节流或 429，检查 `.omega/model.toml` 的 `[provider]` 覆盖项
- 如果 TUI 视图异常，先确认当前文档基线是否与 `docs/TODO.md` 和相关 spec 一致
- `.omega-state/` 默认是可清理、不可提交的 runtime state；`.omega/` 则是 repo-local config/source root，可按仓库策略纳入版本控制

## Troubleshooting

### 编译错误

**Problem**: 编译失败
**Solution**:
```bash
cargo build
```

### 运行时错误

**Problem**: API Key 未设置
**Solution**:
```bash
export OMEGA_API_KEY="your-api-key"
# 或者
export OMEGA_MINIMAX_API_KEY="your-api-key"
```

### 文档入口看起来互相冲突

**Problem**: 同一主题出现多份文档，不知道该信哪一份
**Solution**: 先看 `docs/README.md` 的分组导航；`docs/TODO.md` 只负责 open work；如果文档位于 `docs/archive/`，默认视为历史材料，不作为 active source of truth

## Best Practices

- 保持 crate 边界清晰，优先修正文档入口和 contract 再扩功能
- 修改实现时同步更新 `docs/TODO.md` 与相关 spec
- 默认把 `docs/specs/` 当作 source of truth，把 `docs/archive/` 当作历史背景
- 新行为要补测试，至少覆盖当前改动所经过的主路径

## Related Topics

- 技术规格文档: docs/specs/omega-agent-spec.md
- ADR 文档: docs/decisions/
- TODO 跟踪: docs/TODO.md
