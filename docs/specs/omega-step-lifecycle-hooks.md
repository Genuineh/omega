---
content_revision: 118
created: 2026-03-25
generation_id: gen_000037_r000118
last_verified_commit: N/A
owner: omega-team
projection_version: 37
related_prds: []
source_doc_id: "spec:docs-specs-omega-step-lifecycle-hooks"
status: implemented
supersedes: []
updated: 2026-03-25
---

# Omega Step Lifecycle Hooks

## Overview

当前 Omega 已经具备 `step prompt + tool_request + input/output contract + max_iterations` 的基础工作流模型，也已经为 `feature` / `research` 补上了最小可用的 todo-driven execute repeat。但这条主路径仍然把“什么时候允许离开当前 step”写死在 runtime 里，导致不同工作场景一旦出现不同的执行判据，就只能继续往 `omega-session` 里叠加条件分支。

本规格定义下一阶段的收敛方向：不引入独立的 `runtime_policy` 对象，而是把 **step 生命周期** 和 **能否进入下一步的判定** 收敛为显式的 Rust hook 机制。workflow 继续声明 step 的静态结构，runtime 继续拥有最终编排权；用户可以在 `.omega/hooks/` 下编写 Rust hook，通过统一的单方法接口参与 step 生命周期，并在 `BeforeAdvance` 阶段决定是否允许进入下一步。

## Current Baseline After Module Split

`Task 15F-15 ~ 15F-18` 已完成，因此本规格不再建立在 `omega-session` / `omega-workflow` 单体文件的假设之上。当前与 hook/gate 方向直接相关的真实落点如下：

- `crates/omega-session/src/runner.rs`: 当前 step 编排、output repair/regenerate、`StepTransition::Repeat` 与 `should_repeat_execute_step()` stopgap 所在位置，也是未来 `BeforeAdvance` gate 的主要接线点。
- `crates/omega-session/src/session_state.rs`: `SessionContext` 与后续 step-scoped hook storage 最自然的宿主边界。
- `crates/omega-session/src/ui_emit.rs` 与 `runtime_ui.rs`: hook diagnostics、advance deny/repeat decision 进入 Activity / Diagnostics / reducer 的稳定输出面。
- `crates/omega-workflow/src/model.rs`、`config.rs`、`defaults.rs`: `hooks[]`、`max_step_repeats` 与 builtin workflow 默认值应落到这里，而不是回到已拆解的 monolith。
- `crates/omega-session/src/lib_tests.rs`: 当前已存在 `SequencedClient`、`IdleClient`、repair/repeat 测试样式；Task 15F-19 应先从这里抽离稳定 test-support，而不是从零重新发明一套完全脱离现有用例的 harness。

这意味着后续任务的重点已经从“先拆大文件”转为“沿现有 split 模块继续收口 contract 与 runtime 注入点”。

`Task 15F-20` 已在 `omega-workflow` 侧落地最小 contract：`WorkflowStep` 现在显式携带 `max_step_repeats` 与 `hooks`，`WorkflowConfig` 已支持对应 TOML 字段，builtin `feature/research` execute 默认会写出 `max_step_repeats = 8` 与 `hooks = ["todo_managed_execute"]`，从而先把生命周期扩展点固定为声明式 step 结构。

`Task 15F-21` 与 `Task 15F-22` 现已把 host/runtime 边界与 advance gate 都落地到真实代码：workspace 新增独立 `omega-hooks` crate，负责 `.omega/hooks/*/Hook.toml` manifest catalog、artifact loading、`HookHost` / `HookSession` dispatch 与 step-scoped storage；`omega-session` 则通过 `hook_adapter.rs` 在 `runner.rs` 中接入 `BeforeStep`、`AfterToolCall`、`BeforeAdvance`、`AfterStep` 与 `StepFailed`，并用 `BeforeAdvance` deny 统一驱动 repeat / fail 判定，替换原先 execute repeat stopgap。

## Goals

- 为 step 建立统一的生命周期事件模型，而不是散落的 before/after 特例。
- 允许 workflow step 通过配置引用 `.omega/hooks/` 下的 Rust hook。
- hook 只需要实现一个方法，即可处理 before-step、after-step、tool 周期、advance gate 等生命周期事件。
- hook 可以读取 step 可见工具、session context、todo 状态，并拥有当前 step 生命周期内可持久的运行时存储。
- “是否允许进入下一步”由 runtime 在 hook 参与下判定；若不允许，则保持当前 step 重复执行直到达到 repeat 上限。
- 为相关逻辑提供 deterministic mock LLM harness，使 workflow 行为可以在不依赖真实模型稳定性的情况下验证。

## Non-Goals

- 不把 step 编排权下放给 prompt 或 tool 自行决定。
- 不让 tool 直接控制 workflow advance；tool 只产生事实，advance 由 runtime 判定。
- 不在 v1 支持任意全局 hook 事件总线或跨 turn 持久插件状态。
- 不在 v1 允许 hook 绕过 step 的可见工具集或直接突破 workspace 安全边界。
- 不在 v1 自动为 `.omega/hooks/` 做隐式编译；runtime 只负责发现并加载已构建好的 hook artifact。

## Design Principles

### No Separate Runtime Policy Layer

workflow step 只需要额外声明两类信息：

- 当前 step 绑定了哪些 hook
- 当前 step 最多允许因为 advance 被拒而重复多少次

是否进入下一步由 hook 和现有 output/input contract 共同决定；不再引入独立的 `runtime_policy` 配置对象。

### Lifecycle Is Explicit

`before_step`、`after_step`、`advance gate` 都属于同一个 step 生命周期，不应继续以若干散落布尔分支存在。runtime 必须把这些点收敛为统一的事件模型。

### Hooks Are Runtime Helpers, Not Hidden Orchestrators

hook 可以影响 step 是否继续停留在当前阶段，但不能绕过 session 的最终编排边界：

- required output contract 仍然优先于 hook
- session 仍然拥有 repeat budget 和最终 error/advance 决策
- hook 只能通过显式 API 读写上下文、todo、storage 和工具

## Architecture

### Components

- `omega-workflow`: 解析 step 级 `hooks` 与 `max_step_repeats` 配置。
- `omega-session`: 在 step 生命周期边界分发 hook 事件，收集 advance 决策，并维持 step-scoped runtime storage。
- `omega-hooks`: 负责 hook manifest 解析、artifact 动态加载、hook dispatch 和 step-scoped storage。
- `.omega/hooks/`: 用户编写的 Rust hook 源码、manifest 与构建产物所在目录。
- deterministic workflow test harness: 已由 `omega-client::test_support` 与现有 session tests 提供，为 hook/gate/repeat/repair 路径提供可预测的脚本化验证环境。

### Lifecycle Events

单个 step 的最小生命周期事件定义为：

1. `BeforeStep`
2. `BeforeModelTurn`
3. `AfterModelTurn`
4. `BeforeToolCall`
5. `AfterToolCall`
6. `BeforeAdvance`
7. `AfterStep`
8. `StepFailed`

v1 重点落地 `BeforeStep`、`AfterToolCall`、`BeforeAdvance` 与 `AfterStep`；其余事件保持接口预留，避免未来再次断裂式改动。

## Hook Programming Model

### Single-Method Contract

用户 hook 只需要实现一个入口方法：

```rust
pub trait StepHook {
    fn handle(&mut self, event: StepHookEvent, ctx: &mut StepHookContext) -> HookResult;
}
```

其中：

- `event` 描述当前处于 step 生命周期的哪个阶段
- `ctx` 提供受控的 runtime 视图与可变能力
- `HookResult` 用于报告 diagnostics、记录 storage 变化、以及在 `BeforeAdvance` 阶段表达 allow/deny

用户不需要分别实现 `before_step()`、`after_step()`、`should_advance()` 三个方法；只需在单方法里按 `event` 分支处理。

### Recommended Export Shape

为避免插件样板代码膨胀，host SDK 应提供导出宏，例如：

```rust
use omega_hook_sdk::{export_step_hook, HookResult, StepHookContext, StepHookEvent};

struct TodoManagedExecuteHook;

impl TodoManagedExecuteHook {
    fn handle(&mut self, event: StepHookEvent, ctx: &mut StepHookContext) -> HookResult {
        match event {
            StepHookEvent::BeforeStep => HookResult::continue_run(),
            StepHookEvent::BeforeAdvance => {
                if ctx.todo().has_open_items() {
                    ctx.deny_advance("todo items remain open");
                }
                HookResult::continue_run()
            }
            _ => HookResult::continue_run(),
        }
    }
}

export_step_hook!(TodoManagedExecuteHook);
```

## Hook Context Surface

`StepHookContext` 必须显式暴露以下能力：

- `workflow()` / `step()` / `iteration()`：当前执行位置与重复次数
- `session_context()`：只读的 `SessionContext` 快照
- `structured_input()` / `structured_output()`：当前 step 已知的结构化输入输出
- `todo()`：读取与更新当前共享 todo 状态的受控接口
- `tools()`：查看当前 step 可见工具，并通过 host 提供的方法调用这些工具
- `storage()`：当前 step 生命周期内的可变运行时存储
- `diagnostics()`：向 runtime/TUI 追加 hook 诊断信息
- `deny_advance(reason)`：仅在 `BeforeAdvance` 阶段生效，要求 runtime 保持当前 step

### Tool Visibility Rule

hook 只能调用当前 step 已经可见的工具。也就是说：

- `research.execute` hook 只能看到 research execute 允许的只读工具
- `feature.execute` hook 可以看到写工具，因为该 step 本身具备这些能力
- root routing hook 仍然受 root tool block 约束

hook 不应成为突破 tool policy 的后门。

### Runtime Storage Rule

v1 只提供 **step-scoped** runtime storage：

- 生命周期从 `BeforeStep` 开始，到 `AfterStep` / `StepFailed` 结束
- 会跨当前 step 的 repeated execute 保持
- 默认按 hook id namespaced，避免多个 hook 互相踩写
- 数据模型使用 JSON-like value map，避免 host/plugin 边界上的任意 Rust 类型耦合

这足以支撑：

- todo execute 中的 no-progress 计数
- 证据累积类 research hook
- gating 前的中间状态缓存

workflow-scoped 或 turn-scoped hook storage 留作后续扩展，而不是 v1 必选项。

## Advance Gate Model

### Base Rule

step 结束后，session 按以下顺序决定是否允许进入下一步：

1. 先检查 `output_contract`
2. 若 required structured output 尚未满足，则当前 step 不允许 advance
3. 若 output contract 已满足，则触发 `BeforeAdvance` hook 事件
4. 若任一 hook 拒绝 advance，则保持当前 step，进入下一轮 repeat
5. 若没有 hook 拒绝 advance，则允许进入下一 step

因此，advance gate 不是新的 policy 对象，而是当前 step 生命周期中的一个标准事件点。

### Repeat Budget

workflow step 新增：

- `hooks = ["..."]`
- `max_step_repeats = N`

当 `BeforeAdvance` 被拒绝时：

- 若 repeat 次数尚未达到上限，则重复当前 step
- 若已达到上限，则 session 以 step failure 结束，而不是静默跳到下一步

这让“保持当前步骤迭代直到满足条件，或者直到达到步级上限”成为统一机制。

### Why This Is Better Than Tool-Owned Flow Control

tool 可以更新 todo、写入上下文、产生验证结果，但不能直接决定 workflow advance。原因是：

- tool output 是事实，不是 orchestration 决策
- advance 判定需要聚合 output contract、todo 状态、storage、hook 诊断
- 把流程控制塞进 tool 会形成隐式 contract，难以测试和调试

## Execute Item Loop Follow-up

### Problem After Task 15F-22

当前 `todo_managed_execute` 已能把 `BeforeAdvance` deny/allow 收敛为统一 hook gate，但 runtime 仍把整个 `execute` 当成单个 step：

- repeat budget 仍直接作用于整个 `execute`，而不是某个明确的 todo item / work item
- `Contract Diagnostics` 只能解释“当前 step 被拒绝前进”，还不能稳定解释“总共有多少 todo、完成了几个、当前正在推进哪个 item、为什么这一轮算 no progress”
- 当 step 结束、失败或耗尽 `max_step_repeats` 时，前端只能看到 whole-step 结果，难以区分“部分 item 已完成”与“整个 execute 已完成”

这说明 `BeforeAdvance` gate 已经是正确方向，但还需要从 **whole-step repeat** 继续收敛到 **itemized execute loop**。

### Design Goal

对用户语义来说，这看起来像是“`loop_mode` 支持基于列表循环”；但在 contract 上，不应把“单次 agent/tool loop”与“按 item 展开外层循环”混在一个字段里。更稳妥的目标是：

- 保留当前 step 内部的 `agent_loop` / max_iterations 语义，继续表示一次 item 执行中的模型-工具回合
- 为 `execute` 新增显式的 item loop contract，声明“这个 step 应围绕哪个列表 source 逐项推进”
- runtime 将逻辑上的单个 `execute` 展开为共享上下文的 item runs，例如 `execute-1`、`execute-2`

这样既满足“配置后可以循环这个 loop 完成任务”的产品语义，也不会把两层循环语义塞进同一个布尔/字符串开关里。

### Proposed Contract Shape

建议新增 step 级 loop contract，首轮只覆盖 todo / plan task 驱动的 execute：

```toml
[[steps]]
id = "execute"
label = "Execute"
loop_mode = "agent_loop"
loop_contract = { kind = "todo_items", source = "plan.tasks", child_step_prefix = "execute", max_item_repeats = 3 }
hooks = ["todo_managed_execute"]
max_step_repeats = 10
```

其中：

- `loop_mode = "agent_loop"` 继续表示单个 item 内部仍由 bounded agent/tool loop 驱动
- `loop_contract.kind = "todo_items"` 表示外层按 todo/item 列表展开
- `source = "plan.tasks"` 表示首轮从 `plan` 输出与共享 todo state 推导 loop items
- `child_step_prefix = "execute"` 约定运行态子 id，形成 `execute-1`、`execute-2` 这类稳定 identity
- `max_item_repeats = 3` 是单个 item 被 deny 后的最大重试上限；`max_step_repeats` 仍保留为整个 execute 的安全总预算，两层分别控制

`max_step_repeats` 与 `max_item_repeats` 的关系：`max_step_repeats` 是所有 item 累计消耗的总重试次数上限，`max_item_repeats` 是单个 item 连续 deny 的独立上限。当任一层级触顶时，runtime 以 step failure 结束。这避免了单个难解 item 独占全部重试预算。

如后续确实需要把 `loop_mode` 升级成更复杂的复合类型，也应在保持这两个维度分离的前提下演进，而不是继续复用当前 parse-only alias。

### Runtime Behavior

当 step 配置了 item loop contract 时，runtime 规则应变为：

1. 父级 `execute` 进入时，先解析 loop items（首轮来自 todo / `plan.tasks`）
2. 为当前 item 生成稳定子 id，例如 `execute-1`
3. 单个 item run 内部继续使用现有 `agent_loop + BeforeAdvance + output contract + hook storage`
4. item 完成后，更新 todo state、item diagnostics 与 loop cursor，再决定是否进入下一个 item
5. 只有当 required items 全部完成后，父级 `execute` 才允许进入 `report`

#### BeforeAdvance 双层语义过渡

引入 item loop 后，`BeforeAdvance` 的语义从"整个 execute 能否前进"变为两阶段判定：

1. **per-item gate**: hook 继续回答"当前 item 是否已满足完成条件"（deny = 当前 item 继续重试，allow = 当前 item 完成）
2. **item progression**: runtime 接管 item 推进——当前 item 完成后，runtime 查看 loop cursor，决定切到下一个 item 还是全部完成后允许父级 advance

hook 不需要理解 item loop 的 orchestration 逻辑。`todo_managed_execute` 只需获知 `current_item_id` 并据此缩窄判断范围（例如只检查当前 todo item 的完成状态），item 切换与父级 advance 由 runtime 负责。为此，`HookDispatchInput` 应新增 item context 字段（additive-only change，不需要 `api_version` bump）：

```rust
pub struct HookDispatchInput {
    // ...existing fields...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_total: Option<usize>,
}
```

现有外部 hook 不受影响：JSON-over-C-ABI 加 `#[serde(default)]` 保证 additive 兼容。

#### Loop Source Resolution Ownership

`loop_contract.source` 的解析由 `runner.rs` 负责，而非 hook 或 workflow 层。Runner 已持有 `session_context.step_outputs` 与 `todo_manager`，且解析应封装为可测试的纯函数：

```rust
// crates/omega-session/src/runner.rs
fn resolve_loop_items(
    loop_contract: &StepLoopContract,
    step_outputs: &BTreeMap<String, Value>,
    todo_manager: &CoreSharedTodoManager,
) -> anyhow::Result<Vec<LoopItem>>
```

这让 item 解析与 runtime loop 编排可以独立测试，同时保持 hook host 不参与 orchestration。

### Visibility And Diagnostics Requirements

`Contract Diagnostics` 和 runtime-visible activity 至少应新增以下 execute 级字段：

- `todo_total`
- `todo_completed`
- `todo_open`
- `current_item_id`
- `current_item_index` / `current_item_total`
- `repeat_count_for_item`
- `no_progress_streak_for_item`
- `completion_source`（tool call / structured output / hook decision / manual todo update）

TUI/Activity 层应能同时展示父级 `execute` 与当前子 item，例如：

- `step  child:research  Execute  [streaming]`
- `item  execute-1  #risk-1  [streaming]`

这样用户看到的不再只是“Execute repeated / finished”，而是“当前在做哪个 item、已完成多少、为什么继续循环”。

### Rollout Extension

在 `Task 15F-22` 之后继续追加：

6. `Task 15F-26`: 扩展 execute diagnostics，先把 todo 进展与 repeat/no-progress 指标稳定暴露到 diagnostics / tracing / runtime activity。
7. `Task 15F-27`: 在 `omega-workflow` / `omega-session` 中引入 item loop contract，固定 `execute` 的外层列表循环模型与子 step identity。
8. `Task 15F-28`: 落地 itemized execute loop runtime、visibility 和 deterministic tests，用 item-level completion 取代 whole-step repeat 的最终 stopgap。

## Hook Loading Model

### Workspace Layout

建议目录约定：

```text
.omega/
  hooks/
    todo_managed_execute/
      Cargo.toml
      src/lib.rs
      Hook.toml
      target/.../libtodo_managed_execute.so
```

`Hook.toml` 至少描述：

- hook id
- crate/package name
- compiled artifact path
- supported host API version

推荐的最小 manifest 形状为：

```toml
id = "todo_managed_execute"
package = "todo_managed_execute"
artifact = "target/release/libtodo_managed_execute.so"
api_version = 1
```

其中 step 中的 `hooks = ["todo_managed_execute"]` 直接引用这里的 `id`，manifest 目录约定为 `.omega/hooks/todo_managed_execute/Hook.toml`。

当前实现还提供了一层 host 内建 fallback：若 step 绑定了 `todo_managed_execute` 且 workspace 中不存在对应 manifest，`omega-hooks` 会使用内建 hook 逻辑继续执行 `BeforeAdvance` deny/allow 判定。这样默认 `feature` / `research` execute workflow 不需要先在仓库中物化 hook artifact，也能保留 todo-driven repeat 语义；repo-local manifest 一旦存在，仍以 manifest + artifact 路径为准。

### ABI Strategy

运行时加载用户 Rust hook 不能依赖原生 Rust trait object ABI。`Task 15F-21` 的当前实现已采用更窄的 JSON-over-C-ABI 边界：host 通过 `libloading` 调用导出的 `omega_hook_api_version`、`omega_hook_invoke_json` 与 `omega_hook_free_string`，以 JSON payload 交换 lifecycle input/output，并用真实编译 fixture 覆盖 manifest/loader/dispatch 路径。

这让 v1 先把 host/runtime contract 固定下来，而不把插件侧绑定到不稳定的 Rust trait object ABI；如后续需要更丰富的 typed SDK，再在保持该边界或兼容升级路径的前提下演进。

### Build Strategy

v1 只要求 runtime **加载** 已构建好的 hook artifact，不要求 host 在运行中自动调用 Cargo 编译 `.omega/hooks/`。后续如需补 build helper，可作为独立体验增强，而不是核心 contract 的一部分。

## Workflow Configuration Surface

step 配置未来可收敛为：

```toml
[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "agent_loop"
max_iterations = 200
max_step_repeats = 8
hooks = ["todo_managed_execute"]
tool_request = { mode = "inherit" }
input_contract = { mode = "required", sources = ["plan"] }
output_contract = { mode = "optional", format = "json", schema_path = ".omega/schema/step/execute.json" }
```

这里没有额外的 `runtime_policy` 字段；step 的运行时完成语义来自：

- output/input contract
- hook 列表
- repeat 上限

## Deterministic Testing Strategy

LLM 不可控性不能成为 workflow runtime 行为不可验证的理由。为支撑 hook/gate/repeat 的迭代，测试层必须补一个稳定的 scripted mock harness。

### Target Capabilities

测试基建至少要能脚本化模拟：

- 正常 response / streaming response
- structured output valid / invalid / regenerate
- tool_use 序列
- execute no-progress / partial-progress / full-progress
- hook deny/allow advance
- repeat exhaustion
- scene-specific execute 差异（feature vs research）

### Recommended Shape

- 先把当前 `omega-session/src/lib_tests.rs` 内部 ad-hoc 的 `SequencedClient` / `IdleClient` 抽成稳定的测试支持模块；只有在 `omega-session`、`omega-client`、`omega-core` 都开始复用后，再提升为独立 crate
- 提供 `ScriptedLlmClient` / `ScriptedWorkflowHarness` builder，而不是继续在每个测试里手写 response 向量
- 为 hook host 提供两类测试：
  - in-process fake hook，用于快测 lifecycle dispatch
  - real compiled fixture hook，用于验证 loader/ABI/manifest

### Why This Matters

未来一旦引入 step hooks、advance gates、step-scoped storage，就不能只靠 prompt 文案和人工试跑验证 workflow 逻辑。deterministic mock harness 是保证 Task 10、Task 11 和未来 scene-specific execute 迭代稳定性的前置条件。

## Rollout Plan

1. `Task 15F-19`: 抽离 deterministic scripted workflow harness，优先复用 `omega-session` 现有 repair/repeat 场景而不是重写测试语料。
2. `Task 15F-20`: 已完成，在 `omega-workflow::{model,config,defaults}` 与 `.omega/workflows/*.toml` 中引入 `hooks[]` / `max_step_repeats` / manifest contract。
3. `Task 15F-21`: 已完成，独立 hook host、step-scoped storage、manifest 解析与 lifecycle dispatch adapter 已落地；`omega-session` 已通过窄接口接入 `BeforeStep`、`AfterToolCall`、`AfterStep` 与 `StepFailed`。
4. `Task 15F-22`: 已完成，在 `omega-session::runner` 中用 `BeforeAdvance` gate 替换当前 `should_repeat_execute_step()` stopgap，并把 deny/repeat diagnostics 通过现有 runtime UI warning/error/log 通道暴露出来。
5. 已完成：`feature.execute` / `research.execute` 已绑定 `todo_managed_execute`，并由 host 内建 fallback 提供首个 stopgap → hook contract 的迁移验证。

## Task Mapping After Split

### Task 15F-19

- 输出物应首先是稳定 test-support，而不是立即引入新的 production crate。
- 目标是为 `runner.rs` 的 repair/regenerate、tool_use、repeat 与后续 hook gate 提供 deterministic regression safety。

### Task 15F-20

- 已完成：workflow step contract 现已显式携带 `hooks[]` 与 `max_step_repeats`，并在默认 execute workflow TOML 中留下 manifest 约定。
- 仍然不在本任务内承担 artifact loading、ABI 选择或 host runtime 接线。

### Task 15F-21

- 已完成：`omega-hooks` 已承接 hook host、loader、dispatch、storage 与 fixture testing；`omega-session` 通过 `hook_adapter` 消费 host，而没有把 loader 细节重新塞回 session 根模块。
- 当前已接入的 runtime 事件是 `BeforeStep`、`AfterToolCall`、`BeforeAdvance`、`AfterStep` 与 `StepFailed`。

### Task 15F-22

- 已完成：`runner.rs` 现已在 output contract 通过后统一调用 `BeforeAdvance` gate，并按 `max_step_repeats` 处理 deny → repeat / deny exhaustion → fail。
- 旧的 `should_repeat_execute_step()` 与 `EXECUTE_REPEAT_MAX_NO_PROGRESS_ATTEMPTS` special-case 路径已删除；默认 execute workflow 改为显式依赖 `hooks = ["todo_managed_execute"]`。

## Open Questions

- v1 是否需要 workflow-scoped hook storage，还是 step-scoped 已足够
- hook diagnostics 是否需要在保留当前 Activity/Log 输出之外，再同步进入 `RuntimeUiEffect::UpsertStepDiagnostics`
- build helper 是放进 `omega-tools-builtin`、单独 crate，还是晚些再做
- ~~item loop contract 首轮是否只允许 `todo_items`，还是在 v1 就支持任意 `structured_output` 列表 source~~ → 已定：v1 只允许 `todo_items`，source 限 `plan.tasks`；解析由 `runner.rs` 负责

---

### Change Log
- 2026-03-26: 架构评审后补充：明确 `BeforeAdvance` 双层语义过渡（per-item gate + runtime-owned item progression）、`max_item_repeats` 与 `max_step_repeats` 双层预算关系、`runner.rs` 拥有 loop source resolution、`HookDispatchInput` 新增 item context 为 additive-only change、关闭 v1 loop source scope open question。
- 2026-03-26: 新增 execute item loop follow-up，明确 `Task 15F-26 ~ 15F-28` 的方向：先补 execute todo progress diagnostics，再把 whole-step repeat 收敛为 itemized execute loop 与子 step identity。
- 2026-03-25: `Task 15F-22` 落地 `BeforeAdvance` runtime gate、repeat budget enforcement 与 `todo_managed_execute` 内建 fallback；默认 `feature` / `research` execute 现显式绑定该 hook，并补齐 `omega-hooks` / `omega-workflow` / `omega-session` 回归测试。
- 2026-03-25: `Task 15F-21` 落地独立 `omega-hooks` crate、`.omega/hooks/*/Hook.toml` manifest catalog、JSON-over-C-ABI loader、step-scoped storage 与 `omega-session` 的 `BeforeStep` / `AfterToolCall` / `AfterStep` / `StepFailed` lifecycle dispatch，并补齐真实 fixture + session integration tests。
- 2026-03-25: `Task 15F-20` 落地 `WorkflowStep::{hooks,max_step_repeats}`、TOML 解析与默认 execute contract，并把 `.omega/hooks/<hook-id>/Hook.toml` 的最小 manifest 字段固定为 `id/package/artifact/api_version`。
- 2026-03-25: 结合 `Task 15F-15 ~ 15F-18` 完成后的真实模块边界，补充 post-split insertion points，并把 15F-19~22 的 rollout/task mapping 收敛到 `runner`、workflow model/config/defaults 与独立 hook host 的明确职责划分。
- 2026-03-25: 初版规格，提出用单方法 Rust hook + lifecycle events + `BeforeAdvance` gate 统一 step 生命周期，并明确 deterministic mock LLM harness 是该方向的前置测试能力。
