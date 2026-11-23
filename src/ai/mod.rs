//! Multi-provider AI engine (Official OpenAI Sign in with ChatGPT + Official
//! Google Gemini API), `omp` skills registry, and inline diffing.

pub mod chatgpt;
pub mod diff_engine;
pub mod gemini;
pub mod provider;
pub mod skill_registry;

/// Returns `true` when an AI error message indicates that the provider's API
/// rate limit, quota, or subscription usage limit has been reached.
pub fn is_ai_limit_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("api limit reached")
        || lower.contains("usage limit reached")
        || lower.contains("rate limit")
        || lower.contains("quota exceeded")
        || lower.contains("resource_exhausted")
        || lower.contains("insufficient_quota")
        || lower.contains("too many requests")
        || lower.contains("http 429")
        || lower.contains("(429")
        || lower.contains("limit has been reached")
}
