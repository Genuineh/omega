---
content_revision: 96
created: 2026-04-08
generation_id: gen_000013_r000096
last_verified_commit: N/A
owner: omega-team
projection_version: 13
related_prds: []
source_doc_id: "spec:docs-specs-omega-root-skill-routing"
status: implemented
supersedes: []
updated: 2026-04-08
---

# Omega Root Skill Routing Specification

## Overview

当前 Omega 的 skill 加载仍是 step-local 行为：每个 workflow step 在执行前基于原始任务文本即时做一次 `match_task`，再把命中的 skill 正文拼进 system prompt。这条链路已经能工作，但它有两个结构性问题：root workflow 无法显式决定“本轮后续执行到底该预装哪些 skills”，而 child workflow 也无法区分“这是 root routing 已经选定的 capability”还是“每个 step 临时又猜了一次”。

本规格定义一个 root-owned skill routing 扩展：root workflow 在 `select-workflow` 之后增加 `select-skills` step，由它输出结构化 `selected_skill_ids` 列表；`omega-session` 先把这份结果写入 session-owned typed context，再在 child workflow 启动前执行固定 `load-skills` runtime action，把 routed skills 分流成 `recognized / loaded / ignored` 三类状态。child workflow 只消费 `loaded` baseline，step 级 `skill_request` 继续存在，但退化为 modifier，而不是唯一的 skill 选择来源。

Status note (2026-04-08): 当前基线已落地为两步 root workflow：`select-workflow` 负责场景与 child workflow 选择，`select-skills` 负责输出 routed `selected_skill_ids`。`omega-session` 随后会在 root → child 边界执行固定 `load-skills` action，把 root 输出分流成 `selected_skill_ids`、`loaded_skill_ids` 与 `ignored_skill_ids`；child workflow step 现只消费 `loaded_skill_ids`，再应用 step-level `skill_request` modifier。unknown skill id 不再污染 child prompt，而是仅记录到 ignored state；当 root JSON routing 退回到 text fallback 时，runtime 仍会合成结构化 `select-workflow` handoff，确保后续 `select-skills` 与 `load-skills` 不会中断。

## Goals

- 把“本轮应该预装哪些 skills”提升为 root workflow 的显式结构化决策。
- 让 root routing 输出不仅包含 `recognized_scene_id` / `selected_workflow_id`，还包含 `selected_skill_ids`。
- 让 child workflow 在执行前自动继承 root 选择的 skills，而不是每个 step 都重新猜一次。
- 保留 `StepSkillRequest` 的 step-level modifier 语义，使 `Append` / `Disable` 仍可局部覆盖。
- 为后续 runtime diagnostics、TUI 可视化和 replay/debug 提供稳定的 typed skill-routing state。

## Non-Goals

- 本轮不重新设计 `omega-skills::SkillLoader` 的关键词匹配算法。
- 本轮不把 skill routing 下沉到 `omega-app`、`omega-tui` 或 prompt 模板外层脚本。
- 本轮不要求 root step 直接读取仓库文件、搜索代码或探测 workspace；skill selection 仍以用户请求、routing context 和已知 catalog metadata 为主。
- 本轮不要求为每个 child step 引入独立 skill planner；root-selected skills 是 turn-scoped baseline，而不是无限细粒度的 per-step policy engine。

## Problem Statement

当前模型存在以下缺口：

- `StepSkillRequest::MatchTask` 直接绑定到每个 step 的即时任务匹配，skill 选择不经过 root workflow，也不进入 typed handoff。
- child workflow 无法区分“root 已经选定的核心 skill”与“step 本地临时命中的 skill”，导致诊断和回放都缺少稳定依据。
- 同一 turn 内多个 step 会对相同原始任务反复做 `match_task`，结果可能漂移，且会重复消耗 prompt 空间。
- root workflow 现在只决定场景和 child workflow，不决定 capability baseline；这使 routing contract 只完成了一半。

## Architecture

### Root Workflow Shape

root workflow 的目标形态调整为：

| Step | Purpose | Output |
|------|---------|--------|
| `select-workflow` | 识别 scene 并选择 child workflow | `recognized_scene_id`, `selected_workflow_id` |
| `select-skills` | 基于用户任务和已选 workflow 选择本轮应预装的 skills | `selected_skill_ids`, `selection_reason?` |

在 root workflow 之后、child workflow 之前，runtime 还会执行一个固定动作：

| Runtime Action | Purpose | Result |
|----------------|---------|--------|
| `load-skills` | 校验 routed skill ids 是否真实存在，并建立 child 可消费的 loaded baseline | `loaded_skill_ids`, `ignored_skill_ids` |

关键约束：

- `select-skills` 是 root workflow 内的普通 step，不是 `omega-session` 中的隐藏特殊逻辑。
- `select-skills` 读取 `select-workflow` 的结构化输出与 routing context，但不自行完成 skills 校验/加载。
- `load-skills` 是 root → child 边界上的固定 runtime action，不是额外的模型 step。
- child workflow 只有在 `select-skills` 完成且 `load-skills` action 执行后才开始执行。

### Ownership

- `omega-workflow`: 定义 root workflow step、`select-skills` prompt/config、输出 contract 与默认 preset。
- `omega-session`: 执行 root workflow、持久化 typed skill-routing state、在 child step 执行前注入 routed skills。
- `omega-skills`: 继续拥有 skill metadata、lookup 与 content loading；不拥有 root routing policy。
- `omega-tui`: 后续可消费 routed skill state，但不拥有选择逻辑。

## Typed Data Model

推荐在 `SessionContext` 中新增独立的 `SkillRoutingContext`，而不是把 skills 混进现有 `RoutingContext`：

```rust
pub struct SkillRoutingContext {
    pub selected_skill_ids: Vec<String>,
  pub loaded_skill_ids: Vec<String>,
  pub ignored_skill_ids: Vec<String>,
    pub selection_reason: Option<String>,
    pub source_step_id: String,
}

pub struct SessionContext {
    pub latest_user_turn: String,
    pub routing: RoutingContext,
    pub skill_routing: SkillRoutingContext,
    pub step_summaries: Vec<StepSummary>,
    pub step_outputs: BTreeMap<String, Value>,
    // ...
}
```

设计约束：

- `selected_skill_ids` 表示 root `select-skills` 识别到的 routed skill ids，不直接存渲染后的正文。
- `loaded_skill_ids` 表示固定 `load-skills` action 成功校验并允许 child prompt 继承的 skill ids。
- `ignored_skill_ids` 表示 root 识别到了、但当前 catalog 中不存在的 skill ids，用于 diagnostics/UI，可视为“recognized but not loadable”。
- runtime 必须在固定 `load-skills` action 中按当前 `SessionSkillCatalog` 做存在性校验、去重和顺序保留。
- 首版应把 routed skill 数量限制在 5 个以内，避免 child workflow 全程携带过大的 preloaded prompt。
- 未命中任何 skill 时，`selected_skill_ids = []` 是合法结果，不应视为 contract 失败。

## Output Contract

`select-skills` 应声明 required JSON output contract。目标输出形态：

```json
{
  "selected_skill_ids": ["docs-specs", "docs-todo", "plan"],
  "selection_reason": "task asks for planning and spec writing"
}
```

字段约束：

- `selected_skill_ids`: 必填数组，元素必须是 string。
- `selection_reason`: 可选 string，用于 diagnostics / logging，不直接暴露为用户回答正文。

推荐 schema 约束：

- `selected_skill_ids.maxItems = 5`
- `selected_skill_ids.uniqueItems = true`
- `selection_reason.maxLength = 240`

## Session Injection Semantics

child workflow step 执行前的 skill 解析顺序调整为：

1. root `select-skills` 先写入 `SessionContext.skill_routing.selected_skill_ids`。
2. runtime 在 root → child 边界执行固定 `load-skills` action，把 `selected_skill_ids` 分流为 `loaded_skill_ids` 与 `ignored_skill_ids`。
3. child step 读取 `SessionContext.skill_routing.loaded_skill_ids` 作为 routed baseline。
4. 再应用当前 step 的 `StepSkillRequest` 作为 modifier。

建议的兼容语义：

| `StepSkillRequest` | 新语义 |
|--------------------|--------|
| `Disable` | 忽略 root-selected skills，也不做 task match |
| `MatchTask` | 若 `loaded_skill_ids` 非空，则直接使用 loaded baseline；若为空，回退到现有 `match_task` |
| `Append(items)` | 以 `loaded_skill_ids` 为 baseline；若 baseline 为空则回退到 `match_task`，之后再追加显式 items |

这样做的原因：

- 保持 repo-local workflow config 的兼容面最小，不需要一次性重写所有 child step 的 `skill_request`。
- 一旦 root 已经明确选过 skills，child step 默认不应继续重复猜测。
- `Disable` 和 `Append` 仍保留局部 override 能力。

## Prompt And Workflow Rules

### `select-skills` Step

- prompt 只能基于用户请求、routing context、已选 workflow 和 skill catalog descriptions 做判断。
- 不允许读取仓库文件、列目录或运行工具去“研究后再选”。
- 默认 `skill_request` 应设为 `disable`，避免 skill selector 自己先被隐式 skill preload 污染。

### Child Workflow Steps

- 绝大多数 child step 继续保留 `skill_request = { mode = "match_task" }` 即可，但其运行时语义改为“继承 routed skills 或回退到 match_task”。
- 少数需要强制关闭 skills 的 step 继续使用 `disable`。
- 少数需要显式补充 skill 的 step 继续使用 `append`。

## Failure And Fallback Policy

- `select-skills` 输出缺失或 JSON 校验失败时，沿用现有 structured output repair/regenerate 机制。
- 若 `selected_skill_ids` 中存在未知 skill，runtime 应记录 warning 并在写入 `SkillRoutingContext` 前丢弃未知项，而不是整轮失败。
- 若 `select-skills` 最终仍为空数组，child step 退回现有 `match_task` 兼容路径。
- 若 root workflow 仍处于旧配置且没有 `select-skills` step，session 应保持当前 step-local `match_task` 语义，不破坏旧仓库配置。

## Migration Plan

### Phase 1: Model And Contract

- 为 root workflow 新增 `select-skills` step 与 JSON schema。
- 在 `SessionContext` 中新增 `SkillRoutingContext`。
- 扩展 `StepExecutionInput` / prompt builder，使 child step 能消费 routed skill state。

### Phase 2: Runtime Injection

- `omega-session` 在 root workflow 完成后先写入 `selected_skill_ids`，再在 child workflow 启动前执行固定 `load-skills` action。
- `SessionSkillCatalog` 新增 “from routed baseline + step modifier” 的解析入口。
- child workflow step 改为优先消费 `loaded_skill_ids`，不再直接使用原始未校验 routed ids。

### Phase 3: Validation And Visibility

- 为 root routing 与 child step prompt 组装补齐回归测试。
- 在 runtime diagnostics 中记录 routed skill ids 和 fallback path。
- 已通过显式 `Load Skills` response section、typed `SkillLoadSummary` state 和 Sidebar `Skills` panel 把 routed skills 暴露到 Response / Sidebar drill-down。

## Risks

| Risk | Level | Mitigation |
|------|-------|------------|
| root-selected skills 与 step-local `match_task` 双写，导致 prompt 重复膨胀 | High | 明确 `MatchTask` 在 routed baseline 非空时不再重新匹配 |
| skill selector 自己被预装 skills 影响判断 | High | `select-skills` step 默认 `skill_request = disable` |
| 旧 workflow config 没有 `select-skills` 时行为突变 | Medium | 保留无 routed skills 时的 `match_task` fallback |
| unknown skill id 让整轮 routing 失败 | Medium | runtime 丢弃未知 skill 并记录 diagnostics，而不是 hard fail |

## Testing Strategy

- `omega-workflow` 单测：`select-skills` step config、schema path 与 root workflow 默认 preset 加载。
- `omega-session` 单测：root workflow 写入 `SkillRoutingContext`，固定 `load-skills` action 产出 `loaded_skill_ids` / `ignored_skill_ids`，child workflow step 只继承 loaded skills。
- `omega-session` 单测：`Disable` 会屏蔽 routed skills，`Append` 会在 routed baseline 上追加，空 routed baseline 会回退到 `match_task`。
- `omega-session` 单测：空 `selected_skill_ids` 时跳过 `load-skills`；unknown skill id 只进入 ignored state，不终止 turn。
- 集成测试：root text-routing fallback 仍可完成 `select-workflow -> select-skills -> load-skills -> child workflow`，且 child prompt 只预装实际可加载的 skills。

---

### Change Log

- 2026-04-08: 初版规格，提出 root-owned `select-skills` step、`selected_skill_ids` output contract，以及 routed skill injection 对 child workflow 的兼容语义。
- 2026-04-08: Task 5A ~ 5C 已实现：默认 root workflow、新增 prompt/schema、`SkillRoutingContext`、routed skill precedence，以及 root text-routing fallback 的合成 structured handoff 全部落地。
- 2026-04-08: Task 5D ~ 5E 已实现：root → child 边界新增固定 `load-skills` runtime action，`SkillRoutingContext` 扩展为 recognized/loaded/ignored 三分状态，child workflow 只消费 `loaded_skill_ids`，并新增空 selection / unknown id / text fallback 回归覆盖。
- 2026-04-08: Task 15B-47 ~ 15B-48 已实现：`load-skills` 现在会发出显式 Response section 与 typed `SkillLoadSummary` runtime state；`omega-tui` Sidebar 新增 `Skills` 视图，并支持从 Response skill lane 与 Sidebar 进入同一个 routed skills detail overlay。
