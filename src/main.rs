//! VanguardCV desktop entry point.

#![cfg_attr(
    all(not(debug_assertions), feature = "ui"),
    windows_subsystem = "windows"
)]

#[cfg(feature = "ui")]
use vanguard_cv::ai::provider::MultiProviderAiClient;
#[cfg(feature = "ui")]
use vanguard_cv::ai::skill_registry::FileSkillRegistry;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vanguard_cv=info".into()),
        )
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "VanguardCV starting");

    #[cfg(feature = "ui")]
    {
        vanguard_cv::runtime::init();
        let services = build_services()?;
        vanguard_cv::ui::run(services)?;
    }

    #[cfg(not(feature = "ui"))]
    {
        println!(
            "VanguardCV {} built without the desktop UI. Run `cargo run --features ui`.",
            env!("CARGO_PKG_VERSION")
        );
    }

    Ok(())
}

/// Builds the production service graph: local SQLite storage (starting cleanly
/// empty on fresh installs), the zero-trust PII scrubber, the embedded Typst
/// PDF engine, the multi-provider AI client (Sign in with ChatGPT + Google
/// Gemini API), and the hot-reloading skill registry.
#[cfg(feature = "ui")]
fn build_services() -> anyhow::Result<vanguard_cv::contracts::Services> {
    use std::sync::Arc;

    use vanguard_cv::contracts::{
        AiOrchestrator, AiProviderKind, CvRepository, DEFAULT_CHATGPT_MODEL, DEFAULT_GEMINI_MODEL,
        Services,
    };
    use vanguard_cv::db::repository::SqliteRepository;
    use vanguard_cv::pdf::typst_compiler::TypstCompiler;
    use vanguard_cv::privacy::scrubber::PiiScrubber;

    let repository = match SqliteRepository::open_default() {
        Ok(repository) => repository,
        Err(err) => {
            tracing::error!(
                "could not open the local database ({err}); falling back to an in-memory library"
            );
            SqliteRepository::in_memory()?
        }
    };

    let pdf = TypstCompiler::new()?;

    let skill_registry = Arc::new(FileSkillRegistry::new()?);
    // Keep the filesystem watcher alive for the lifetime of the application.
    // It runs on notify's own background thread.
    if let Ok(watcher) = skill_registry.start_watcher() {
        std::mem::forget(watcher);
    }
    let skills: Arc<dyn vanguard_cv::contracts::SkillRegistry> = skill_registry;

    let ai = MultiProviderAiClient::from_env_with_skills(skills.clone());
    let mut ai_config = ai.config();

    if let Ok(Some(provider_str)) = repository.load_setting("ai.provider") {
        ai_config.provider = AiProviderKind::from_setting(&provider_str);
    }
    if let Ok(Some(model)) = repository.load_setting("ai.model") {
        let trimmed = model.trim();
        let lower = trimmed.to_ascii_lowercase();
        if !trimmed.is_empty() && !lower.starts_with("gemini") && !lower.starts_with("gemma") {
            ai_config.model = trimmed.to_string();
        } else {
            ai_config.model = DEFAULT_CHATGPT_MODEL.to_string();
        }
    }
    if let Ok(Some(gemini_model)) = repository.load_setting("ai.gemini_model") {
        let trimmed = gemini_model.trim();
        if !trimmed.is_empty() {
            ai_config.gemini_model = trimmed.to_string();
        } else {
            ai_config.gemini_model = DEFAULT_GEMINI_MODEL.to_string();
        }
    }
    if ai_config.gemini_api_key.is_none()
        && let Ok(Some(saved_key)) = vanguard_cv::credentials::load_gemini_api_key()
    {
        ai_config.gemini_api_key = Some(saved_key);
    }
    ai.configure(ai_config)?;

    Ok(Services::new(
        Arc::new(repository),
        Arc::new(PiiScrubber),
        Arc::new(pdf),
        Arc::new(ai),
        skills,
    ))
}
