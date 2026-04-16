---
content_revision: 101
created: 2026-03-23
generation_id: gen_000017_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 17
related_prds: []
source_doc_id: "spec:docs-specs-omega-client-anthropic-api-abstraction"
status: implemented
supersedes: []
updated: 2026-04-02
---

# Omega Client Anthropic API Abstraction Specification

## Overview

Status note (2026-04-02): 该抽象层和对应测试矩阵已经落地，当前文档保留为已实现基线与后续 provider 扩展入口。

`omega-client` 当前已经能以 Minimax 的 Anthropic-compatible endpoint 驱动主路径，但内部仍以 `MinimaxClient` 直接承载协议、传输、类型模型和 provider 细节。下一阶段应把 Anthropic Messages API 及其相关服务抽象为 `omega-client` 的稳定接口层，再让 Minimax 作为一个 Anthropic-compatible provider 挂接在该层之下。

该重构的目标不是引入 Go SDK，而是参考 Anthropic 官方 API 文档与 `anthropic-sdk-go` 的接口组织方式，为 Rust workspace 建立清晰、可扩展、可测试的本地抽象。

## Goals

- 将 `omega-client` 分为 provider-neutral 的 Anthropic API 抽象层和 provider-specific 的 Minimax 适配层。
- 为 Anthropic Messages family 建立完整、显式、类型化的请求/响应模型。
- 支持 Minimax 文档要求的 Anthropic-compatible tool use、thinking、prompt caching 与消息历史回传语义。
- 为后续扩展 `count_tokens`、`models`、`message batches`、beta headers 与更多 Anthropic-compatible provider 预留稳定边界。
- 建立独立、完备、可分层执行的测试体系，避免回归只能通过端到端人工验证发现。

## Non-Goals

- 本规格不要求本轮一次性实现全部 Anthropic 官方 beta 能力。
- 本规格不要求把 `omega-core`、`omega-session` 或 `omega-tui` 改写为直接依赖 Anthropic 风格对象。
- 本规格不要求强依赖第三方 Rust Anthropic SDK；仓库继续以自有 typed client 为主。
- 本规格不要求在无 provider 支持时伪造功能可用；能力不足应通过 capability 明确暴露。

## Architecture

### Components

- **Anthropic API Model Layer**: 定义 provider-neutral 的 request/response/block/header/capability types。
- **Anthropic Transport Layer**: 负责 HTTP 请求、header 注入、错误解码、SSE/stream 累积与 timeout/retry policy。
- **Anthropic Service Layer**: 暴露 `messages`、`count_tokens`、`models`、`message_batches` 等服务接口。
- **Provider Adapter Layer**: 将具体 provider 的 base URL、env、beta header、能力矩阵和兼容差异接入 Anthropic service layer。
- **Compatibility Layer**: 保留当前 `LlmClient` / `ChatRequest` / `ChatResponse` 主路径，作为更高层对 Anthropic `messages` 的窄适配。

### Proposed Module Layout

- `omega_client::anthropic::types`: Anthropic-neutral data models。
- `omega_client::anthropic::services`: service traits and request builders。
- `omega_client::anthropic::transport`: reqwest-based HTTP + SSE transport。
- `omega_client::anthropic::streaming`: event parser and accumulator。
- `omega_client::provider::minimax`: Minimax Anthropic-compatible provider config and capability declarations。
- `omega_client::compat`: current `LlmClient` bridge built on top of the Anthropic messages service。

### Dependency Direction

- `omega-core` / `omega-session` / `omega-subagent` -> `omega-client::compat`
- `omega-client::compat` -> `omega-client::anthropic::services`
- `omega-client::anthropic::services` -> `omega-client::anthropic::types` + `transport`
- `omega-client::provider::minimax` -> `omega-client::anthropic::*`

该方向保持 `omega-client` 为下游 crates 的唯一入口，同时避免把 Minimax 细节上卷到 agent loop 和 session orchestration。

## Data Flow

1. 上层仍通过 `LlmClient` 或后续更窄的 `MessagesClient` 发起请求。
2. compatibility layer 将现有 `ChatRequest` 映射为 Anthropic `messages.create` request。
3. provider adapter 注入 base URL、认证、版本 header、beta header 与 provider capability。
4. transport layer 执行请求，并在 streaming/non-streaming 两种模式下解析响应。
5. accumulator 将 SSE 事件累积为稳定的 typed response/event blocks。
6. compatibility layer 再将 Anthropic-neutral response 映射回当前 `ChatResponse` / `ChatEvent`。

## API Specification

### Core Provider Interface

#### `AnthropicProviderConfig`
- **Input**: provider identity, API key source, base URL, API version, default betas, timeout policy.
- **Output**: immutable provider config used by service clients.
- **Errors**: missing API key, invalid base URL, invalid header value.

#### `AnthropicProviderCapabilities`
- **Input**: none; declared by provider.
- **Output**: booleans/enums indicating support for tools, thinking, prompt caching, count tokens, batches, models, streaming, beta features.
- **Errors**: none.

### Service Interfaces

#### `MessagesService::create`
- **Input**: `AnthropicMessageCreateRequest`
- **Output**: `AnthropicMessage`
- **Errors**: transport error, API error, unsupported capability error, decode error.

#### `MessagesService::create_stream`
- **Input**: `AnthropicMessageCreateRequest`
- **Output**: stream of `AnthropicStreamEvent`
- **Errors**: transport error, SSE decode error, unsupported capability error.

#### `MessagesService::count_tokens`
- **Input**: `AnthropicCountTokensRequest`
- **Output**: `AnthropicTokenCount`
- **Errors**: unsupported capability error, transport/API/decode errors.

#### `ModelsService::list`
- **Input**: optional filters.
- **Output**: `Vec<AnthropicModelInfo>`
- **Errors**: unsupported capability error, transport/API/decode errors.

#### `MessageBatchesService::{create,get,list,results}`
- **Input**: batch params or batch id.
- **Output**: typed batch state/result models.
- **Errors**: unsupported capability error, transport/API/decode errors.

### Compatibility Interface

#### `LlmClient for AnthropicMessagesCompatClient`
- **Input**: current `ChatRequest`
- **Output**: current `ChatResponse` / `ChatEventStream`
- **Errors**: preserved `ClientError` surface with richer source classification.

该层需要保证当前 `omega-core` 主路径不因为 Anthropic 抽象下沉而被迫改签名。

## Data Models

### `AnthropicTextBlock`

| Field | Type | Description |
|-------|------|-------------|
| text | string | Text payload |
| cache_control | option | Optional prompt caching marker |
| citations | array | Optional citation metadata |

### `AnthropicToolDefinition`

| Field | Type | Description |
|-------|------|-------------|
| name | string | Tool name |
| description | string | Tool description |
| input_schema | json | JSON schema |
| cache_control | option | Optional cache marker for reusable tool definitions |
| strict | option<bool> | Optional strict schema validation hint |

### `AnthropicMessageCreateRequest`

| Field | Type | Description |
|-------|------|-------------|
| model | string | Target model id |
| max_tokens | u32 | Output token budget |
| messages | array | User/assistant turns with content blocks |
| system | array | Anthropic text blocks for system prompt |
| tools | array | Tool definitions |
| tool_choice | option | Auto/any/tool-specific choice |
| stream | bool | Streaming switch |
| metadata | option | Request metadata |
| stop_sequences | array | Optional stop sequences |
| temperature/top_p/top_k | option | Sampling controls |
| thinking | option | Thinking configuration when provider supports it |
| cache_control | option | Top-level cache marker when supported |
| betas | array | Requested beta flags |

### `AnthropicUsage`

| Field | Type | Description |
|-------|------|-------------|
| input_tokens | u32 | Non-cached input tokens |
| output_tokens | u32 | Output tokens |
| cache_creation_input_tokens | option<u32> | Tokens written into prompt cache |
| cache_read_input_tokens | option<u32> | Tokens read from prompt cache |

### `ProviderCapabilityError`

| Field | Type | Description |
|-------|------|-------------|
| provider | string | Provider name |
| operation | string | Unsupported operation |
| detail | string | Human-readable explanation |

## Full-Function Support Target

### GA Surface

- Messages create
- Messages streaming
- Message token counting
- Models listing
- Message batches create/list/get/results

### Messages Feature Surface

- Text blocks
- Thinking blocks and signatures
- Tool use / tool result blocks
- Structured `tool_choice`
- Citations in text blocks
- Metadata
- Output config / structured output hooks
- Prompt caching markers on tools/system/messages
- Top-level cache control where provider supports it

### Provider-Specific Compatibility Surface

- Minimax Anthropic-compatible base URL selection (`.com` / `.io`)
- Minimax prompt caching semantics
- Minimax tool-use message-history roundtrip requirements
- Minimax-compatible env fallback (`OMEGA_*`, `ANTHROPIC_*`)

### Deferred but Planned Surface

- Files API
- Skills API
- MCP-related beta features
- Container/context-management variants
- Server tool use blocks

这些能力应先在抽象层有类型与 capability 位置，再按 provider support matrix 决定何时落实现。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| Primary abstraction | Anthropic Messages-first service layer | 当前主路径和 Minimax 官方兼容面都围绕 Messages API |
| Backward compatibility | Keep `LlmClient` as compatibility facade | 避免把大范围改签名扩散到 `omega-core`/`omega-session` |
| Provider modeling | Explicit capability matrix | “全功能支持”不能靠隐式猜测；能力缺口必须可观察 |
| Transport | Continue using `reqwest` | 保持 Rust workspace 统一依赖与可控错误处理 |
| Streaming | Native SSE parser + accumulator | 参考 `anthropic-sdk-go` 的 accumulate 测试模型，保证 block-level correctness |
| Prompt caching | First-class typed fields, not raw JSON escape hatches | 缓存语义是 MiniMax 对接的核心，不应继续靠 `serde_json::Value` 临时拼装 |
| Env resolution | Support both `OMEGA_*` and Anthropic-standard env names | 按官方文档配置即可启动，减少集成摩擦 |

## Security Considerations

- API key resolution必须避免在日志中输出明文。
- Raw request/response tracing 默认只在 TRACE 级启用，并提供敏感字段掩码策略。
- 工具 schema、tool result 和 message content 的 JSON 反序列化必须做严格错误分类，防止把 provider 异常响应误判为业务成功。
- Beta headers 只能显式启用，避免默认打开未知实验能力。

## Performance Requirements

- Non-streaming create path 在现有 `reqwest` client 复用前提下不引入额外网络 hop。
- Streaming accumulator 必须是单次线性累积，不得在每个 delta 上重新解析完整内容。
- Request body building 对常见 messages/tool payload 保持 O(n) 序列化开销。
- 测试层需要覆盖 prompt cache usage 字段解析，便于后续优化真实成本。

## Testing Strategy

### Unit Tests

- Request builders serialize correct Anthropic JSON shapes.
- `system` string and block-array compatibility both serialize correctly.
- Tool definitions preserve `input_schema`, cache markers, and optional strict fields.
- Usage parsing covers cache hit/miss fields.
- Env resolution covers `OMEGA_*` and `ANTHROPIC_*` precedence.
- Capability gating returns deterministic `ProviderCapabilityError`.

### Streaming Tests

- SSE `message_start` / `content_block_start` / delta / stop / `message_stop` accumulate into the expected message.
- `thinking_delta`, `text_delta`, and `input_json_delta` merge correctly.
- Partial JSON for `tool_use.input` is accumulated without lossy intermediate parsing.
- Unknown or malformed stream events fail with typed decode errors.

### Provider Adapter Tests

- Minimax provider builds the correct `x-api-key`, `anthropic-version`, base URL, and optional beta headers.
- Minimax prompt caching requests serialize `cache_control` on tools/system/messages correctly.
- Minimax-compatible response payloads with thinking/tool_use/cache usage parse correctly.

### Mock HTTP Integration Tests

- Mock `/v1/messages` success response.
- Mock `/v1/messages` API error body classification.
- Mock `/v1/messages/count_tokens` contract.
- Mock `/v1/models` contract.
- Mock streaming SSE contract with interleaved thinking and tool use.

### Compatibility Tests

- `ChatRequest` -> Anthropic request mapping preserves tool use semantics.
- Anthropic response -> `ChatResponse` mapping preserves full assistant history blocks.
- Current `omega-core` loop tests continue to pass on top of the compatibility layer.

### Live Acceptance Tests

- Ignored tests gated by env for real Minimax Anthropic-compatible endpoint.
- One tool-use happy path.
- One prompt-caching path validating `cache_creation_input_tokens` then `cache_read_input_tokens`.
- One streaming path with thinking/tool_use.

### Test File Layout

- `crates/omega-client/tests/anthropic_messages_request_tests.rs`
- `crates/omega-client/tests/anthropic_messages_stream_tests.rs`
- `crates/omega-client/tests/anthropic_provider_minimax_tests.rs`
- `crates/omega-client/tests/anthropic_capability_tests.rs`
- `crates/omega-client/tests/anthropic_live_minimax_tests.rs`

## Implementation Plan

### Phase 1: Core Abstraction

- Extract Anthropic-neutral types from current `MinimaxClient` model layer.
- Introduce provider config and capability structs.
- Keep `LlmClient` compatibility intact.

### Phase 2: Messages Service Completion

- Support block-array `system`, cache markers, richer usage fields, and env fallback.
- Add explicit stream accumulator.
- Add mock-server coverage for create and stream.

### Phase 3: Service Expansion

- Add `count_tokens`, `models`, and `message_batches` service surfaces.
- Add unsupported-capability behavior for providers that do not implement them yet.

### Phase 4: Production Hardening

- Add live ignored acceptance tests.
- Add richer tracing fields and redaction.
- Verify all existing downstream crates remain source-compatible.

## Acceptance Criteria

- `omega-client` has a provider-neutral Anthropic API layer with Minimax adapter below it.
- Current `LlmClient` callers remain source-compatible.
- Prompt caching is expressible as typed request data, not ad hoc JSON mutation.
- Streaming accumulation is covered by deterministic tests modeled after Anthropic SDK behavior.
- Independent test suites cover request serialization, response parsing, streaming, provider config, capability gating, and live acceptance.
