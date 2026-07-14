---
content_revision: 174
created: 2026-06-03
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: 152deb1818837dc5c3e7575c7010dc965eef2c38
owner: architect agent
projection_version: 87
source_doc_id: "spec:docs-specs-omega-documentation-system-audit"
source_path: docs/specs/omega-documentation-system-audit.md
status: draft
updated: 2026-06-03
---

# Spec: Omega Documentation System Audit

## Overview

> **Related**: ADR-007 (omega-hpc extraction), ADR-008 (TUI component architecture)
> **Status**: draft
> **Output format**: card layout — every finding and every task is a bordered
> 4-line card, so a reader can scan the page top-to-bottom without losing
> their place. See "Format" section below.

---

## Format

Every block in this spec is a card. A card has:

```
┌─ <KIND> <ID> / <topic> ────────── <status> ─┐
│ 证据: ...                                   │
│ 影响: ...                                   │
│ 建议: ...                                   │
└────────────────────────────────────────────┘
```

- **KIND** is one of `Finding` or `Task` or `Phase` or `Stat`.
- **ID** is a stable slug (`F-01`, `T-01`, etc.) so the spec can be cross-
  referenced from TODO.md, ADR-009, and downstream CI jobs.
- **status** is a bracketed text badge (`[critical]` / `[major]` /
  `[minor]` / `[done]` / `[todo]` / `[wip]`) — no emoji, matching the
  TODO.md style established in Task 39.
- Card bodies are 3 short lines: 证据 (evidence) / 影响 (impact) /
  建议 (recommendation). A reader who only reads the headers can
  already triage the spec.

Cards are separated by `───` so rendered markdown shows clear
visual blocks. Status badges double as progress tracking.

---

## Phase Strip

```
[phase: explore]     22 raw observations, no scoring      done
[phase: architect]   5 维 × 10 评分, 4/9 红旗命中           done
[phase: plan]        6 任务卡片, blocked-by 链明确        done
[phase: implement]   spec 已写, TODO 已 flip              done
```

---

## Section 1 — Baseline Numbers

```
[stat]
  records:         70 across 7 record sets
  specs:           49 (74% of all records)
  decisions:       9
  prds:            3
  guides:          2
  whitepapers:     1
  archive:         4
  relations/:      0 files (declared path, never written)
  frontmatter:     50/66 markdown (75%) carry last_verified_commit: N/A
  version null:    61/70 records (87%) have version: null
  validation:      render-state.json's last_validation_ok = null
  archived_date:   0/4 archive records carry an archive date
  change log:      8/49 specs (16%) lack a Change Log section
[/stat]
```

---

## Section 2 — Findings

```
┌─ Finding F-01 / 文档-代码同步验证缺失 ──────────── [major] ─┐
│ 证据: 50/66 (75%) frontmatter last_verified_commit=N/A;│
│       render-state.json 的 last_validation_ok 字段从  │
│       未填充过 (始终为 null)                           │
│ 影响: 无法判断 spec 描述与代码现状的差距;读者以为   │
│       描述对,实际可能 stale 多年                       │
│ 建议: 引入 CI 钩子 (T-01):spec 引用的源文件 mtime >  │
│       frontmatter.last_verified_commit → 红色;       │
│       last_verified_commit=N/A → 红色                  │
└────────────────────────────────────────────────────────┘
```

```
┌─ Finding F-02 / docs-data/relations/ 空目录 ────────── [major] ─┐
│ 证据: relations/ 下 0 文件,但 manifest.json 声明了    │
│       relation_store_path = docs-data/relations/links  │
│ 影响: 跨文档引用图不可重建;移动/归档 spec 时缺引用   │
│       关系提示,容易引入 dangling link                  │
│ 建议: 重建 relations/ (T-03):从 markdown 反向扫描所有 │
│       链接建立 edges.jsonl;引入 record relation upsert │
└────────────────────────────────────────────────────────┘
```

```
┌─ Finding F-03 / version 字段 87% 为 null ─────────── [major] ─┐
│ 证据: 61/70 records 的 version 字段为 null,即便      │
│       OmegaProjectLayout 声明了 version 必填           │
│ 影响: 文档演进没有 version 锚点,CHANGELOG 无法引用    │
│       文档版本号;render manifest 的 content_revision   │
│       是全局计数,不能定位"哪篇文档改到了第几版"       │
│ 建议: 一次性补齐 (T-02):每个 record 的 version 初始化 │
│       为 "v1.0" 或 "draft-1",按 commit 时间定 baseline │
└────────────────────────────────────────────────────────┘
```

```
┌─ Finding F-04 / TODO.md 承担三重职责 ─────────────── [major] ─┐
│ 证据: TODO.md 同时承担 open-work / current-baseline / │
│       historical 记录;"Current Baseline" 8 条每条都   │
│       复述 spec Change Log + ADR already-links 的内容  │
│ 影响: 同一事实维护 2-3 份;读者交叉校验 2-3 个文件才   │
│       能确认一个 milestone                              │
│ 建议: TODO.md 只保留 open-work + 指针;current-baseline│
│       改为从 spec/ADR 自动聚合 (T-05);historical 走    │
│       docs/archive + git history                        │
└────────────────────────────────────────────────────────┘
```

```
┌─ Finding F-05 / Spec 章节顺序不统一 ──────────────── [minor] ─┐
│ 证据: 49 specs 中:有的是 Overview→Goals→Architecture;  │
│       有的是 Goals→Non-Goals→Data-Model;有的是 Goal→  │
│       Scope→Sub-Workspace-Layout;每篇都要重新定向    │
│ 影响: 读者无法用固定节奏扫读;写新 spec 时无模板      │
│ 建议: 引入 Spec 章节模板 (T-06):7 节固定顺序(Overview│
│       / Goals / Non-Goals / Architecture / Data-Model  │
│       / Testing / Change Log),加缺失章节 = lint error │
└────────────────────────────────────────────────────────┘
```

```
┌─ Finding F-06 / language 混用无规范 ───────────────── [minor] ─┐
│ 证据: 49 specs 中 ~60% 中英混排(Chinese body, English  │
│       headings/code); 8 decisions 纯中文; dev-guide   │
│       纯英文;无 language 字段                          │
│ 影响: 国际化/翻译/搜索索引都会受影响;grep 跨语言失效  │
│ 建议: T-06 一并引入 language 字段(spec.zh-CN / en /    │
│       bilingual),frontmatter 强制声明,bilingual spec  │
│       的 body 与 heading 语言可不同但要声明             │
└────────────────────────────────────────────────────────┘
```

```
┌─ Finding F-07 / archive 元数据不完整 ────────────── [minor] ─┐
│ 证据: 4 archive records 状态混用 (3 archived + 1      │
│       superseded);0/4 携带 archived_date 字段;front-  │
│       matter 没有标准的 archive metadata 模板           │
│ 影响: 归档时间线不可恢复;读者不知道一个 spec 为什么   │
│       被 archived 以及被谁决定                        │
│ 建议: 引入 Archive Template (T-06 旁路):archived_date  │
│       + archived_by + replaced_by (如 applicable),     │
│       缺失 = 不可 archive                                │
└────────────────────────────────────────────────────────┘
```

```
┌─ Finding F-08 / decisions/README.md 跟 record 重复 ─── [minor] ─┐
│ 证据: decisions/README.md 手维护 7 行 table;            │
│       decisions.jsonl 已有 8 条 record; 008 行没在      │
│       table 里 (因为手维护错过一次更新)                │
│ 影响: 读者不知道哪个是真的;record 与 table 漂移      │
│ 建议: T-04 让 decisions/README.md 单一源 = 渲染 record;│
│       hand-maintenance 改为 read-only;新 ADR 通过 record│
│       upsert 路径自动出现在 index                       │
└────────────────────────────────────────────────────────┘
```

---

## Section 3 — Tasks

```
┌─ Task T-01 / 文档-代码同步 CI ──────────────────────── [todo] ─┐
│ 依赖: 无                                                │
│ 验证: 故意改一个 spec 引用的源文件 → CI 红色;          │
│       故意让 frontmatter.last_verified_commit 过期     │
│       → CI 红色;绿色基线 (所有 N/A 都已填) → CI 绿色  │
│ 阻塞: T-04, T-05                                       │
│ 范围: .github/workflows/doc-sync.yml (新) + 1 个    │
│       shell 脚本 ~50 行                                │
└────────────────────────────────────────────────────────┘
```

```
┌─ Task T-02 / 补齐 frontmatter 缺失字段 ──────────── [todo] ─┐
│ 依赖: 无 (可与 T-01 并行)                              │
│ 验证: 70/70 records 有 version != null;50/50 markdown  │
│       有 last_verified_commit != N/A                    │
│ 范围: 一次性脚本读 frontmatter + 写回;reviewer         │
│       抽查 10 篇                                       │
└────────────────────────────────────────────────────────┘
```

```
┌─ Task T-03 / relations/ edges 重建 ──────────────── [todo] ─┐
│ 依赖: 无 (可与 T-01/T-02 并行)                         │
│ 验证: docs-data/relations/edges.jsonl 存在且非空;     │
│       所有 archive/relink 操作能查到 dependency graph  │
│ 范围: 1 个新命令 `omega-doc relation build`;从        │
│       markdown 反向扫描所有 `[text](path)` 链接       │
└────────────────────────────────────────────────────────┘
```

```
┌─ Task T-04 / decisions/README.md 单一源化 ────────── [wip] ─┐
│ 依赖: T-01 (last_verified_commit 需要可信)             │
│ 验证: hand-edit README.md → 消失 (无 write 权限);    │
│       record upsert 008 → README 自动出现;            │
│       record archive 001 → README 标记 archived       │
│ 范围: 1 个 build 脚本读取 decisions.jsonl 渲染 README;│
│       删掉手维护的 table;AGENTS.md 加 "do not         │
│       hand-edit decisions/README.md" 规则              │
└────────────────────────────────────────────────────────┘
```

```
┌─ Task T-05 / TODO baseline 自动聚合 ─────────────── [todo] ─┐
│ 依赖: T-04 (decisions 单一源) + T-01 (commit 可信)     │
│ 验证: 给一个新 spec 写"related ADR-007" → TODO.md 的  │
│       baseline 自动多一行;人为编辑 baseline 段 → 消失  │
│ 范围: 1 个 build 脚本读 decisions+specs 拼 baseline; │
│       TODO.md 顶部加 generation 标记                   │
└────────────────────────────────────────────────────────┘
```

```
┌─ Task T-06 / Spec 章节模板 + 语言规范 ────────────── [todo] ─┐
│ 依赖: 无 (可独立启动)                                  │
│ 验证: 故意省略 Overview 节的 PR → CI 红色;            │
│       frontmatter 缺 language 字段 → CI 红色;        │
│       现有 49 specs 通过 lint = 0 errors                │
│ 范围: docs-general skill 扩写 Spec 模板;              │
│       CI 加 spec-lint step;一次性补齐 49 specs         │
└────────────────────────────────────────────────────────┘
```

---

## Section 4 — Open Questions

```
[Q-01] relations/ 的图查询接口是 CLI 子命令,还是 on-demand 解析?
      → 默认:CLI 子命令 (`omega-doc relation build`),与现有
        `omega-doc render / extract / archive` 模式一致

[Q-02] T-04/T-05 的 "render 单一源" 是覆盖 README.md / TODO.md
       的 entire body,还是只覆盖 body、保留 hand-maintained
       prose introduction?
      → 建议:整 body 渲染,prose 移到 spec 草稿
        `docs/specs/omega-documentation-system-rendering.md`

[Q-03] T-06 的章节模板是否覆盖所有 doc_type (spec / decision /
       guide / prd / whitepaper),还是只 spec?
      → 建议:所有 doc_type 各一份模板,挂在
        docs/specs/ 下,被 docs-general skill 引用

[Q-04] archive 模板 (F-07) 的 archived_by 字段填个人邮箱
       还是 role 名?
      → 建议:role 名 (e.g. "omega-team"),邮箱 PII 不进 git
```

---

## Related

- ADR-007: `docs/decisions/007-omega-hpc-extraction.md` (precedent: 物理抽出 + API 不变)
- ADR-008: `docs/decisions/008-tui-component-architecture-refactor.md` (precedent: 卡片状 UI + 视觉回归测试)
- Task 39-39I: TUI 组件架构重构 (卡片状 + 状态条样式的源头)
- Task 38-38I: omega-hpc extraction (record + 渲染分层架构的源头)
- AGENTS.md: "Archive Rules", "Documentation" section (会被 T-04/T-05/T-06 更新)
