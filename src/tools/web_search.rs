use std::error::Error;

use async_trait::async_trait;
use serde_json::json;

use super::web_html::extract_ddg_results;
use super::{schema_object, Tool, ToolResult};
use crate::claude::ToolDefinition;

pub struct WebSearchTool {
    /// When set, use this SearXNG instance instead of DuckDuckGo (more reliable when DDG blocks or returns no results).
    pub searxng_url: Option<String>,
}

impl WebSearchTool {
    pub fn new(searxng_url: Option<String>) -> Self {
        Self { searxng_url }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn definition(&self) -> ToolDefinition {
        let backend = if self.searxng_url.is_some() {
            "SearXNG"
        } else {
            "DuckDuckGo"
        };
        let mut schema = json!({
            "query": {
                "type": "string",
                "description": "The search query. With SearXNG you can use !category or !engine (e.g. !science, !files, !wp) and syntax like filetype:pdf, site:example.com."
            }
        });
        if self.searxng_url.is_some() {
            schema["categories"] = json!({
                "type": "string",
                "description": "SearXNG categories to search: general, science, files, images, it, news, map, music, social, videos, etc. Comma-separated. Overrides default."
            });
            schema["engines"] = json!({
                "type": "string",
                "description": "SearXNG engines to use (e.g. google, semantic_scholar, libgen). Comma-separated. Optional."
            });
            schema["time_range"] = json!({
                "type": "string",
                "description": "Limit results to time range: day, week, month, year. Optional."
            });
        }
        ToolDefinition {
            name: "web_search".into(),
            description: format!(
                "Search the web using {}. Returns titles, URLs, and snippets. Set SEARXNG_URL in .env to use a SearXNG instance (supports categories, engines, time_range).",
                backend
            ),
            input_schema: schema_object(
                schema,
                &["query"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let query = match input.get("query").and_then(|v| v.as_str()) {
            Some(q) => q,
            None => return ToolResult::error("Missing required parameter: query".into()),
        };

        let result = if let Some(ref base) = self.searxng_url {
            let categories = input
                .get("categories")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty());
            let engines = input
                .get("engines")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty());
            let time_range = input
                .get("time_range")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty());
            search_searxng(base, query, categories, engines, time_range).await
        } else {
            search_ddg(query).await
        };

        match result {
            Ok(results) => {
                if results.is_empty() {
                    let msg = if self.searxng_url.is_some() {
                        "No results found.".into()
                    } else {
                        "No results found. DuckDuckGo may be blocking or returning empty results. For reliable search, set SEARXNG_URL in .env to a SearXNG instance (e.g. your own or a public instance).".into()
                    };
                    ToolResult::success(msg)
                } else {
                    ToolResult::success(results)
                }
            }
            Err(e) => ToolResult::error(format!("Search failed: {e}")),
        }
    }
}

fn build_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| {
            let msg = e.to_string();
            let detail = e
                .source()
                .map(|s: &dyn Error| s.to_string())
                .unwrap_or_else(String::new);
            if detail.is_empty() {
                msg
            } else {
                format!("{msg}: {detail}")
            }
        })
}

async fn search_ddg(query: &str) -> Result<String, String> {
    // DuckDuckGo blocks GET/bot requests (403). Use POST with form data and browser-like
    // headers as used by no-JS clients (see SearXNG DDG engine docs).
    let url = "https://html.duckduckgo.com/html/";

    let client = build_http_client()?;

    let form = [("q", query), ("b", ""), ("kl", "wt-wt")];

    let resp = client
        .post(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        )
        .header("Referer", "https://html.duckduckgo.com/")
        .header("Sec-Fetch-Mode", "navigate")
        .header("Sec-Fetch-Dest", "document")
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&form)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!(
            "DuckDuckGo returned HTTP {} (bot detection may be blocking; try again later)",
            resp.status()
        ));
    }

    let body = resp.text().await.map_err(|e| e.to_string())?;
    let items = extract_ddg_results(&body, 8);

    let mut output = String::new();
    for (i, item) in items.iter().enumerate() {
        output.push_str(&format!(
            "{}. {}\n   {}\n   {}\n\n",
            i + 1,
            item.title,
            item.url,
            item.snippet
        ));
    }

    Ok(output)
}

async fn search_searxng(
    base_url: &str,
    query: &str,
    categories: Option<&str>,
    engines: Option<&str>,
    time_range: Option<&str>,
) -> Result<String, String> {
    let base = base_url.trim_end_matches('/');
    let encoded = urlencoding::encode(query);
    let mut url = format!("{base}/search?q={encoded}&format=json");
    if let Some(c) = categories {
        url.push_str("&categories=");
        url.push_str(&urlencoding::encode(c));
    }
    if let Some(e) = engines {
        url.push_str("&engines=");
        url.push_str(&urlencoding::encode(e));
    }
    if let Some(tr) = time_range {
        let tr_lower = tr.trim().to_ascii_lowercase();
        let tr_val = match tr_lower.as_str() {
            "day" | "week" | "month" | "year" => tr_lower,
            _ => "month".to_string(),
        };
        url.push_str("&time_range=");
        url.push_str(&urlencoding::encode(&tr_val));
    }

    let client = build_http_client()?;

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let msg = if status.as_u16() == 429 {
            "SearXNG instance rate limited (429 Too Many Requests). Use another instance (SEARXNG_URL), self-host, or try again later."
        } else {
            "SearXNG returned HTTP "
        };
        return Err(format!("{}{}.", msg, status));
    }

    let body = resp.text().await.map_err(|e| e.to_string())?;
    let data: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let results: &[serde_json::Value] = data
        .get("results")
        .and_then(|v| v.as_array())
        .map(|v| v.as_slice())
        .unwrap_or(&[]);

    let mut output = String::new();
    for (i, item) in results.iter().take(8).enumerate() {
        let title = item
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let url = item
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let content = item
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if url.is_empty() && title.is_empty() {
            continue;
        }
        output.push_str(&format!(
            "{}. {}\n   {}\n   {}\n\n",
            i + 1,
            title,
            url,
            content
        ));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_web_search_definition() {
        let tool = WebSearchTool::new(None);
        assert_eq!(tool.name(), "web_search");
        let def = tool.definition();
        assert_eq!(def.name, "web_search");
        assert!(def.description.contains("DuckDuckGo"));
        assert!(def.input_schema["properties"]["query"].is_object());
        let required = def.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "query"));
    }

    #[tokio::test]
    async fn test_web_search_missing_query() {
        let tool = WebSearchTool::new(None);
        let result = tool.execute(json!({})).await;
        assert!(result.is_error);
        assert!(result.content.contains("Missing required parameter: query"));
    }

    #[tokio::test]
    async fn test_web_search_null_query() {
        let tool = WebSearchTool::new(None);
        let result = tool.execute(json!({"query": null})).await;
        assert!(result.is_error);
        assert!(result.content.contains("Missing required parameter: query"));
    }
}
