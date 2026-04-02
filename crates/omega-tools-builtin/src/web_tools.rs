use anyhow::{Context, Result};
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use regex::Regex;
use reqwest::Client;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;

const DEFAULT_SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const MAX_FETCH_BYTES: usize = 1_000_000;
const DEFAULT_FETCH_SUMMARY_CHARS: usize = 1_600;
const DEFAULT_SEARCH_RESULTS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WebSearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, Clone)]
pub struct WebSearchHandler {
    client: Client,
    endpoint: String,
}

impl Default for WebSearchHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearchHandler {
    pub fn new() -> Self {
        Self::with_endpoint(DEFAULT_SEARCH_ENDPOINT)
    }

    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            client: build_client(),
            endpoint: endpoint.into(),
        }
    }
}

impl ToolHandler for WebSearchHandler {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the public web and return structured result candidates."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "description": "Maximum number of results to return.",
                    "default": 5
                }
            },
            "required": ["query"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.execute_v2(input)?.output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        let query = required_string(&input, "query")?;
        let max_results = input
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|value| value.clamp(1, 10) as usize)
            .unwrap_or(DEFAULT_SEARCH_RESULTS);
        let client = self.client.clone();
        let endpoint = self.endpoint.clone();

        run_async_http(move || async move {
            let response = client
                .get(&endpoint)
                .query(&[("q", query.as_str())])
                .send()
                .await;

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    return Ok(request_error_result(
                        "web_search",
                        error,
                        json!({ "query": query }),
                    ))
                }
            };

            let status = response.status();
            let body = match response.text().await {
                Ok(body) => body,
                Err(error) => {
                    return Ok(request_error_result(
                        "web_search",
                        error,
                        json!({ "query": query, "status": status.as_u16() }),
                    ))
                }
            };

            let results = parse_search_results(&body, max_results);
            let output = render_search_output(&query, &results);

            Ok(ToolResult::success(output)
                .with_preview(format!("{} results for '{}'", results.len(), query))
                .with_metadata(json!({
                    "query": query,
                    "result_count": results.len(),
                    "results": results.iter().map(|result| json!({
                        "title": result.title,
                        "url": result.url,
                        "snippet": result.snippet,
                    })).collect::<Vec<_>>(),
                })))
        })
    }
}

#[derive(Debug, Clone)]
pub struct WebFetchHandler {
    client: Client,
}

impl Default for WebFetchHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetchHandler {
    pub fn new() -> Self {
        Self {
            client: build_client(),
        }
    }
}

impl ToolHandler for WebFetchHandler {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a known URL and return a structured summary of its content."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "HTTP or HTTPS URL to fetch."
                },
                "max_chars": {
                    "type": "integer",
                    "minimum": 200,
                    "maximum": 8000,
                    "description": "Maximum summary size in characters.",
                    "default": 1600
                }
            },
            "required": ["url"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.execute_v2(input)?.output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        let url = required_string(&input, "url")?;
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult::error(
                "URL must start with http:// or https://",
                ToolErrorKind::Validation,
            )
            .with_metadata(json!({ "url": url })));
        }

        let max_chars = input
            .get("max_chars")
            .and_then(Value::as_u64)
            .map(|value| value.clamp(200, 8_000) as usize)
            .unwrap_or(DEFAULT_FETCH_SUMMARY_CHARS);

        let client = self.client.clone();
        run_async_http(move || async move {
            let response = client.get(&url).send().await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    return Ok(request_error_result(
                        "web_fetch",
                        error,
                        json!({ "url": url }),
                    ))
                }
            };

            let status = response.status();
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            if let Some(length) = response.content_length() {
                if length as usize > MAX_FETCH_BYTES {
                    return Ok(ToolResult::error(
                        format!("Response body exceeds {MAX_FETCH_BYTES} bytes"),
                        ToolErrorKind::Execution,
                    )
                    .with_metadata(json!({
                        "url": url,
                        "status": status.as_u16(),
                        "content_type": content_type,
                        "content_length": length,
                    })));
                }
            }

            let body = match response.text().await {
                Ok(body) => body,
                Err(error) => {
                    return Ok(request_error_result(
                        "web_fetch",
                        error,
                        json!({ "url": url, "status": status.as_u16(), "content_type": content_type }),
                    ))
                }
            };
            if body.len() > MAX_FETCH_BYTES {
                return Ok(ToolResult::error(
                    format!("Response body exceeds {MAX_FETCH_BYTES} bytes"),
                    ToolErrorKind::Execution,
                )
                .with_metadata(json!({
                    "url": url,
                    "status": status.as_u16(),
                    "content_type": content_type,
                    "content_length": body.len(),
                })));
            }

            let (title, summary, truncated) = summarize_response(&body, &content_type, max_chars);
            let output = render_fetch_output(&url, status.as_u16(), title.as_deref(), &summary);

            Ok(ToolResult::success(output)
                .with_optional_preview(title.clone())
                .with_truncated(truncated)
                .with_metadata(json!({
                    "url": url,
                    "status": status.as_u16(),
                    "content_type": content_type,
                    "title": title,
                    "summary": summary,
                })))
        })
    }
}

fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("omega/0.1 web-tools")
        .build()
        .expect("web tool client should build")
}

fn run_async_http<T, Fut, F>(operation: F) -> Result<T>
where
    T: Send + 'static,
    Fut: Future<Output = Result<T>> + Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("failed to build tokio runtime for web tool")
                .and_then(|runtime| runtime.block_on(operation()));
            let _ = sender.send(result);
        });
        receiver
            .recv()
            .context("web tool worker thread terminated before returning")?
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime for web tool")?
            .block_on(operation())
    }
}

fn required_string(input: &Value, field: &str) -> Result<String> {
    let value = input
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing required field '{field}'"))?;
    Ok(value.to_string())
}

fn request_error_result(tool_name: &str, error: reqwest::Error, metadata: Value) -> ToolResult {
    let error_kind = if error.is_timeout() {
        ToolErrorKind::Timeout
    } else {
        ToolErrorKind::Execution
    };
    ToolResult::error(
        format!("{tool_name} request failed: {error}"),
        error_kind,
    )
    .with_metadata(metadata)
}

fn parse_search_results(body: &str, max_results: usize) -> Vec<WebSearchResult> {
    let title_regex = Regex::new(
        r#"(?s)<a[^>]*class="result__a"[^>]*href="(?P<url>[^"]+)"[^>]*>(?P<title>.*?)</a>"#,
    )
    .expect("search title regex should compile");
    let snippet_regex = Regex::new(
        r#"(?s)(?:<a[^>]*class="result__snippet"[^>]*>(?P<snippet_a>.*?)</a>|<div[^>]*class="result__snippet"[^>]*>(?P<snippet_div>.*?)</div>)"#,
    )
    .expect("search snippet regex should compile");

    title_regex
        .captures_iter(body)
        .take(max_results)
        .map(|captures| {
            let match_end = captures.get(0).map(|value| value.end()).unwrap_or(0);
            let snippet_window_end = (match_end + 600).min(body.len());
            let snippet_window = &body[match_end..snippet_window_end];
            let title = collapse_whitespace(&strip_html(&decode_html_entities(
                captures.name("title").map(|value| value.as_str()).unwrap_or(""),
            )));
            let url = decode_html_entities(captures.name("url").map(|value| value.as_str()).unwrap_or(""));
            let snippet_capture = snippet_regex.captures(snippet_window);
            let snippet = collapse_whitespace(&strip_html(&decode_html_entities(
                snippet_capture
                    .as_ref()
                    .and_then(|captures| {
                        captures
                            .name("snippet_a")
                            .or_else(|| captures.name("snippet_div"))
                    })
                        .map(|value| value.as_str())
                        .unwrap_or(""),
                    )));
            WebSearchResult { title, url, snippet }
        })
        .filter(|result| !result.title.is_empty() && !result.url.is_empty())
        .collect()
}

fn render_search_output(query: &str, results: &[WebSearchResult]) -> String {
    if results.is_empty() {
        return format!("No web results found for '{query}'.");
    }

    let mut lines = vec![format!("Web results for '{query}':")];
    for (index, result) in results.iter().enumerate() {
        lines.push(format!("{}. {}", index + 1, result.title));
        lines.push(format!("   URL: {}", result.url));
        if !result.snippet.is_empty() {
            lines.push(format!("   Snippet: {}", result.snippet));
        }
    }
    lines.join("\n")
}

fn summarize_response(body: &str, content_type: &str, max_chars: usize) -> (Option<String>, String, bool) {
    if content_type.contains("html") || content_type.contains("xml") {
        let title = extract_title(body);
        let text = html_to_text(body);
        let (summary, truncated) = truncate_chars(&text, max_chars);
        (title, summary, truncated)
    } else {
        let (summary, truncated) = truncate_chars(&collapse_whitespace(body), max_chars);
        (None, summary, truncated)
    }
}

fn render_fetch_output(url: &str, status: u16, title: Option<&str>, summary: &str) -> String {
    let mut lines = vec![format!("URL: {url}"), format!("Status: {status}")];
    if let Some(title) = title {
        if !title.is_empty() {
            lines.push(format!("Title: {title}"));
        }
    }
    lines.push("Summary:".to_string());
    lines.extend(summary.lines().map(str::to_string));
    lines.join("\n")
}

fn extract_title(body: &str) -> Option<String> {
    let regex = Regex::new(r#"(?is)<title[^>]*>(?P<title>.*?)</title>"#).ok()?;
    let captures = regex.captures(body)?;
    let title = decode_html_entities(&strip_html(captures.name("title")?.as_str()));
    (!title.is_empty()).then_some(title)
}

fn html_to_text(body: &str) -> String {
    let script_re = Regex::new(r#"(?is)<script[^>]*>.*?</script>"#).expect("script regex");
    let style_re = Regex::new(r#"(?is)<style[^>]*>.*?</style>"#).expect("style regex");
    let body = script_re.replace_all(body, " ");
    let body = style_re.replace_all(&body, " ");
    collapse_whitespace(&decode_html_entities(&strip_html(&body)))
}

fn strip_html(text: &str) -> String {
    let tag_re = Regex::new(r#"(?is)<[^>]+>"#).expect("tag regex");
    tag_re.replace_all(text, " ").to_string()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    let count = text.chars().count();
    if count <= max_chars {
        return (text.to_string(), false);
    }

    let truncated = text.chars().take(max_chars).collect::<String>();
    (format!("{}...", truncated.trim_end()), true)
}

#[cfg(test)]
mod tests {
    use super::{extract_title, html_to_text, parse_search_results, WebFetchHandler};
    use super::WebSearchHandler;
    use omega_tools::ToolHandler;
    use serde_json::json;

    #[test]
    fn parse_search_results_extracts_title_url_and_snippet() {
        let html = r#"
            <div class="result">
              <a class="result__a" href="https://example.com/doc">Example &amp; Guide</a>
              <div class="result__snippet">Read the &lt;strong&gt;full&lt;/strong&gt; guide.</div>
            </div>
        "#;

        let results = parse_search_results(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example & Guide");
        assert_eq!(results[0].url, "https://example.com/doc");
        assert_eq!(results[0].snippet, "Read the full guide.");
    }

    #[test]
    fn html_summary_extracts_title_and_collapses_text() {
        let html = r#"
            <html>
              <head><title>Alpha</title></head>
              <body><main>Hello <b>world</b>.</main><script>ignored()</script></body>
            </html>
        "#;

        assert_eq!(extract_title(html).as_deref(), Some("Alpha"));
        assert_eq!(html_to_text(html), "Alpha Hello world .");
    }

    #[test]
    fn web_fetch_rejects_non_http_urls() {
        let result = WebFetchHandler::new()
            .execute_v2(json!({"url": "file:///tmp/test"}))
            .unwrap();

        assert_eq!(result.error_kind, Some(omega_tools::ToolErrorKind::Validation));
    }

    #[test]
    fn web_handlers_can_be_constructed_inside_tokio_runtime() {
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime.block_on(async {
            let search = WebSearchHandler::new();
            let fetch = WebFetchHandler::new();

            drop(search);
            drop(fetch);
        });
    }
}