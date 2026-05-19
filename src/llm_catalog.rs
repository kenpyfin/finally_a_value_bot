//! Provider/model catalog for Web UI model selection and cost reference.
//!
//! **How models are chosen:** This is a **curated static list** in code (`PROVIDER_CATALOG`), keyed
//! by `LLM_PROVIDER` from `.env` (with aliases like `gemini` → `google`). It is **not** fetched
//! live from provider APIs. When your active `LLM_MODEL` is not in the list, the API still
//! prepends it so you can keep using it (custom id or newer releases before we update the catalog).
//!
//! Pricing values are approximate list-price hints (USD per 1M tokens), not live quotes.

use std::collections::HashSet;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct CatalogModel {
    pub id: &'static str,
    /// Input USD per 1M tokens; `None` for local / unknown.
    pub input_usd_per_mtok: Option<f64>,
    /// Output USD per 1M tokens; `None` for local / unknown.
    pub output_usd_per_mtok: Option<f64>,
    /// Short tier for UI badges: free, low, standard, high, premium.
    pub cost_tier: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct CatalogProvider {
    pub id: &'static str,
    pub label: &'static str,
    pub default_base_url: &'static str,
    pub models: &'static [CatalogModel],
}

macro_rules! catalog_model {
    ($id:expr, $in:expr, $out:expr, $tier:expr) => {
        CatalogModel {
            id: $id,
            input_usd_per_mtok: $in,
            output_usd_per_mtok: $out,
            cost_tier: $tier,
        }
    };
}

pub const PROVIDER_CATALOG: &[CatalogProvider] = &[
    CatalogProvider {
        id: "openai",
        label: "OpenAI",
        default_base_url: "https://api.openai.com/v1",
        models: &[
            catalog_model!("gpt-5.2", Some(1.75), Some(14.0), "high"),
            catalog_model!("gpt-5", Some(1.25), Some(10.0), "standard"),
            catalog_model!("gpt-5-mini", Some(0.25), Some(2.0), "low"),
        ],
    },
    CatalogProvider {
        id: "openrouter",
        label: "OpenRouter",
        default_base_url: "https://openrouter.ai/api/v1",
        models: &[
            catalog_model!("openrouter/auto", None, None, "standard"),
            catalog_model!(
                "anthropic/claude-sonnet-4.5",
                Some(3.0),
                Some(15.0),
                "standard"
            ),
            catalog_model!("openai/gpt-5.2", Some(1.75), Some(14.0), "high"),
        ],
    },
    CatalogProvider {
        id: "anthropic",
        label: "Anthropic",
        default_base_url: "",
        models: &[
            catalog_model!("claude-opus-4-7", Some(5.0), Some(25.0), "premium"),
            catalog_model!(
                "claude-opus-4-6-20260205",
                Some(15.0),
                Some(75.0),
                "premium"
            ),
            catalog_model!("claude-sonnet-4-6", Some(3.0), Some(15.0), "standard"),
            catalog_model!(
                "claude-sonnet-4-5-20250929",
                Some(3.0),
                Some(15.0),
                "standard"
            ),
            catalog_model!("claude-haiku-4-5-20251001", Some(1.0), Some(5.0), "low"),
        ],
    },
    CatalogProvider {
        id: "ollama",
        label: "Ollama (local)",
        default_base_url: "http://127.0.0.1:11434/v1",
        models: &[
            catalog_model!("llama3.2", None, None, "free"),
            catalog_model!("qwen2.5-coder:7b", None, None, "free"),
            catalog_model!("mistral", None, None, "free"),
        ],
    },
    CatalogProvider {
        id: "llama",
        label: "Llama.cpp (local)",
        default_base_url: "http://127.0.0.1:8080/v1",
        models: &[catalog_model!("local", None, None, "free")],
    },
    CatalogProvider {
        id: "google",
        label: "Google (Gemini API)",
        default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        models: &[
            catalog_model!("gemini-3.1-pro-preview", Some(2.0), Some(12.0), "high"),
            catalog_model!("gemini-3-flash-preview", Some(0.50), Some(3.0), "standard"),
            catalog_model!("gemini-3.1-flash-lite", Some(0.25), Some(1.50), "low"),
            catalog_model!("gemini-2.5-pro", Some(1.25), Some(10.0), "high"),
            catalog_model!("gemini-2.5-flash", Some(0.30), Some(2.50), "low"),
            catalog_model!("gemini-2.5-flash-lite", Some(0.10), Some(0.40), "low"),
        ],
    },
    CatalogProvider {
        id: "alibaba",
        label: "Alibaba Cloud (Qwen / DashScope)",
        default_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        models: &[
            catalog_model!("qwen3-max", Some(1.20), Some(6.0), "standard"),
            catalog_model!("qwen-max-latest", Some(1.60), Some(6.4), "standard"),
        ],
    },
    CatalogProvider {
        id: "deepseek",
        label: "DeepSeek",
        default_base_url: "https://api.deepseek.com/v1",
        models: &[
            catalog_model!("deepseek-chat", Some(0.28), Some(0.42), "low"),
            catalog_model!("deepseek-reasoner", Some(0.55), Some(2.19), "standard"),
        ],
    },
    CatalogProvider {
        id: "moonshot",
        label: "Moonshot AI (Kimi)",
        default_base_url: "https://api.moonshot.cn/v1",
        models: &[
            catalog_model!("kimi-k2.5", Some(0.60), Some(3.0), "standard"),
            catalog_model!("kimi-k2", Some(0.40), Some(2.0), "low"),
        ],
    },
    CatalogProvider {
        id: "mistral",
        label: "Mistral AI",
        default_base_url: "https://api.mistral.ai/v1",
        models: &[
            catalog_model!("mistral-large-latest", Some(2.0), Some(6.0), "high"),
            catalog_model!("ministral-8b-latest", Some(0.10), Some(0.10), "low"),
        ],
    },
    CatalogProvider {
        id: "azure",
        label: "Microsoft Azure AI",
        default_base_url:
            "https://YOUR-RESOURCE.openai.azure.com/openai/deployments/YOUR-DEPLOYMENT",
        models: &[
            catalog_model!("gpt-5.2", Some(1.75), Some(14.0), "high"),
            catalog_model!("gpt-5", Some(1.25), Some(10.0), "standard"),
        ],
    },
    CatalogProvider {
        id: "bedrock",
        label: "Amazon AWS Bedrock",
        default_base_url: "https://bedrock-runtime.YOUR-REGION.amazonaws.com/openai/v1",
        models: &[
            catalog_model!(
                "anthropic.claude-opus-4-6-v1",
                Some(15.0),
                Some(75.0),
                "premium"
            ),
            catalog_model!(
                "anthropic.claude-sonnet-4-5-v2",
                Some(3.0),
                Some(15.0),
                "standard"
            ),
        ],
    },
    CatalogProvider {
        id: "zhipu",
        label: "Zhipu AI (GLM / Z.AI)",
        default_base_url: "https://open.bigmodel.cn/api/paas/v4",
        models: &[
            catalog_model!("glm-4.7", Some(0.50), Some(2.0), "low"),
            catalog_model!("glm-4.7-flash", Some(0.10), Some(0.40), "low"),
        ],
    },
    CatalogProvider {
        id: "minimax",
        label: "MiniMax",
        default_base_url: "https://api.minimax.io/v1",
        models: &[catalog_model!(
            "MiniMax-M2.1",
            Some(0.30),
            Some(1.20),
            "low"
        )],
    },
    CatalogProvider {
        id: "cohere",
        label: "Cohere",
        default_base_url: "https://api.cohere.ai/compatibility/v1",
        models: &[
            catalog_model!("command-a-03-2025", Some(2.50), Some(10.0), "high"),
            catalog_model!("command-r-plus-08-2024", Some(2.50), Some(10.0), "high"),
        ],
    },
    CatalogProvider {
        id: "tencent",
        label: "Tencent AI Lab",
        default_base_url: "https://api.hunyuan.cloud.tencent.com/v1",
        models: &[
            catalog_model!("hunyuan-t1-latest", Some(1.0), Some(4.0), "standard"),
            catalog_model!("hunyuan-turbos-latest", Some(0.30), Some(1.0), "low"),
        ],
    },
    CatalogProvider {
        id: "xai",
        label: "xAI",
        default_base_url: "https://api.x.ai/v1",
        models: &[
            catalog_model!("grok-4", Some(3.0), Some(15.0), "high"),
            catalog_model!("grok-3", Some(2.0), Some(10.0), "standard"),
        ],
    },
    CatalogProvider {
        id: "huggingface",
        label: "Hugging Face",
        default_base_url: "https://router.huggingface.co/v1",
        models: &[
            catalog_model!("Qwen/Qwen3-Coder-Next", None, None, "standard"),
            catalog_model!("meta-llama/Llama-3.3-70B-Instruct", None, None, "standard"),
        ],
    },
    CatalogProvider {
        id: "together",
        label: "Together AI",
        default_base_url: "https://api.together.xyz/v1",
        models: &[
            catalog_model!(
                "deepseek-ai/DeepSeek-V3",
                Some(1.25),
                Some(1.25),
                "standard"
            ),
            catalog_model!(
                "meta-llama/Llama-3.3-70B-Instruct-Turbo",
                Some(0.88),
                Some(0.88),
                "low"
            ),
        ],
    },
    CatalogProvider {
        id: "custom",
        label: "Custom (manual config)",
        default_base_url: "",
        models: &[catalog_model!("custom-model", None, None, "standard")],
    },
];

pub const APP_SETTING_LLM_MODEL: &str = "LLM_MODEL";

/// Normalize `.env` `LLM_PROVIDER` to a catalog entry id (`gemini` → `google`, etc.).
pub fn resolve_catalog_provider_id(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "gemini" => "google".to_string(),
        other => other.to_string(),
    }
}

pub fn find_provider(provider: &str) -> Option<&'static CatalogProvider> {
    let p = resolve_catalog_provider_id(provider);
    PROVIDER_CATALOG
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(&p))
}

pub fn default_model_for_provider(provider: &str) -> &'static str {
    find_provider(provider)
        .and_then(|p| p.models.first().map(|m| m.id))
        .unwrap_or("custom-model")
}

/// Whether `model` is listed for `provider`, or is the active custom model for unknown providers.
pub fn model_allowed_for_provider(provider: &str, model: &str, allow_custom: bool) -> bool {
    let model = model.trim();
    if model.is_empty() {
        return false;
    }
    if allow_custom {
        return true;
    }
    if let Some(preset) = find_provider(provider) {
        if preset.models.iter().any(|m| m.id == model) {
            return true;
        }
    }
    false
}

pub fn format_cost_summary(model: &CatalogModel) -> String {
    match (model.input_usd_per_mtok, model.output_usd_per_mtok) {
        (None, None) if model.cost_tier == "free" => "Local — no API usage cost".to_string(),
        (None, None) => "Pricing varies — check your provider dashboard".to_string(),
        (Some(i), Some(o)) => format!("~${i:.2} / ${o:.2} per 1M input / output tokens"),
        (Some(i), None) => format!("~${i:.2} per 1M input tokens"),
        (None, Some(o)) => format!("~${o:.2} per 1M output tokens"),
    }
}

#[derive(Debug, Serialize)]
pub struct CatalogModelJson {
    pub id: String,
    pub input_usd_per_mtok: Option<f64>,
    pub output_usd_per_mtok: Option<f64>,
    pub cost_tier: String,
    pub cost_summary: String,
    /// True when this row was added because it is the active model but not in the curated list.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub from_active_config: bool,
}

impl CatalogModelJson {
    pub fn from_model(m: &CatalogModel) -> Self {
        CatalogModelJson {
            id: m.id.to_string(),
            input_usd_per_mtok: m.input_usd_per_mtok,
            output_usd_per_mtok: m.output_usd_per_mtok,
            cost_tier: m.cost_tier.to_string(),
            cost_summary: format_cost_summary(m),
            from_active_config: false,
        }
    }

    pub fn from_active_config(model_id: &str) -> Self {
        CatalogModelJson {
            id: model_id.to_string(),
            input_usd_per_mtok: None,
            output_usd_per_mtok: None,
            cost_tier: "standard".to_string(),
            cost_summary:
                "Active model from your config — not in curated catalog; check provider pricing."
                    .to_string(),
            from_active_config: true,
        }
    }
}

/// Curated models for `provider`, plus `active_model` when it is missing from the list.
pub fn catalog_models_json(provider: &str, active_model: &str) -> Vec<CatalogModelJson> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let active = active_model.trim();
    if !active.is_empty() {
        let in_preset = find_provider(provider)
            .map(|p| p.models.iter().any(|m| m.id == active))
            .unwrap_or(false);
        if !in_preset {
            seen.insert(active.to_string());
            out.push(CatalogModelJson::from_active_config(active));
        }
    }
    if let Some(p) = find_provider(provider) {
        for m in p.models {
            if seen.insert(m.id.to_string()) {
                out.push(CatalogModelJson::from_model(m));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_catalog_has_sonnet() {
        let p = find_provider("anthropic").expect("anthropic preset");
        assert!(p.models.iter().any(|m| m.id.contains("sonnet")));
    }

    #[test]
    fn gemini_provider_alias_uses_google_catalog() {
        let models = catalog_models_json("gemini", "");
        assert!(models.iter().any(|m| m.id == "gemini-3-flash-preview"));
    }

    #[test]
    fn active_model_prepended_when_not_in_catalog() {
        let models = catalog_models_json("google", "my-experimental-gemini-model");
        assert!(models.first().is_some_and(|m| m.from_active_config));
        assert!(models.iter().any(|m| m.id == "gemini-3-flash-preview"));
    }

    #[test]
    fn model_allowed_rejects_unknown_for_known_provider() {
        assert!(!model_allowed_for_provider(
            "anthropic",
            "not-a-real-model",
            false
        ));
        assert!(model_allowed_for_provider(
            "anthropic",
            "claude-sonnet-4-5-20250929",
            false
        ));
    }
}
