---
content_revision: 101
created: 2026-04-11
generation_id: gen_000016_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 16
related_prds:
  - docs/specs/omega-project-system.md
  - docs/specs/omega-session-resume.md
  - docs/specs/omega-context-management.md
  - docs/specs/omega-app-package.md
source_doc_id: "spec:docs-specs-omega-project-path-layout"
status: active
supersedes: []
updated: 2026-04-13
---

# Omega Project Path Layout Specification

## Overview

当前 `.omega/` 同时承载了两类本质不同的东西：

1. **用户管理的 repo-local 预设与源码资产**：如 `env.toml`、`model.toml`、`keymap.toml`、`theme.toml`、`tui.toml`、`scenes.toml`、`workflows/`、`prompt/`、`schema/`、`.storeignore`。
2. **运行时生成的 project state**：如 `project.json`、`sessions/`、`memory/`、`store/`。

这会同时带来三个问题：

- 用户无法一眼判断“哪些文件适合跟仓库走，哪些是运行时产物”。
- 清理运行时数据时，路径边界不清晰，容易误删配置或把状态误提交进版本库。
- `.omega/...` 路径字符串已经分散在 `omega-app`、`omega-project`、`omega-session`、`omega-workflow`、`omega-keymap`、`omega-theme`、`omega-hooks`、`omega-memory`、`omega-document` 与 `omega-context` 中，任何一次路径迁移都很容易漏掉调用面、测试面或文档面。

本规格把 project-local Omega 路径拆成两个正式根目录：

- `.omega/`：repo-local config 与 source assets；面向用户，可编辑，可按仓库策略提交。
- `.omega-state/`：runtime-generated project state；面向运行时，默认不提交，可清理。

`~/.omega/` 这类 app-owned machine/global 路径不在本次变更范围内，继续承担日志等跨项目状态。

## Goals

- 明确 repo-local 配置与 runtime-generated project state 的边界。
- 保持当前用户可编辑的 Omega 配置入口可发现、可延续、可按仓库共享。
- 把 session、memory、document store、project metadata 等生成态从 `.omega/` 中迁出。
- 引入统一的 typed path contract，避免继续在各 crate 中散落 `.omega/...` 字面量。
- 提供从 legacy `.omega/<runtime>` 到 `.omega-state/` 的平滑迁移路径。
- 强制代码、测试、fixtures、用户可见提示与文档一起收口，避免 split-brain。

## Non-Goals

- 不迁移 `~/.omega/` 下的全局日志或机器级缓存。
- 不重写 `model.toml`、`theme.toml`、workflow TOML、prompt/schema 等配置文件内容格式。
- 不把所有运行时生成物都定义成“可无损重建的 cache”；其中一部分是可删除但有状态损失后果的 durable generated state。
- 不在本轮引入跨机器同步、云端 project registry 或 profile 级配置服务。

## Current Classification

| Current path | Current role | Problem | Target role |
|-------------|--------------|---------|-------------|
| `.omega/env.toml` | repo-local env defaults | 与 runtime state 混放 | 保留在 `.omega/` |
| `.omega/model.toml` | repo-local model/tool policy | 与 runtime state 混放 | 保留在 `.omega/` |
| `.omega/keymap.toml` / `.omega/theme.toml` / `.omega/tui.toml` | 用户可编辑 UI 配置 | 与 runtime state 混放 | 保留在 `.omega/` |
| `.omega/scenes.toml` / `.omega/workflows/` | workflow config | 与 runtime state 混放 | 保留在 `.omega/` |
| `.omega/prompt/` / `.omega/schema/` | prompt/schema source assets | 与 runtime state 混放 | 保留在 `.omega/` |
| `.omega/.storeignore` / `.omega/doc-rules.toml` | repo-local governance rules | 与 runtime state 混放 | 保留在 `.omega/` |
| `.omega/hooks/` | hook manifest/source，且当前隐含承载 artifact 位置 | source 与生成态混放 | 拆 source vs artifact |
| `.omega/project.json` | generated project metadata snapshot | 看起来像配置，实则运行时生成 | 迁到 `.omega-state/project.json` |
| `.omega/sessions/` | generated session state | 与配置混放 | 迁到 `.omega-state/sessions/` |
| `.omega/memory/` | generated repo-local memory store | 与配置混放 | 迁到 `.omega-state/memory/` |
| `.omega/store/` | generated document/index store | 与配置混放 | 迁到 `.omega-state/store/` |

## Proposed Layout

### Config Root

```text
<project-root>/.omega/
  env.toml
  model.toml
  keymap.toml
  theme.toml
  tui.toml
  scenes.toml
  project.toml              # optional user-managed project manifest
  workflows/
  prompt/
  schema/
  hooks/                    # hook source, manifest, checked-in assets
  .storeignore
  doc-rules.toml
```

规则：

- 这是用户管理的 repo-local surface。Omega 仍可在“缺失时生成默认模板”，因为这些文件本身就是用户可编辑合同的一部分。
- 这个目录中的内容应被视为“可随仓库共享的配置/源码资产”，而不是运行时缓存。
- `project.toml` 成为可选的 user-authored project manifest；project detection 不能再依赖 generated state 文件是否存在。

### State Root

```text
<project-root>/.omega-state/
  project.json              # generated project metadata snapshot
  sessions/
    <session-id>/
      session.json
      session.context.jsonl
  memory/
    turns/
    observations.jsonl
  store/
    files.jsonl
    tantivy/
    lance/
    history/
    staging/
    store-version.json
    index-commit-log.json
  hooks/
    <hook-id>/              # compiled artifacts or runtime-owned hook outputs
```

规则：

- 这是 runtime-owned 目录，默认应从版本控制中排除。
- 目录中同时允许两类生成态：
  - durable generated state：`project.json`、`sessions/`、`memory/`
  - rebuildable derived state：`store/`、hook build artifacts
- 删除 `.omega-state/` 是被允许的，但不是无成本的：`store/` 可以重建，`sessions/` 与 `memory/` 会失去已持久化的运行时历史。

## Path Resolution Contract

project-local 布局合同必须收口到单一 crate。当前最合理的 owning boundary 是 `omega-project`，因为它已经拥有 project identity、repo-scoped runtime surface 与 session catalog。

```rust
pub struct OmegaProjectLayout {
    pub root: PathBuf,
    pub config_root: PathBuf,
    pub state_root: PathBuf,
}

impl OmegaProjectLayout {
    pub fn project_manifest_path(&self) -> PathBuf;
    pub fn project_state_path(&self) -> PathBuf;
    pub fn sessions_dir(&self) -> PathBuf;
    pub fn session_dir(&self, session_id: &str) -> PathBuf;
    pub fn memory_dir(&self) -> PathBuf;
    pub fn store_dir(&self) -> PathBuf;
    pub fn hook_source_dir(&self) -> PathBuf;
    pub fn hook_artifact_dir(&self) -> PathBuf;

    pub fn model_config_path(&self) -> PathBuf;
    pub fn env_config_path(&self) -> PathBuf;
    pub fn keymap_path(&self) -> PathBuf;
    pub fn theme_path(&self) -> PathBuf;
    pub fn tui_config_path(&self) -> PathBuf;
    pub fn scenes_path(&self) -> PathBuf;
    pub fn workflows_dir(&self) -> PathBuf;
    pub fn prompt_dir(&self) -> PathBuf;
    pub fn schema_dir(&self) -> PathBuf;
    pub fn storeignore_path(&self) -> PathBuf;
    pub fn doc_rules_path(&self) -> PathBuf;
}
```

合同规则：

- production code 不得在 layout module 之外硬编码 `.omega/...` 或 `.omega-state/...`，除非是由 layout helper 派生出的 display-only 文本。
- config loaders 只消费 `config_root` helper。
- session、memory、document 与 project persistence 只消费 `state_root` helper。
- tests 与 fixtures 必须使用同一个 layout helper，避免新路径和测试样本再次分叉。

## Detection And Ownership Changes

### Project Detection

project detection 应按以下顺序工作：

1. explicit root selection
2. `.omega/project.toml`
3. `.git/`
4. `Cargo.toml`、`package.json` 等仓库入口文件

generated `.omega-state/project.json` 是 runtime state，不能再作为 canonical detection marker；它只应在 root 已知后作为 project snapshot 读取。

### Hook Split

hook source 与 hook runtime artifact 必须停止共用一个根目录：

- `.omega/hooks/<hook-id>/`：manifest、source、checked-in support files
- `.omega-state/hooks/<hook-id>/`：compiled artifact 或 runtime-owned outputs

这样才不会继续把 build outputs 伪装成可提交配置。

## Migration Strategy

### Phase 1: Layout API And Dual Read

- 引入 `OmegaProjectLayout`，所有新增读写先接到这个 helper 上。
- config readers 继续读写 `.omega/`。
- state readers 优先读 `.omega-state/`，但在迁移期保留对 legacy `.omega/<runtime>` 的 fallback。
- 用户可见 diagnostics 一旦切到 layout helper，就应优先显示新路径，避免继续强化旧路径心智。

### Phase 2: Lazy Legacy Migration

当 project 打开时，如果 `.omega-state/` 尚不存在、但 legacy runtime data 仍在 `.omega/` 下，则由 `omega-project` 触发已知路径迁移：

- `.omega/project.json` -> `.omega-state/project.json`
- `.omega/sessions/` -> `.omega-state/sessions/`
- `.omega/memory/` -> `.omega-state/memory/`
- `.omega/store/` -> `.omega-state/store/`
- hook artifacts -> `.omega-state/hooks/`

迁移规则：

- config files 与 source assets 绝不能被挪出 `.omega/`
- 迁移必须可重入、可重复执行
- 若新旧路径同时存在，只在 merge 规则确定时自动合并；否则给出 warning 并保留现场
- `store/` 允许在 merge 语义不清晰时选择 rebuild，而不是冒险拼接
- `sessions/` 与 `memory/` 必须优先保证保留，不允许为了“目录整洁”做破坏性处理

### Phase 3: Legacy Read Removal

只有在代码、测试、fixtures、用户提示和文档都完成迁移后，才移除 legacy `.omega/<runtime>` fallback。

## Impacted Components

| Component | Required change |
|----------|-----------------|
| `omega-project` | 新增 `OmegaProjectLayout`、project manifest/state split、legacy migration entrypoint |
| `omega-app` | 保持 config bootstrap 仍走 `.omega/*`，运行时 state surface 改经 layout helper |
| `omega-session` | session catalog 与 ledger 持久化迁到 `.omega-state/sessions/` |
| `omega-memory` | turn archive 与 observations 迁到 `.omega-state/memory/` |
| `omega-document` | manifest/tantivy/lance/version/history 迁到 `.omega-state/store/`，同时继续读取 `.omega/.storeignore` 与 `.omega/doc-rules.toml` |
| `omega-context` | diagnostics、governance path 与 command output 改为消费 layout helper |
| `omega-hooks` | 分离 hook source path 与 artifact path |
| `omega-workflow` | workflow/prompt/schema/config 继续留在 `.omega/`，并更新默认常量/注释/fixtures 的路径来源 |
| `omega-keymap` / `omega-theme` / `omega-tui` | 保持用户配置留在 `.omega/`，并修正文案，明确这不是 runtime state 目录 |
| docs/tests/fixtures | 更新路径引用、预期输出、fixture 生成逻辑与 contributor guidance |

## Testing Strategy

- layout resolution 与 per-path helper 的 unit tests
- legacy `.omega/` -> `.omega-state/` migration tests
- project/session resume tests，覆盖新 session root
- memory/document integration tests，覆盖新 state root 与旧 config root 的组合
- hook loader tests，覆盖 source/artifact split
- `crates/` 与 `docs/` 上的 broad search validation，用来抓 stale `.omega/<runtime>` 引用

## Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| 漏改 runtime path literal | 读写分裂、出现 split-brain state | 先落 layout helper，再做各 crate 迁移；不允许跳过 Task 18K 直改子模块 |
| 把 `.omega-state/project.json` 当作 detection marker | project identity 依赖 generated state | 引入 `.omega/project.toml`，并把 detection 严格限制在 config/repo markers |
| session 或 memory 迁移不完整 | 数据丢失 | 迁移遵循 preserve-first，必要时告警并保留双路径 |
| `.omega-state/` 未被忽略 | 运行时状态被误提交 | 在同一任务里更新 ignore guidance 与开发文档 |
| docs / fixtures 仍引用旧路径 | 后续实现再次回归旧合同 | 把 docs/tests sweep 设为单独收尾任务，不当作可选 cleanup |

## Acceptance Criteria

- 新贡献者查看项目根目录时，能一眼区分 Omega 的 config/source 与 generated state。
- production runtime persistence surface 不再把 session/memory/store/project metadata 写入 `.omega/`。
- project detection 与 config loading 不再依赖 generated runtime files。
- 搜索 production code 时，路径布局字面量只出现在共享 layout module 或显式批准的 config constants 中。
- TODO、开发指南、规格与测试对新路径布局达成一致。

## Task Breakdown

1. `Task 18K`: introduce shared layout API and legacy migration scaffold
2. `Task 18L`: migrate runtime-owned project/session/memory/document persistence to `.omega-state/`
3. `Task 18M`: preserve repo-local config under `.omega/` and split hook source vs artifact paths
4. `Task 18N`: sweep docs, tests, fixtures, ignore rules, and user-visible diagnostics

---

## Change Log

- 2026-04-13: Marked implemented after introducing `omega-project-layout`, migrating runtime state to `.omega-state/`, keeping config/source under `.omega/`, and splitting hook source vs artifact roots.
- 2026-04-11: Initial spec for splitting repo-local `.omega/` config/source assets from generated `.omega-state/` project state.
