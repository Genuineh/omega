---
content_revision: 101
created: 2026-04-14
generation_id: gen_000017_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 17
related_prds:
  - docs/specs/omega-structured-document-system.md
  - docs/specs/omega-document-cli.md
  - docs/specs/omega-project-plan-docs-data-convergence.md
source_doc_id: "spec:docs-specs-omega-document-projection-versioning"
status: active
supersedes: []
updated: 2026-04-15
---

# Omega Document Projection Versioning Specification

## Overview

在 structured docs v2 下，`docs-data/` 已经是 canonical source，而 `docs/` 只是 generated presentation。下一步如果要把 docs mutation 收口到 `omega-document-cli`，系统还必须能稳定回答一个问题：**当前这份 generated docs，究竟对应的是哪一版 docs-data？**

没有这层 version contract，会持续出现三类模糊区：

1. 用户无法确认当前看到的 `docs/*.md` 是否来自最新的 docs-data mutation。
2. `render` 与 `validate` 只能回答“现在像不像”，却无法回答“是不是同一代 projection”。
3. CLI、skills、`AGENTS.md` 即使都要求 mutation 走 CLI，也缺少一个可核验的 revision identity 来支撑 cutover。

本规格定义 docs-data 与 generated docs 的 projection version contract。

## Implementation Status

截至 `2026-04-15`，`Task 19J` 已完成并成为当前基线：

- `docs-data/manifest.json` 已记录 `content_revision`、`projection_version` 与 `last_generation_id`。
- `docs-data/render/render-state.json` 已记录 `generation_id`、`content_revision`、`projection_version` 与 rendered doc linkage。
- generated `docs/*.md` 已写入 `source_doc_id`、`content_revision`、`projection_version` 与 `generation_id` metadata。
- `validate_projection` 已把 content drift 与 version drift 分开报告。
- `omega-doc doctor`、`get`、`render`、`validate` 已消费并显示这套 version information。

## Goals

- 为 docs-data mutation、render generation 和 generated docs frontmatter 建立稳定的版本对应关系。
- 让 `omega-document-cli render|validate|get` 可以显式展示并校验这种对应关系。
- 让 generated docs 不只是“看起来同步”，而是带有可验证的来源 revision。
- 为后续 `/plan` 收口到 docs-data 后的统一 project knowledge versioning 预留一致方向。

## Non-Goals

- 不把 git commit hash 作为唯一版本来源。
- 不要求每篇文档都独立做 semver 发布。
- 不要求 Phase 1 提供跨多个 branch 的 merge-aware version graph。

## Version Model

至少需要三层 identity：

1. **schema version**
   - 描述 docs-data layout/schema 版本。
   - 用于回答“这个仓库的 docs-data 结构是什么版本”。

2. **content revision**
   - 每次 canonical docs-data mutation 后递增。
   - 用于回答“当前 docs-data 内容是第几版”。

3. **projection generation**
   - 每次成功 render 后生成新的 generation id / projection revision。
   - 用于回答“当前 docs/ 是由哪次 render 产出的”。

## Data Contract

### Manifest

`docs-data/manifest.json` 应扩展以下字段：

```json
{
  "schema_version": 1,
  "content_revision": 42,
  "projection_version": 3,
  "last_generation_id": "gen_20260414_001",
  "generated_root": "docs"
}
```

### Render State

`docs-data/render/render-state.json` 应至少记录：

- `generation_id`
- `content_revision`
- `projection_version`
- `rendered_doc_ids`
- `rendered_at`

### Generated Docs Metadata

generated `docs/*.md` frontmatter 或稳定 header metadata 应可表达：

- `source_doc_id`
- `content_revision`
- `projection_version`
- `generation_id`

目的是让人类和工具都能回答：

- 这篇 docs 来自哪个 canonical record
- 它是 docs-data 的第几版内容
- 它属于哪一次 render generation

## CLI Responsibilities

`omega-document-cli` 当前使用这套版本合同：

- `render` 在成功后推进 generation metadata。
- `validate` 校验 generated docs 与当前 manifest/render-state 的版本是否一致。
- `get <id>` 或 detail output 会显示 record 的 canonical version 和最近一次 projection version。
- `doctor` 会检查 version fields 缺失、漂移或 generation 失配。

## Rollout Status

### Task 19J-1: Completed Manifest Revision Fields

- `docs-data/manifest.json` 已增加 content/projection revision 字段。

### Task 19J-2: Completed Render State Generation Identity

- `render-state.json` 已增加 generation id 与 revision linkage。

### Task 19J-3: Completed Generated Docs Metadata

- generated `docs/` 已带有可读且稳定的来源/version metadata。

### Task 19J-4: Completed CLI Validation And Display

- `omega-doc render|validate|get|doctor` 已消费并展示版本信息。

## Acceptance Standards

- 对任意 generated docs，都能回答其 `source_doc_id`、`content_revision` 与 `generation_id`。
- `validate` 能区分“内容不一致”和“version/generation 漂移”。
- 版本字段缺失时，CLI 和 health tooling 能给出明确诊断。

## Related Specs

- `docs/specs/omega-document-cli.md`
- `docs/specs/omega-structured-document-system.md`
- `docs/specs/omega-project-plan-docs-data-convergence.md`

---

### Change Log

- 2026-04-15: v0.2 — `Task 19J` completed: manifest/render-state/generated docs now share a concrete revision contract, and `omega-doc render|validate|get|doctor` all consume that linkage.
- 2026-04-14: v0.1 — 初版规格，定义 docs-data 与 generated docs 的 projection version contract，作为 CLI-first docs workflow cutover 的前置条件。
