---
status: draft
owner: omega-team
created: 2026-03-23
updated: 2026-03-23
version: 0.1
supersedes: []
related_prds: []
---

# Omega Tool System Upgrade Specification

## Overview

Omega 当前的 tool 主路径已经具备可用的 trait-based abstraction、step 级可见性控制和 runtime tool lifecycle，但常见仓库分析任务仍然会把大量只读检索工作压到 `bash`。这导致两个直接问题：第一，模型需要自己拼装 shell 语法，容易因为 redirection、escaping、`find -exec` 一类形式细节触发安全拦截；第二，tool 返回值仍以自由文本为主，session 和 TUI 虽然已经有 typed tool-run contract，却缺少来自 tool 本身的稳定结构化结果。

参考现有成熟实现，下一阶段不应继续单纯放宽 `bash`，而应把常见的查找、读取、编辑和编排行为收敛为更细粒度、更稳定的 built-in tools，让 `bash` 回到 escape hatch 角色。

## Goals

- 降低 chat / explore / routing step 对 `bash` 拼接命令的依赖。
- 为仓库分析类任务提供稳定的结构化只读工具集合。
- 让工具结果从“纯字符串”升级为“文本 + 元数据 + 诊断 + 截断信息”的稳定 contract。
- 保持现有 workflow step tool filtering、runtime tool lifecycle 和安全边界不退化。
- 为后续 `subagent`、`batch`、代码智能和更细粒度权限控制建立稳定底座。

## Non-Goals

- 不把 Omega 变成通用 shell 自动化器。
- 不在本轮直接引入完整 MCP/plugin 生态替代当前内建工具。
- 不要求首轮就支持所有 IDE/LSP 能力或跨仓库索引。
- 不通过无约束放宽 shell redirection、expansion 或 workspace escape 来解决当前问题。

## Current Assessment

### Strengths

- `omega-tools` 的 `ToolHandler` / `ToolDispatcher` 边界清晰，扩展点稳定。
- `omega-session` 已具备 step 级 tool visibility resolve，运行时不会把 workflow policy 写死在 agent loop 内。
- `omega-session` 与 `omega-tui` 已经有 `ToolRun` lifecycle，可承接更丰富的工具元数据。
- `omega-tools-builtin` 已具备 workspace path safety、timeout 和基础文件工具。
- `.omega` 配置已经开始承载 tool policy，说明 repo-local customization 方向成立。

### Weaknesses

- `bash` 仍承担了目录浏览、文件搜索、内容搜索、输出裁剪等太多本应结构化的行为。
- 当前 `ToolHandler::execute` 只返回 `Result<String>`，tool 自身无法稳定表达 title、preview、truncated、attachments、diagnostics、error kind 等结构化信息。
- `read_file` / `write_file` / `edit_file` 能力偏薄，缺少更贴近日常代理工作流的 list/glob/grep/apply_patch/multi-edit 能力。
- 常见只读探索任务无法并行化执行，agent 容易在多个小 bash 调用上消耗 loop budget。
- `bash` 当前使用字符串归一化 + `shlex` 解析做校验，安全边界清晰，但对真实 shell 语义理解能力弱，容易出现“命令名允许但语法被拦”的体验裂缝。
- runtime 已有 typed tool-run envelope，但 tool result 本身仍然缺少统一 typed payload，导致 preview 和详情质量受限。

## Observed Failure Pattern

以“分析这个项目的优劣”为例，当前 agent 会自然尝试以下动作：列目录、找 Cargo manifest、搜索 docs/specs、统计 crate 结构、抽样读取核心文件。这类任务本质上是结构化 workspace inspection，但当前工具面不足，模型只能退回到：

- `find ... -exec ...`
- `head ... 2>/dev/null || ...`
- `wc ... | tail ...`
- `grep ...`

这些命令即使意图是只读的，也会因为 shell redirection、复杂 quoting 或 `find` action 被拦截，最终把 step loop 的 budget 消耗在纠错上，而不是实际分析上。

## Architecture Direction

### 1. Tool Contract V2

保留当前 trait-based registry，但把 handler 的结果从单一字符串升级为统一结构。

建议新增：

| Field | Type | Purpose |
|------|------|---------|
| `output` | `String` | 给模型回看的主要文本内容 |
| `preview` | `Option<String>` | 一行或少量行的摘要 |
| `metadata` | `serde_json::Value` | 结构化附加信息 |
| `truncated` | `bool` | 明确标记输出是否被截断 |
| `error_kind` | `Option<ToolErrorKind>` | 把安全拒绝、校验失败、执行失败分开 |

`title` 和 `artifacts` 延后到有明确消费者时再加。

兼容策略：

- 首轮保留 `String` 兼容包装，允许旧 handler 通过 adapter 自动映射到 `ToolResult`。
- `omega-core` 与 `omega-session` 统一消费 `ToolResult`，不再依赖字符串启发式生成 preview。

### 2. Built-in Tool Families

下一阶段应把 tool 分成四个稳定族，而不是继续把能力塞进 `bash`。

#### Workspace Inspection

- `list_dir`: 列目录，返回稳定排序的 child 列表。
- `glob_search`: 按 glob 查找文件路径。
- `grep_search`: 按正则或普通文本搜索内容，返回文件/行号/摘要。
- `read_file`: 升级为显式 `start_line` / `end_line` 或 `offset` / `limit` 语义，而不是只给 path + limit。
- 可选后续：`read_manifest` / `workspace_overview` 这类 repo-aware 高层工具。

#### Structured Editing

- `apply_patch`: 面向文本补丁的主编辑工具，降低模型直接拼接整文件写入的概率。
- `create_file`: 明确“仅创建新文件”的能力。
- `replace_text` 或 `edit_file`: 保留精确替换，但补齐更好的失败反馈。
- 可选后续：`multi_edit`，用于同文件多块替换。

#### Orchestration

- `batch`: 并行执行多个只读工具，减少多个小调用消耗的回合数。
- `task`: 将复杂检索或子问题委托给 subagent；此项应与 `Task 10` 汇合推进。
- `todo_read`: 当前只有 `todo` 写入，后续可补读取工具让模型显式查询状态，而不是依赖 reminder。
- 可选后续：`question`，把真正需要用户确认的分支显式抬升。

#### Escape Hatch

- `bash` 保留，但明确降级为 fallback。
- 常见 repo inspection prompt 默认应优先列出结构化只读工具，不鼓励 shell 组合拳。

### 3. Bash V2 Direction

`bash` 不应继续承担"默认查找器"角色，但仍值得做参数层增强：增加 `workdir`、`description`、更清晰的 timeout 语义与 policy error 分类。

AST/argv-aware 校验评估移出本轮 scope——8D 补齐后 bash 承载的主路径职责大幅下降，当前字符串归一化策略够用。后续如果观察到仍有高频误判，再单独立项评估。

重要边界：

- 不因为体验问题直接放开 shell expansion、workspace 外访问、危险 `find` action。
- 是否允许极窄范围的 stderr redirection，如 `2>/dev/null`，应作为单独决策，不与本轮结构化工具补齐捆绑。

### 4. Runtime And UI Integration

`omega-session` 已有 `ToolRun` lifecycle，因此 Tool Contract V2 的主要落点应是：

- `invocation_preview` 来自 tool 自身定义，而不是 session 侧猜测。
- `result_preview` 优先使用 tool 的 `preview` 字段。
- 详情 overlay 可直接展示 tool metadata、diff、diagnostics、truncation 来源。
- tool failure 在 UI 中区分 `validation / policy / execution / timeout`，帮助模型和用户都能更快纠偏。

### 5. Configuration And Permissions

当前 `.omega/model.toml` 已承载一部分 `[tools.bash]` 配置。后续应把策略扩展为更通用的 tool policy 面：

- 每个 tool 的 enable/disable。
- 只读工具默认可见集。
- bash allowlist 与 timeout。
- 是否允许 external directory 请求。
- batch 并发上限。

短期内可继续放在 `[tools.*]` 下；当配置面明显膨胀时，再考虑拆到专门的 `.omega/tools.toml`。

## Recommended Upgrade Tasks

### Task 8C: omega-tools / omega-core / omega-session — Tool Contract V2

- **Type**: Design + Implementation
- **Complexity**: M
- **Dependencies**: None
- **Description**: 为内建工具建立统一 `ToolResult` 结构（首轮 5 字段：output/preview/metadata/truncated/error_kind），保留对旧 `String` 返回值的兼容包装，并让 runtime tool lifecycle 直接消费 typed preview/metadata。
- **Deliverable**: `ToolHandler` v2 contract、compat adapter、`omega-core`/`omega-session` 接线、对应测试与文档更新。
- **Note**: session 已有完整 `ToolRun`/`ToolRunDetail`/`ToolRunStatus`，不需要重新设计 UI contract。

### Task 8D: omega-tools-builtin — Structured Workspace Inspection Tools

- **Type**: Implementation
- **Complexity**: L
- **Dependencies**: None（用现有 `Result<String>` 即可工作；8C 落地后回补结构化返回值）
- **Description**: 新增 `list_dir`、`glob_search`、`grep_search`，并为 `read_file` 增加 `start_line`/`end_line` 范围读取语义，覆盖当前 repo analysis 最常见的只读探索场景。同时统一 `BashHandler.validate_path_within_root` 与文件 handler 区域的 `safe_path_within_root` 为模块级共享函数。
- **Deliverable**: 新工具 handlers、schema、路径安全校验、截断与排序策略、针对真实仓库分析场景的回归测试。

### Task 8E: omega-tools-builtin — Patch-Centric Editing Toolset

- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: Task 8C
- **Priority Note**: 当前失败模式集中在读路径而非写路径，优先级从 High 降为 Medium。
- **Description**: 新增 `apply_patch` 与 `create_file`，并增强现有 `edit_file` / `write_file` 的错误反馈与 diff/diagnostic 元数据，使写路径更稳定。
- **Deliverable**: patch 工具、创建工具、结构化 diff 元数据、编辑后诊断反馈。

### Task 8F: omega-tools-builtin / omega-session — Batch Read-Only Tool

- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: Task 8C, Task 8D
- **Description**: 提供可并行执行多个只读工具调用的 `batch`，用于目录浏览、manifest 读取和 grep/glob 组合检索。
- **Deliverable**: 并行执行 contract、失败聚合格式、runtime tool-run integration、loop-budget 回归测试。

### Task 8G: omega-tools-builtin — Bash V2

- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: Task 8D
- **Description**: 为 `bash` 增加 `workdir`、`description`、更清晰的 policy error 分类。AST-based 校验评估移出本轮 scope。
- **Deliverable**: 更稳定的 bash schema 和 policy error surface；在结构化工具补齐后，prompt 也同步把 bash 降级为 fallback。

### Task 8H: omega-tools / omega-workflow / omega-app — Tool Policy Surface

- **Type**: Design + Implementation
- **Complexity**: M
- **Dependencies**: Task 8C, Task 8D, Task 8G
- **Description**: 为 repo-local tool config、step 级默认工具组、batch 限制和 bash policy 建立统一配置入口，并把 workflow presets 与 prompt 统一到新的工具面。
- **Deliverable**: `.omega` tool policy config、workflow/tool prompt 对齐、测试与文档更新。

### Task 10: omega-subagent — Task Tool Wiring

- **Type**: Implementation
- **Complexity**: L
- **Dependencies**: Task 8C, Task 8D
- **Description**: 在 fresh-context subagent loop 之上真正接入 `task` 工具，把复杂搜索/规划类工作从主 agent loop 中分流出去。subagent 需要结构化 inspection 能力，但不强依赖 batch。
- **Deliverable**: `task` tool、runtime 可见性、权限边界、回归测试。

## Task Ordering Rationale

推荐顺序：`8D -> 8C -> 8E -> 8F -> 8G -> 8H -> 10`。

原因如下：

- **先做 `8D`**：当前稳定性收益最大，用现有 `Result<String>` 即可工作，不需要等 contract 升级。`list_dir/glob_search/grep_search/read_file` 的范围读取能最快减少 repo analysis 对 bash 的依赖。
- **`8C` 紧跟**：给 8D 的新工具补 typed result，同时让后续新增工具从一开始就用新 contract。session 已有 `ToolRun` lifecycle，compat adapter 是标准模式，复杂度为 M。
- `8E` 让写路径与读路径一样结构化，避免 execute step 继续回退到整文件写入或 shell patch。当前痛点在读不在写，降为 Medium。
- `8F` 在只读工具补齐后价值最大，否则 batch 只是在并行跑脆弱 bash。
- `8G` 要做，但应该在 bash 不再承担主路径职责之后做。AST 评估移出 scope，只做参数增强，复杂度降为 M。
- `8H` 作为收口任务，把 config、workflow、prompt 和 runtime 重新对齐。
- `Task 10` 的 `task` tool 依赖 8C + 8D（结构化 inspection），但不强依赖 batch。

## Testing Strategy

- `omega-tools`: 为 `ToolResult` compat adapter、schema filtering 和 dispatcher 新增单测。
- `omega-tools-builtin`: 为每个新 tool 做路径安全、参数校验、截断、空结果和失败模式单测。
- `omega-core`: 增加多工具组合、batch 聚合和 visible-tools 回归测试。
- `omega-session`: 增加 chat / analysis workflow 的 trace-style 集成测试，验证同类任务不再因为 bash 细节消耗 loop budget。
- `omega-app` / `omega-tui`: 验证 tool metadata 能稳定进入 runtime UI。

重点回归场景：

- “分析当前项目优劣”
- “列出 workspace crate 并总结职责”
- “查找所有 workflow 配置并读取关键字段”
- “按补丁编辑单文件并返回诊断”

## Risks

- `ToolHandler` contract 升级会波及 `omega-tools`、`omega-core`、`omega-session`、`omega-tui`，需要兼容层避免一次性大爆炸。
- 如果先做太多高层智能工具而不先补齐基础 inspection/edit tools，模型仍会回退到 bash。
- `batch` 必须显式限制为 safe/readonly 子集，否则并行只会放大风险。
- `task` tool 的接入会引入权限和 runtime 复杂度，应该放在基础工具稳定之后。

## Change Log

- 2026-03-23: 审查修订——8D 解除对 8C 的阻塞依赖，执行顺序改为 `8D→8C→8E→8F→8G→8H→10`；8C 复杂度从 L 降为 M（缩减为 5 核心字段）；8E 优先级从 High 降为 Medium（当前痛点在读不在写）；8G 复杂度从 L 降为 M（AST 评估移出 scope）；Task 10 依赖从 8C+8F 改为 8C+8D；path safety 重复标注为 8D 子项。
- 2026-03-23: 初版 tool system upgrade 规划，基于当前 Omega tool runtime 与参考实现对比，明确把结构化 inspection/edit/orchestration tools 设为下一阶段主线。