//! Multi-provider AI orchestrator dispatching between:
//! - Official OpenAI Sign in with ChatGPT (OAuth 2.0 / OIDC + Responses API)
//! - Official Google Gemini API (`generativelanguage.googleapis.com/v1beta`)

use std::sync::{Arc, RwLock};

use anyhow::Result;
use async_trait::async_trait;

use crate::ai::chatgpt::ChatGptClient;
use crate::ai::gemini::GeminiClient;
use crate::contracts::{
    AiConfig, AiEventSink, AiOrchestrator, AiProposal, AiProviderKind, ChatGptAuthStatus,
    InlineRewriteRequest, ModelInfo, ScrubbedPayload, SkillRegistry, VariationOutcome,
    VariationRequest,
};

/// Composite [`AiOrchestrator`] that keeps both ChatGPT OAuth and Google Gemini
/// API available simultaneously and routes inference / model discovery to the
/// currently selected [`AiProviderKind`].
pub struct MultiProviderAiClient {
    config: RwLock<AiConfig>,
    chatgpt: ChatGptClient,
    gemini: GeminiClient,
}

impl MultiProviderAiClient {
    pub fn from_env_with_skills(skills: Arc<dyn SkillRegistry>) -> Self {
        let chatgpt = ChatGptClient::from_env().with_skills(skills.clone());
        let mut initial_config = chatgpt.config();
        if let Ok(key) = std::env::var("GEMINI_API_KEY")
            && !key.trim().is_empty()
        {
            initial_config.gemini_api_key = Some(key.trim().to_string());
        }
        let gemini = GeminiClient::new(initial_config.clone()).with_skills(skills);
        Self {
            config: RwLock::new(initial_config),
            chatgpt,
            gemini,
        }
    }

    fn active_provider(&self) -> AiProviderKind {
        self.config
            .read()
            .map(|c| c.provider)
            .unwrap_or(AiProviderKind::ChatGpt)
    }
}

#[async_trait]
impl AiOrchestrator for MultiProviderAiClient {
    async fn generate_variation(
        &self,
        request: VariationRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<VariationOutcome> {
        match self.active_provider() {
            AiProviderKind::ChatGpt => {
                self.chatgpt
                    .generate_variation(request, payload, events)
                    .await
            }
            AiProviderKind::GeminiApi => {
                self.gemini
                    .generate_variation(request, payload, events)
                    .await
            }
        }
    }

    async fn rewrite_bullet(
        &self,
        request: InlineRewriteRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<Option<AiProposal>> {
        match self.active_provider() {
            AiProviderKind::ChatGpt => self.chatgpt.rewrite_bullet(request, payload, events).await,
            AiProviderKind::GeminiApi => self.gemini.rewrite_bullet(request, payload, events).await,
        }
    }

    async fn complete_structured_prompt(
        &self,
        system_prompt: String,
        user_prompt: String,
    ) -> Result<String> {
        match self.active_provider() {
            AiProviderKind::ChatGpt => {
                self.chatgpt
                    .complete_structured_prompt(system_prompt, user_prompt)
                    .await
            }
            AiProviderKind::GeminiApi => {
                self.gemini
                    .complete_structured_prompt(system_prompt, user_prompt)
                    .await
            }
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        match self.active_provider() {
            AiProviderKind::ChatGpt => self.chatgpt.list_models().await,
            AiProviderKind::GeminiApi => self.gemini.list_models().await,
        }
    }

    fn auth_status(&self) -> ChatGptAuthStatus {
        self.chatgpt.auth_status()
    }

    async fn sign_in_with_chatgpt(
        &self,
        force_consent: bool,
        fresh_registration: bool,
    ) -> Result<ChatGptAuthStatus> {
        self.chatgpt
            .sign_in_with_chatgpt(force_consent, fresh_registration)
            .await
    }

    async fn sign_out(&self) -> Result<Option<String>> {
        self.chatgpt.sign_out().await
    }

    fn configure(&self, config: AiConfig) -> Result<()> {
        self.chatgpt.configure(config.clone())?;
        self.gemini.configure(config.clone())?;
        if let Ok(mut lock) = self.config.write() {
            *lock = config;
        }
        Ok(())
    }

    fn config(&self) -> AiConfig {
        self.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    fn is_configured(&self) -> bool {
        match self.active_provider() {
            AiProviderKind::ChatGpt => self.chatgpt.is_configured(),
            AiProviderKind::GeminiApi => self.gemini.is_configured(),
        }
    }
}
