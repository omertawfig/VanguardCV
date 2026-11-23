//! Official Google Gemini REST API client (`generativelanguage.googleapis.com/v1beta`).
//!
//! Uses a user-supplied Gemini API key (`x-goog-api-key` header) to:
//! - Discover available models via `GET /models`
//! - Stream CV tailoring and inline bullet rewrites via
//!   `POST /models/{model}:streamGenerateContent?alt=sse` using Gemini
//!   `functionDeclarations`
//!
//! Zero-trust invariants:
//! - Verifies the [`crate::contracts::ScrubbedPayloadToken`] FNV-1a digest
//!   before transmitting any bytes.
//! - Rejects any AI proposal containing `[REDACTED_PII_...]` via
//!   [`crate::ai::chatgpt::parse_tool_call`].

use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use url::Url;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai::chatgpt::{
    ToolCallResult, build_chatgpt_http_client, compute_fnv1a_digest, get_tool_declarations,
    parse_tool_call, synthesize_tailoring_summary,
};
use crate::contracts::{
    AiConfig, AiEvent, AiEventSink, AiOrchestrator, AiProposal, AtsScore, DEFAULT_GEMINI_MODEL,
    GEMINI_API_BASE_URL, InlineRewriteRequest, ModelInfo, ScrubbedPayload, SkillRegistry,
    VariationOutcome, VariationRequest,
};

/// Official Google Gemini REST API client.
pub struct GeminiClient {
    config: RwLock<AiConfig>,
    http: RwLock<Client>,
    skills: Option<Arc<dyn SkillRegistry>>,
}

impl GeminiClient {
    pub fn new(config: AiConfig) -> Self {
        let http = build_chatgpt_http_client(config.request_timeout_secs)
            .unwrap_or_else(|_| Client::new());
        Self {
            config: RwLock::new(config),
            http: RwLock::new(http),
            skills: None,
        }
    }

    fn http_client(&self) -> Client {
        self.http
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|_| Client::new())
    }

    pub fn with_skills(mut self, skills: Arc<dyn SkillRegistry>) -> Self {
        self.skills = Some(skills);
        self
    }

    fn verify_payload_token(payload: &ScrubbedPayload) -> Result<()> {
        let computed = compute_fnv1a_digest(&payload.json);
        if computed != payload.token.payload_digest() {
            bail!(
                "Zero-trust privacy violation: ScrubbedPayload token digest mismatch. \
                 Payload may have been tampered with after PrivacyGuard."
            );
        }
        Ok(())
    }

    fn compose_skills(&self, skill_ids: &[String]) -> String {
        if let Some(registry) = &self.skills {
            registry
                .compose_system_prompt(skill_ids)
                .unwrap_or_else(|e| {
                    tracing::warn!("Failed to compose skill prompt: {e}");
                    String::new()
                })
        } else {
            String::new()
        }
    }

    fn resolved_base_url(cfg: &AiConfig) -> String {
        let trimmed = cfg.gemini_base_url.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            GEMINI_API_BASE_URL.to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn resolved_model(cfg: &AiConfig) -> String {
        let trimmed = cfg.gemini_model.trim();
        let cleaned = trimmed.strip_prefix("models/").unwrap_or(trimmed);
        if cleaned.is_empty() {
            DEFAULT_GEMINI_MODEL.to_string()
        } else {
            cleaned.to_string()
        }
    }

    fn require_api_key(cfg: &AiConfig) -> Result<String> {
        cfg.gemini_api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow!("Gemini API key is not configured. Add your API key in Settings.")
            })
    }

    async fn stream_generate_content(
        &self,
        system_prompt: String,
        user_prompt: String,
        payload: &ScrubbedPayload,
        events: &AiEventSink,
        allow_ats: bool,
        only_bullet_id: Option<Uuid>,
    ) -> Result<(Vec<AiProposal>, Option<AtsScore>, String)> {
        let cfg = self.config();
        let api_key = Self::require_api_key(&cfg)?;
        let base_url = Self::resolved_base_url(&cfg);
        let model = Self::resolved_model(&cfg);

        let url = format!("{base_url}/models/{model}:streamGenerateContent?alt=sse");

        let function_declarations: Vec<serde_json::Value> = get_tool_declarations()
            .into_iter()
            .filter(|tool| only_bullet_id.is_none() || tool.name == "propose_bullet_rewrite")
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": sanitize_gemini_schema(tool.parameters),
                })
            })
            .collect();

        let body = serde_json::json!({
            "systemInstruction": {
                "parts": [{ "text": system_prompt }]
            },
            "contents": [{
                "role": "user",
                "parts": [{ "text": user_prompt }]
            }],
            "tools": [{
                "functionDeclarations": function_declarations
            }],
            "toolConfig": {
                "functionCallingConfig": {
                    "mode": "AUTO"
                }
            },
            "generationConfig": {
                "temperature": 0.2
            }
        });

        // Retry once on transient connection/reset errors (e.g. closed edge socket)
        // before any HTTP headers are received.
        let mut response_opt = None;
        for attempt in 0..2 {
            match self
                .http_client()
                .post(&url)
                .header("x-goog-api-key", &api_key)
                .header("accept", "text/event-stream")
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => {
                    response_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let transient =
                        !err.is_timeout() && (err.is_connect() || err.is_request() || err.is_body());
                    if attempt == 0 && transient {
                        tracing::warn!(
                            "Transient Gemini connection error on attempt 1 ({err}); retrying…"
                        );
                        let _ = events.send(AiEvent::Status {
                            message: format!(
                                "Reconnecting to Google Gemini ({model})…"
                            ),
                        });
                        tokio::time::sleep(Duration::from_millis(350)).await;
                        continue;
                    }
                    bail!("{}", format_gemini_transport_error(&err, &model));
                }
            }
        }
        let response = response_opt
            .ok_or_else(|| anyhow!("Failed to connect to Google Gemini API ({model})."))?;

        if !response.status().is_success() {
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let err_text = response.text().await.unwrap_or_default();
            bail!(
                "{}",
                format_gemini_api_error_with_retry(
                    status,
                    &err_text,
                    &model,
                    retry_after.as_deref()
                )
            );
        }

        let mut proposals = Vec::new();
        let mut ats_score = None;
        let mut summary_text = String::new();
        let mut notified_thinking = false;

        let mut stream = response.bytes_stream().eventsource();
        while let Some(event_res) = stream.next().await {
            let ev = event_res.map_err(|e| anyhow!("Error reading Gemini SSE stream: {e}"))?;
            let data_payload = ev.data.trim();
            if data_payload.is_empty() || data_payload == "[DONE]" {
                continue;
            }

            process_gemini_chunk(
                data_payload,
                payload,
                events,
                allow_ats,
                only_bullet_id,
                &mut proposals,
                &mut ats_score,
                &mut summary_text,
                &mut notified_thinking,
            )?;
        }

        Ok((proposals, ats_score, summary_text))
    }
}

/// Recursively strips JSON Schema fields rejected by Google Gemini v1beta
/// `functionDeclarations[].parameters` (such as `additionalProperties`).
pub fn sanitize_gemini_schema(mut schema: serde_json::Value) -> serde_json::Value {
    match &mut schema {
        serde_json::Value::Object(map) => {
            map.remove("additionalProperties");
            for value in map.values_mut() {
                let taken = std::mem::replace(value, serde_json::Value::Null);
                *value = sanitize_gemini_schema(taken);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                let taken = std::mem::replace(item, serde_json::Value::Null);
                *item = sanitize_gemini_schema(taken);
            }
        }
        _ => {}
    }
    schema
}

#[allow(clippy::too_many_arguments)]
fn process_gemini_chunk(
    data_payload: &str,
    payload: &ScrubbedPayload,
    events: &AiEventSink,
    allow_ats: bool,
    only_bullet_id: Option<Uuid>,
    proposals: &mut Vec<AiProposal>,
    ats_score: &mut Option<AtsScore>,
    summary_text: &mut String,
    notified_thinking: &mut bool,
) -> Result<()> {
    let Ok(chunk) = serde_json::from_str::<GeminiStreamChunk>(data_payload) else {
        tracing::debug!("Skipping unparseable Gemini SSE chunk");
        return Ok(());
    };

    if let Some(err) = chunk.error {
        let msg = err
            .message
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty());
        if is_gemini_limit_detail(err.code, err.status.as_deref(), msg) {
            let extra = msg.map(|m| format!(" ({m})")).unwrap_or_default();
            bail!(
                "API limit reached (Google Gemini): Your rate limit or quota has been reached{extra}. Wait briefly, check your Google AI Studio quota, or switch to another model in Settings."
            );
        }
        let fallback = msg.unwrap_or("Gemini API returned a stream error.");
        bail!("Gemini stream error: {fallback}");
    }

    for candidate in chunk.candidates.unwrap_or_default() {
        let Some(content) = candidate.content else {
            continue;
        };
        for part in content.parts.unwrap_or_default() {
            if part.thought {
                if !*notified_thinking {
                    *notified_thinking = true;
                    let _ = events.send(AiEvent::Status {
                        message: "Gemini is analyzing your CV against the job description…"
                            .to_string(),
                    });
                }
                continue;
            }
            if let Some(text) = part.text
                && !text.is_empty()
            {
                summary_text.push_str(&text);
                let _ = events.send(AiEvent::TextDelta { delta: text });
            }
            if let Some(fc) = part.function_call {
                let args = fc.args.unwrap_or_else(|| serde_json::json!({}));
                let _ = events.send(AiEvent::ToolCall {
                    name: fc.name.clone(),
                    arguments: args.clone(),
                });
                match parse_tool_call(&fc.name, &args, &payload.body) {
                    Ok(ToolCallResult::Proposal(prop)) => {
                        if let Some(target_id) = only_bullet_id {
                            let matches_target = matches!(
                                prop,
                                AiProposal::BulletRewrite { bullet_id, .. } if bullet_id == target_id
                            );
                            if !matches_target {
                                continue;
                            }
                        }
                        let _ = events.send(AiEvent::Proposal {
                            proposal: prop.clone(),
                        });
                        proposals.push(prop);
                    }
                    Ok(ToolCallResult::Score(score)) => {
                        if allow_ats {
                            let _ = events.send(AiEvent::AtsScore {
                                score: score.clone(),
                            });
                            *ats_score = Some(score);
                        }
                    }
                    Err(err) => {
                        tracing::warn!("Rejected Gemini tool call '{}': {err}", fc.name);
                        let _ = events.send(AiEvent::Status {
                            message: format!("Skipped invalid AI tool call ({}): {err}", fc.name),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn build_gemini_system_prompt(skill_directives: &str) -> String {
    format!(
        r#"You are VanguardCV AI Assistant, an elite executive career strategist and ATS optimization engine.
Your mission is to tailor the candidate's CV for a specific target job.

CRITICAL ANTI-HALLUCINATION & INTEGRITY DIRECTIVES:
1. NEVER invent, fabricate, or assume degrees, certifications, universities, organizations, job titles, or dates not present in the candidate's CV data.
2. NEVER fabricate numerical metrics or claim accomplishments not grounded in the candidate's provided text.
3. Align terminology and technical keywords from the job description ONLY when the candidate genuinely possesses that skill in their experience or skills section.
4. Interact with the CV exclusively through the provided deterministic function tools.
5. PRUNE IRRELEVANT OR HARMFUL CONTENT: Carefully audit every section, timeline entry, bullet, and skill category against the target job description. Call `toggle_item_visibility(item_id, enabled=false, reason=...)` to hide/remove items that are not required, redundant, off-domain, or could dilute or harm the candidate's fit for this specific role. Always give a clear, specific `reason`.
6. END-OF-RUN REASONING: After emitting your tool calls, always write a clear, concise reasoning summary for the user explaining: (a) key tailoring improvements made, (b) which parts you recommended removing/hiding and why they were unnecessary or could hurt fit, and (c) top remaining gaps or advice.

=== ACTIVE AI SKILLS ===
{skill_directives}
"#
    )
}

#[async_trait]
impl AiOrchestrator for GeminiClient {
    async fn generate_variation(
        &self,
        request: VariationRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<VariationOutcome> {
        let run_id = Uuid::new_v4();
        let _ = events.send(AiEvent::RunStarted { run_id });

        if let Err(err) = Self::verify_payload_token(&payload) {
            let _ = events.send(AiEvent::RunFailed {
                run_id,
                message: err.to_string(),
            });
            return Err(err);
        }

        let _ = events.send(AiEvent::Status {
            message: format!(
                "Preparing zero-trust tailoring pass with Gemini ({})…",
                Self::resolved_model(&self.config())
            ),
        });

        let skill_directives = self.compose_skills(&request.skill_ids);
        let system_prompt = build_gemini_system_prompt(&skill_directives);

        let user_prompt = format!(
            "Target Company: {}\nTarget Role: {}\n\nJob Description:\n{}\n\n\
             Candidate Scrubbed CV (JSON):\n{}\n\n\
             Instructions:\n\
             1. Audit every section, timeline entry, bullet, and skill category against the target job description.\n\
             2. Proactively call `toggle_item_visibility(item_id, enabled=false, reason=...)` to REMOVE (hide from this tailored PDF) \
             any sections, entries, bullets, or skill categories that are irrelevant, redundant, off-domain, or could weaken/harm the candidate's fit for this role. Always explain clearly why in `reason`.\n\
             3. Call `propose_bullet_rewrite`, `propose_summary_rewrite`, and `reorder_skill_category_items` to strengthen relevant achievements without inventing facts or metrics.\n\
             4. Call `score_ats_alignment` with matched/missing keywords and a 0-100 score.\n\
             5. After emitting tool calls, write a clear, concise reasoning summary for the user explaining what you tailored, which parts you recommended removing/hiding and why, and any remaining gaps.",
            request.job_context.company_name,
            request.job_context.target_role,
            request.job_context.job_description_raw,
            payload.json
        );

        let (proposals, ats_score, summary_text) = match self
            .stream_generate_content(system_prompt, user_prompt, &payload, &events, true, None)
            .await
        {
            Ok(res) => res,
            Err(err) => {
                let _ = events.send(AiEvent::RunFailed {
                    run_id,
                    message: format!("{err:#}"),
                });
                return Err(err);
            }
        };

        let summary = synthesize_tailoring_summary(
            &summary_text,
            &proposals,
            ats_score.as_ref(),
            &payload.body,
            "Google Gemini",
        );

        let _ = events.send(AiEvent::RunCompleted {
            run_id,
            summary: summary.clone(),
        });

        Ok(VariationOutcome {
            child_cv_id: request.child_cv_id,
            proposals,
            ats_score,
            summary,
        })
    }

    async fn rewrite_bullet(
        &self,
        request: InlineRewriteRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<Option<AiProposal>> {
        let run_id = Uuid::new_v4();
        let _ = events.send(AiEvent::RunStarted { run_id });

        if let Err(err) = Self::verify_payload_token(&payload) {
            let _ = events.send(AiEvent::RunFailed {
                run_id,
                message: err.to_string(),
            });
            return Err(err);
        }

        let Some(target_bullet) = payload.body.find_bullet(request.bullet_id) else {
            let err = anyhow!(
                "Target bullet {} not found in scrubbed CV body",
                request.bullet_id
            );
            let _ = events.send(AiEvent::RunFailed {
                run_id,
                message: err.to_string(),
            });
            return Err(err);
        };

        let skill_directives = self.compose_skills(&request.skill_ids);
        let system_prompt = build_gemini_system_prompt(&skill_directives);

        let user_prompt = format!(
            "Target Bullet ID: {}\nCurrent Bullet Text: {}\nUser Instruction: {}\n\n\
             Full Scrubbed CV Context (JSON):\n{}\n\n\
             Call `propose_bullet_rewrite` once for bullet_id `{}` following the instruction \
             while preserving all factual details and metrics.",
            request.bullet_id,
            target_bullet.text,
            request.instruction,
            payload.json,
            request.bullet_id
        );

        let (proposals, _, _) = match self
            .stream_generate_content(
                system_prompt,
                user_prompt,
                &payload,
                &events,
                false,
                Some(request.bullet_id),
            )
            .await
        {
            Ok(res) => res,
            Err(err) => {
                let _ = events.send(AiEvent::RunFailed {
                    run_id,
                    message: format!("{err:#}"),
                });
                return Err(err);
            }
        };

        let matching = proposals.into_iter().find(|p| {
            matches!(
                p,
                AiProposal::BulletRewrite { bullet_id, .. } if *bullet_id == request.bullet_id
            )
        });

        let summary = if matching.is_some() {
            "Generated inline bullet rewrite via Gemini.".to_string()
        } else {
            "Gemini completed without proposing a bullet rewrite.".to_string()
        };
        let _ = events.send(AiEvent::RunCompleted { run_id, summary });

        Ok(matching)
    }

    async fn complete_structured_prompt(
        &self,
        system_prompt: String,
        user_prompt: String,
    ) -> Result<String> {
        let cfg = self.config();
        let api_key = Self::require_api_key(&cfg)?;
        let base_url = Self::resolved_base_url(&cfg);
        let model = Self::resolved_model(&cfg);
        let url = format!("{base_url}/models/{model}:generateContent");

        let body = serde_json::json!({
            "systemInstruction": {
                "parts": [{ "text": system_prompt }]
            },
            "contents": [{
                "role": "user",
                "parts": [{ "text": user_prompt }]
            }],
            "generationConfig": {
                "temperature": 0.2,
                "responseMimeType": "application/json"
            }
        });

        let mut response_opt = None;
        for attempt in 0..2 {
            match self
                .http_client()
                .post(&url)
                .header("x-goog-api-key", &api_key)
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => {
                    response_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let transient =
                        !err.is_timeout() && (err.is_connect() || err.is_request() || err.is_body());
                    if attempt == 0 && transient {
                        tokio::time::sleep(Duration::from_millis(350)).await;
                        continue;
                    }
                    bail!("{}", format_gemini_transport_error(&err, &model));
                }
            }
        }
        let response = response_opt
            .ok_or_else(|| anyhow!("Failed to connect to Google Gemini API ({model})."))?;

        if !response.status().is_success() {
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let err_text = response.text().await.unwrap_or_default();
            bail!(
                "{}",
                format_gemini_api_error_with_retry(
                    status,
                    &err_text,
                    &model,
                    retry_after.as_deref()
                )
            );
        }

        let resp_text = response.text().await.unwrap_or_default();
        let parsed: GeminiStreamChunk = serde_json::from_str(&resp_text)
            .context("Failed to parse Google Gemini JSON completion response")?;
        if let Some(err) = parsed.error {
            let msg = err.message.as_deref().map(str::trim).filter(|m| !m.is_empty());
            if is_gemini_limit_detail(err.code, err.status.as_deref(), msg) {
                let extra = msg.map(|m| format!(" ({m})")).unwrap_or_default();
                bail!(
                    "API limit reached (Google Gemini): Your rate limit or quota has been reached{extra}. Wait briefly, check your Google AI Studio quota, or switch to another model in Settings."
                );
            }
            bail!("{}", msg.unwrap_or("Google Gemini returned an API error."));
        }

        let mut output = String::new();
        for candidate in parsed.candidates.unwrap_or_default() {
            if let Some(content) = candidate.content {
                for part in content.parts.unwrap_or_default() {
                    if !part.thought
                        && let Some(text) = part.text
                    {
                        output.push_str(&text);
                    }
                }
            }
        }
        if output.trim().is_empty() {
            bail!("Google Gemini returned an empty response for structured completion.");
        }
        Ok(output)
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let cfg = self.config();
        let api_key = Self::require_api_key(&cfg)?;
        let base_url = Self::resolved_base_url(&cfg);

        let mut all_models = Vec::new();
        let mut seen_ids = HashSet::new();
        let mut page_token: Option<String> = None;

        for _ in 0..5 {
            let mut parsed_url = Url::parse(&format!("{base_url}/models"))
                .context("Invalid Gemini API base URL")?;
            {
                let mut pairs = parsed_url.query_pairs_mut();
                pairs.append_pair("pageSize", "100");
                if let Some(tok) = &page_token {
                    pairs.append_pair("pageToken", tok);
                }
            }

            let response = self
                .http_client()
                .get(parsed_url.as_str())
                .header("x-goog-api-key", &api_key)
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|err| anyhow!("{}", format_gemini_transport_error(&err, "models")))?;

            if !response.status().is_success() {
                let status = response.status();
                let err_text = response.text().await.unwrap_or_default();
                bail!("{}", format_gemini_api_error(status, &err_text, "models"));
            }

            let body = response
                .text()
                .await
                .context("Failed to read Gemini models response")?;
            let (page_models, next_token) = parse_gemini_models_response(&body)?;
            for model in page_models {
                if seen_ids.insert(model.id.clone()) {
                    all_models.push(model);
                }
            }

            match next_token {
                Some(tok) if !tok.is_empty() => page_token = Some(tok),
                _ => break,
            }
        }

        all_models.sort_by(|a, b| a.display_name.cmp(&b.display_name).then(a.id.cmp(&b.id)));
        Ok(all_models)
    }

    fn configure(&self, config: AiConfig) -> Result<()> {
        let new_client = build_chatgpt_http_client(config.request_timeout_secs)?;
        if let Ok(mut lock) = self.config.write() {
            *lock = config;
        }
        if let Ok(mut lock) = self.http.write() {
            *lock = new_client;
        }
        Ok(())
    }

    fn config(&self) -> AiConfig {
        self.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    fn is_configured(&self) -> bool {
        self.config().is_gemini_configured()
    }
}

// ---------------------------------------------------------------------------
// Wire schemas & error formatting
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GeminiModelsResponse {
    #[serde(default)]
    models: Vec<GeminiModelRecord>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiModelRecord {
    name: String,
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "inputTokenLimit", default)]
    input_token_limit: Option<u64>,
    #[serde(rename = "outputTokenLimit", default)]
    output_token_limit: Option<u64>,
    #[serde(rename = "supportedGenerationMethods", default)]
    supported_generation_methods: Vec<String>,
    #[serde(default)]
    thinking: Option<bool>,
}

/// Parses `GET /v1beta/models` into VanguardCV [`ModelInfo`] items, keeping only
/// `generateContent`-capable Gemini models.
pub fn parse_gemini_models_response(body: &str) -> Result<(Vec<ModelInfo>, Option<String>)> {
    let parsed: GeminiModelsResponse =
        serde_json::from_str(body).context("Failed to parse Gemini models JSON")?;

    let mut out = Vec::new();
    for record in parsed.models {
        let supports_generate = record
            .supported_generation_methods
            .iter()
            .any(|m| m == "generateContent");
        if !supports_generate {
            continue;
        }
        let id = record
            .name
            .strip_prefix("models/")
            .unwrap_or(&record.name)
            .trim()
            .to_string();
        if id.is_empty() {
            continue;
        }
        let lower = id.to_ascii_lowercase();
        if lower.contains("embedding")
            || lower.contains("imagen")
            || lower.contains("veo")
            || lower.contains("gemma")
            || lower.contains("tts")
            || lower.contains("audio")
            || lower.contains("image")
            || lower.contains("aqa")
        {
            continue;
        }
        let display_name = record
            .display_name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| id.clone());
        let supports_thinking = record.thinking.unwrap_or_else(|| {
            lower.contains("2.5") || lower.contains("thinking") || lower.contains("gemini-3")
        });

        out.push(ModelInfo {
            id,
            display_name,
            description: record.description.unwrap_or_default(),
            input_token_limit: record.input_token_limit,
            output_token_limit: record.output_token_limit,
            supports_thinking,
        });
    }

    Ok((out, parsed.next_page_token))
}

#[derive(Debug, Deserialize)]
struct GeminiStreamChunk {
    candidates: Option<Vec<GeminiCandidate>>,
    error: Option<GeminiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    parts: Option<Vec<GeminiPart>>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    #[serde(default)]
    thought: bool,
    text: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GeminiFunctionCall {
    name: String,
    args: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorEnvelope {
    error: Option<GeminiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorDetail {
    #[serde(default)]
    code: Option<u16>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn is_gemini_limit_detail(
    code: Option<u16>,
    api_status: Option<&str>,
    message: Option<&str>,
) -> bool {
    if code == Some(429) {
        return true;
    }
    if api_status.is_some_and(|s| s.eq_ignore_ascii_case("RESOURCE_EXHAUSTED")) {
        return true;
    }
    message.is_some_and(|m| {
        let lower = m.to_ascii_lowercase();
        lower.contains("quota")
            || lower.contains("rate limit")
            || lower.contains("resource_exhausted")
            || lower.contains("too many requests")
            || lower.contains("limit reached")
            || lower.contains("exceeded your current quota")
    })
}

/// Translates a `reqwest::Error` into an actionable user-facing message that
/// preserves the real root cause without leaking sensitive credentials.
pub fn format_gemini_transport_error(err: &reqwest::Error, model: &str) -> String {
    let mut chain = Vec::new();
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        let msg = cause.to_string();
        if !msg.trim().is_empty() && !chain.iter().any(|existing| existing == &msg) {
            chain.push(msg);
        }
        source = cause.source();
    }
    let detail = if chain.is_empty() {
        err.to_string()
    } else {
        chain.join(": ")
    };

    if err.is_timeout() {
        format!(
            "Timed out waiting for Google Gemini ({model}). Try again or switch to a faster model in Settings ({detail})."
        )
    } else if err.is_connect() {
        format!(
            "Could not reach Google Gemini API ({detail}). Check your internet connection, firewall, or proxy."
        )
    } else {
        format!("Google Gemini network error ({model}): {detail}")
    }
}

pub fn format_gemini_api_error(status: reqwest::StatusCode, body: &str, model: &str) -> String {
    format_gemini_api_error_with_retry(status, body, model, None)
}

pub fn format_gemini_api_error_with_retry(
    status: reqwest::StatusCode,
    body: &str,
    model: &str,
    retry_after: Option<&str>,
) -> String {
    let parsed_err = serde_json::from_str::<GeminiErrorEnvelope>(body)
        .ok()
        .and_then(|e| e.error);
    let api_code = parsed_err.as_ref().and_then(|d| d.code);
    let api_status = parsed_err.as_ref().and_then(|d| d.status.as_deref());
    let detail = parsed_err
        .as_ref()
        .and_then(|d| d.message.as_deref())
        .map(str::trim)
        .filter(|m| !m.is_empty());

    if status.as_u16() == 429 || is_gemini_limit_detail(api_code, api_status, detail) {
        let retry_hint = retry_after
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| format!(" Retry after ~{v}s."))
            .unwrap_or_default();
        let detail_hint = detail
            .map(|m| {
                let compact: String = m.chars().take(180).collect();
                format!(" Details: {compact}")
            })
            .unwrap_or_default();
        return format!(
            "API limit reached (Google Gemini, model \"{model}\"): Your rate limit or quota has been reached.{retry_hint} Wait briefly, check your Google AI Studio quota, or switch to another model in Settings.{detail_hint}"
        );
    }

    match status.as_u16() {
        400 if detail.is_some_and(|m| m.to_ascii_lowercase().contains("api key")) => {
            "Invalid Gemini API key. Check your key in Settings (Google AI Studio).".to_string()
        }
        401 | 403 => {
            "Gemini API key was rejected or lacks permission. Verify your key in Settings."
                .to_string()
        }
        404 => format!(
            "Gemini model \"{model}\" was not found. Refresh models in Settings and pick an available model."
        ),
        _ => match detail {
            Some(msg) => format!("Gemini API error ({status}): {msg}"),
            None => format!("Gemini API error ({status})."),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{JobContext, PiiContactHeader, PrivacyGuard};
    use crate::mocks::sample_root_document;
    use crate::privacy::scrubber::PiiScrubber;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn parses_gemini_models_and_filters_non_generate_models() {
        let json = r#"{
            "models": [
                {
                    "name": "models/gemini-2.5-flash",
                    "displayName": "Gemini 2.5 Flash",
                    "description": "Fast hybrid reasoning model.",
                    "inputTokenLimit": 1048576,
                    "outputTokenLimit": 65536,
                    "supportedGenerationMethods": ["generateContent", "countTokens"]
                },
                {
                    "name": "models/text-embedding-004",
                    "displayName": "Embedding 004",
                    "supportedGenerationMethods": ["embedContent"]
                }
            ]
        }"#;

        let (models, next) = parse_gemini_models_response(json).expect("parse models");
        assert!(next.is_none());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gemini-2.5-flash");
        assert_eq!(models[0].display_name, "Gemini 2.5 Flash");
        assert!(models[0].supports_thinking);
        assert_eq!(models[0].input_token_limit, Some(1_048_576));
    }

    #[tokio::test]
    async fn streams_gemini_tool_calls_and_models_against_mock_server() {
        let doc = sample_root_document();
        let bullet = doc.body.bullets()[0].clone();
        let scrubber = PiiScrubber;
        let payload = scrubber
            .scrub(&doc.body, &PiiContactHeader::default())
            .expect("scrub");

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let bullet_id = bullet.id;

        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let mut buf = vec![0u8; 65536];
                let n = socket.read(&mut buf).await.expect("read");
                let req = String::from_utf8_lossy(&buf[..n]);

                if req.starts_with("GET /models") {
                    let body = r#"{"models":[{"name":"models/gemini-2.5-pro","displayName":"Gemini 2.5 Pro","description":"Reasoning model","inputTokenLimit":1048576,"supportedGenerationMethods":["generateContent"]}]}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    socket.write_all(resp.as_bytes()).await.expect("write");
                } else {
                    let sse_chunk = serde_json::json!({
                        "candidates": [{
                            "content": {
                                "parts": [
                                    { "text": "Tailoring complete." },
                                    {
                                        "functionCall": {
                                            "name": "propose_bullet_rewrite",
                                            "args": {
                                                "bullet_id": bullet_id.to_string(),
                                                "proposed_text": "Engineered high-impact security assessments across enterprise targets.",
                                                "rationale": "Stronger XYZ framing"
                                            }
                                        }
                                    }
                                ]
                            }
                        }]
                    });
                    let body = format!("data: {}\n\n", sse_chunk);
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    socket.write_all(resp.as_bytes()).await.expect("write");
                }
            }
        });

        let cfg = AiConfig {
            gemini_base_url: format!("http://{addr}"),
            gemini_api_key: Some("test-gemini-key".to_string()),
            gemini_model: "gemini-2.5-pro".to_string(),
            ..AiConfig::default()
        };
        let client = GeminiClient::new(cfg);

        let models = client.list_models().await.expect("list models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gemini-2.5-pro");

        let (tx, _rx) = unbounded_channel();
        let outcome = client
            .generate_variation(
                VariationRequest {
                    child_cv_id: Uuid::new_v4(),
                    job_context: JobContext {
                        company_name: "Acme".to_string(),
                        target_role: "Security Engineer".to_string(),
                        job_description_raw: "Lead pentests".to_string(),
                        application_url: None,
                        applied_skill_ids: Vec::new(),
                        ats_match_score: None,
                    },
                    skill_ids: Vec::new(),
                },
                payload,
                tx,
            )
            .await
            .expect("generate variation");

        assert_eq!(outcome.proposals.len(), 1);
        server.await.expect("server");
    }

    #[tokio::test]
    async fn retries_transient_connection_drop_and_synthesizes_removal_reasoning() {
        let doc = sample_root_document();
        let hide_section_id = doc.body.sections.last().expect("section").id;
        let scrubber = PiiScrubber;
        let payload = scrubber
            .scrub(&doc.body, &PiiContactHeader::default())
            .expect("scrub");

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            // Attempt 1: accept and drop immediately to simulate a stale/reset socket.
            let (socket, _) = listener.accept().await.expect("accept 1");
            drop(socket);

            // Attempt 2: stream a thinking part + toggle_item_visibility removal + ATS score.
            let (mut socket, _) = listener.accept().await.expect("accept 2");
            let mut buf = vec![0u8; 65536];
            let _ = socket.read(&mut buf).await.expect("read 2");

            let sse_chunk = serde_json::json!({
                "candidates": [{
                    "content": {
                        "parts": [
                            { "thought": true, "text": "Internal chain of thought that should stay hidden." },
                            {
                                "functionCall": {
                                    "name": "toggle_item_visibility",
                                    "args": {
                                        "item_id": hide_section_id.to_string(),
                                        "enabled": false,
                                        "reason": "Unrelated to senior offensive security consulting and adds page bloat."
                                    }
                                }
                            },
                            {
                                "functionCall": {
                                    "name": "score_ats_alignment",
                                    "args": {
                                        "matched_keywords": ["Red Team", "Pentesting"],
                                        "missing_keywords": ["Cloud IAM"],
                                        "score": 84
                                    }
                                }
                            }
                        ]
                    }
                }]
            });
            let body = format!("data: {}\n\n", sse_chunk);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(resp.as_bytes()).await.expect("write 2");
        });

        let cfg = AiConfig {
            gemini_base_url: format!("http://{addr}"),
            gemini_api_key: Some("test-gemini-key".to_string()),
            gemini_model: "gemini-2.5-flash".to_string(),
            ..AiConfig::default()
        };
        let client = GeminiClient::new(cfg);

        let (tx, _rx) = unbounded_channel();
        let outcome = client
            .generate_variation(
                VariationRequest {
                    child_cv_id: Uuid::new_v4(),
                    job_context: JobContext {
                        company_name: "Bishop Fox".to_string(),
                        target_role: "Senior Offensive Security Consultant".to_string(),
                        job_description_raw: "Lead offensive security assessments and cloud IAM reviews.".to_string(),
                        application_url: None,
                        applied_skill_ids: Vec::new(),
                        ats_match_score: None,
                    },
                    skill_ids: Vec::new(),
                },
                payload,
                tx,
            )
            .await
            .expect("retry should recover cleanly");

        assert_eq!(outcome.proposals.len(), 1);
        assert!(matches!(
            &outcome.proposals[0],
            AiProposal::ToggleVisibility { item_id, enabled: false, .. } if *item_id == hide_section_id
        ));
        assert!(!outcome.summary.contains("Internal chain of thought"));
        assert!(outcome.summary.contains("Recommended removals"));
        assert!(outcome.summary.contains("Unrelated to senior offensive security consulting"));
        assert!(outcome.summary.contains("Cloud IAM"));
        server.await.expect("server");
    }

    #[test]
    fn gemini_limit_errors_surface_clear_api_limit_message_and_retry_hint() {
        let body = r#"{
            "error": {
                "code": 429,
                "status": "RESOURCE_EXHAUSTED",
                "message": "You exceeded your current quota, please check your plan and billing details."
            }
        }"#;
        let msg = format_gemini_api_error_with_retry(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            body,
            "gemini-2.5-flash",
            Some("45"),
        );
        assert!(msg.starts_with("API limit reached"), "got: {msg}");
        assert!(msg.contains("gemini-2.5-flash"), "got: {msg}");
        assert!(msg.contains("~45s"), "got: {msg}");
        assert!(crate::ai::is_ai_limit_error(&msg));
    }
}
