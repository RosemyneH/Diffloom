use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::Deserialize;

const DEFAULT_URL: &str = "http://127.0.0.1:11434/api/generate";
const DEFAULT_MODEL: &str = "llama3.2";
const MAX_DIFF_CHARS: usize = 14_000;

#[derive(Clone)]
pub struct LlmReviewConfig {
    pub url: String,
    pub model: String,
}

pub fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn is_enabled() -> bool {
    let disabled = std::env::var("DIFFLOOM_LLM_DISABLE")
        .map(|v| v == "1")
        .unwrap_or(false);
    !disabled
        && (std::env::var("DIFFLOOM_LLM")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
            || std::env::var("DIFFLOOM_LLM_URL").is_ok())
}

pub fn should_scan_after_ingest() -> bool {
    is_enabled()
        && std::env::var("DIFFLOOM_LLM_AFTER_INGEST")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

pub fn load_config() -> LlmReviewConfig {
    LlmReviewConfig {
        url: std::env::var("DIFFLOOM_LLM_URL").unwrap_or_else(|_| DEFAULT_URL.to_string()),
        model: std::env::var("DIFFLOOM_LLM_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string()),
    }
}

pub fn build_review_prompt(path: &str, unified_diff: &str) -> String {
    let diff: String = unified_diff.chars().take(MAX_DIFF_CHARS).collect();
    let tail = if unified_diff.chars().count() > MAX_DIFF_CHARS {
        "\n\n(truncated diff for prompt size)\n"
    } else {
        ""
    };
    format!(
        r#"You are a senior reviewer. Inspect ONLY the unified diff below (not the rest of the repo).

Look for: correctness bugs, panics/unwrap risks, logic errors, API misuse, data races or unsound patterns (if Rust), performance regressions (allocations, hot loops, accidental O(n²)), security issues (injection, secrets), and test gaps implied by the change.

Rules:
- Tie each point to what changed; skip generic advice.
- If the change looks low-risk, answer exactly one line: "No significant issues spotted."
- Use short bullet lines; no preamble.

File: {path}

Unified diff:
{diff}{tail}"#
    )
}

#[derive(Deserialize)]
struct OllamaGenerateResponse {
    #[serde(default)]
    response: String,
}

pub fn run_review(cfg: &LlmReviewConfig, path: &str, unified_diff: &str) -> anyhow::Result<String> {
    let prompt = build_review_prompt(path, unified_diff);
    let payload = serde_json::json!({
        "model": cfg.model,
        "prompt": prompt,
        "stream": false,
    });
    let resp = ureq::post(&cfg.url)
        .timeout(std::time::Duration::from_secs(180))
        .send_json(payload)
        .with_context(|| format!("POST {}", cfg.url))?;
    let status = resp.status();
    let text = resp.into_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        anyhow::bail!("HTTP {status}: {text}");
    }
    let parsed: OllamaGenerateResponse = serde_json::from_str(&text)
        .with_context(|| format!("bad JSON: {}", text.chars().take(200).collect::<String>()))?;
    let out = parsed.response.trim().to_string();
    if out.is_empty() {
        anyhow::bail!("empty model response");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_truncates_long_diff() {
        let huge = "x".repeat(MAX_DIFF_CHARS + 500);
        let p = build_review_prompt("a.rs", &huge);
        assert!(p.len() < huge.len() + 500);
        assert!(p.contains("truncated"));
    }
}
