//! Provider/model catalog for Web UI model selection and cost reference.
//! Pricing values are approximate list-price hints (USD per 1M tokens), not live quotes.

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

const M: fn(&str, Option<f64>, Option<f64>, &str) = |id, inp, out, tier| CatalogModel {
    id,
    input_usd_per_mtok: inp,
    output_usd_per_mtok: out,
    cost_tier: tier,
};

pub const PROVIDER_CATALOG: &[CatalogProvider] = &[
    CatalogProvider {
        id: "openai",
        label: "OpenAI",
        default_base_url: "https://api.openai.com/v1",
        models: &[
            M("gpt-5.2", Some(1.75), Some(14.0), "high"),
            M("gpt-5", Some(1.25), Some(10.0), "standard"),
            M("gpt-5-mini", Some(0.25), Some(2.0), "low"),
        ],
    },
    CatalogProvider {
        id: "openrouter",
        label: "OpenRouter",
        default_base_url: "https://openrouter.ai/api/v1",
        models: &[
            M("openrouter/auto", None, None, "standard"),
            M("anthropic/claude-sonnet-4.5", Some(3.0), Some(15.0), "standard"),
            M("openai/gpt-5.2", Some(1.75), Some(14.0), "high"),
        ],
    },
    CatalogProvider {
        id: "anthropic",
        label: "Anthropic",
        default_base_url: "",
        models: &[
            M(
                "claude-sonnet-4-5-20250929",
                Some(3.0),
                Some(15.0),
                "standard",
            ),
            M("claude-opus-4-6-20260205", Some(15.0), Some(75.0), "premium"),
        ],
    },
    CatalogProvider {
        id: "ollama",
        label: "Ollama (local)",
        default_base_url: "http://127.0.0.1:11434/v1",
        models: &[
            M("llama3.2", None, None, "free"),
            M("qwen2.5-coder:7b", None, None, "free"),
            M("mistral", None, None, "free"),
        ],
    },
    CatalogProvider {
        id: "llama",
        label: "Llama.cpp (local)",
        default_base_url: "http://127.0.0.1:8080/v1",
        models: &[M("local", None, None, "free")],
    },
    CatalogProvider {
        id: "google",
        label: "Google DeepMind",
        default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        models: &[
            M("gemini-2.5-pro", Some(1.25), Some(10.0), "high"),
            M("gemini-2.5-flash", Some(0.30), Some(2.5), "low"),
        ],
    },
    CatalogProvider {
        id: "alibaba",
        label: "Alibaba Cloud (Qwen / DashScope)",
        default_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        models: &[
            M("qwen3-max", Some(1.20), Some(6.0), "standard"),
            M("qwen-max-latest", Some(1.60), Some(6.4), "standard"),
        ],
    },
    CatalogProvider {
        id: "deepseek",
        label: "DeepSeek",
        default_base_url: "https://api.deepseek.com/v1",
        models: &[
            M("deepseek-chat", Some(0.28), Some(0.42), "low"),
            M("deepseek-reasoner", Some(0.55), Some(2.19), "standard"),
        ],
    },
    CatalogProvider {
        id: "moonshot",
        label: "Moonshot AI (Kimi)",
        default_base_url: "https://api.moonshot.cn/v1",
        models: &[
            M("kimi-k2.5", Some(0.60), Some(3.0), "standard"),
            M("kimi-k2", Some(0.40), Some(2.0), "low"),
        ],
    },
    CatalogProvider {
        id: "mistral",
        label: "Mistral AI",
        default_base_url: "https://api.mistral.ai/v1",
        models: &[
            M("mistral-large-latest", Some(2.0), Some(6.0), "high"),
            M("ministral-8b-latest", Some(0.10), Some(0.10), "low"),
        ],
    },
    CatalogProvider {
        id: "azure",
        label: "Microsoft Azure AI",
        default_base_url:
            "https://YOUR-RESOURCE.openai.azure.com/openai/deployments/YOUR-DEPLOYMENT",
        models: &[
            M("gpt-5.2", Some(1.75), Some(14.0), "high"),
            M("gpt-5", Some(1.25), Some(10.0), "standard"),
        ],
    },
    CatalogProvider {
        id: "bedrock",
        label: "Amazon AWS Bedrock",
        default_base_url: "https://bedrock-runtime.YOUR-REGION.amazonaws.com/openai/v1",
        models: &[
            M("anthropic.claude-opus-4-6-v1", Some(15.0), Some(75.0), "premium"),
            M("anthropic.claude-sonnet-4-5-v2", Some(3.0), Some(15.0), "standard"),
        ],
    },
    CatalogProvider {
        id: "zhipu",
        label: "Zhipu AI (GLM / Z.AI)",
        default_base_url: "https://open.bigmodel.cn/api/paas/v4",
        models: &[
            M("glm-4.7", Some(0.50), Some(2.0), "low"),
            M("glm-4.7-flash", Some(0.10), Some(0.40), "low"),
        ],
    },
    CatalogProvider {
        id: "minimax",
        label: "MiniMax",
        default_base_url: "https://api.minimax.io/v1",
        models: &[M("MiniMax-M2.1", Some(0.30), Some(1.20), "low")],
    },
    CatalogProvider {
        id: "cohere",
        label: "Cohere",
        default_base_url: "https://api.cohere.ai/compatibility/v1",
        models: &[
            M("command-a-03-2025", Some(2.50), Some(10.0), "high"),
            M("command-r-plus-08-2024", Some(2.50), Some(10.0), "high"),
        ],
    },
    CatalogProvider {
        id: "tencent",
        label: "Tencent AI Lab",
        default_base_url: "https://api.hunyuan.cloud.tencent.com/v1",
        models: &[
            M("hunyuan-t1-latest", Some(1.0), Some(4.0), "standard"),
            M("hunyuan-turbos-latest", Some(0.30), Some(1.0), "low"),
        ],
    },
    CatalogProvider {
        id: "xai",
        label: "xAI",
        default_base_url: "https://api.x.ai/v1",
        models: &[
            M("grok-4", Some(3.0), Some(15.0), "high"),
            M("grok-3", Some(2.0), Some(10.0), "standard"),
        ],
    },
    CatalogProvider {
        id: "huggingface",
        label: "Hugging Face",
        default_base_url: "https://router.huggingface.co/v1",
        models: &[
            M("Qwen/Qwen3-Coder-Next", None, None, "standard"),
            M("meta-llama/Llama-3.3-70B-Instruct", None, None, "standard"),
        ],
    },
    CatalogProvider {
        id: "together",
        label: "Together AI",
        default_base_url: "https://api.together.xyz/v1",
        models: &[
            M("deepseek-ai/DeepSeek-V3", Some(1.25), Some(1.25), "standard"),
            M(
                "meta-llama/Llama-3.3-70B-Instruct-Turbo",
                Some(0.88),
                Some(0.88),
                "low",
            ),
        ],
    },
    CatalogProvider {
        id: "custom",
        label: "Custom (manual config)",
        default_base_url: "",
        models: &[M("custom-model", None, None, "standard")],
    },
];

pub const APP_SETTING_LLM_MODEL: &str = "LLM_MODEL";

pub fn find_provider(provider: &str) -> Option<&'static CatalogProvider> {
    let p = provider.trim().to_ascii_lowercase();
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
}

impl CatalogModelJson {
    pub fn from_model(m: &CatalogModel) -> Self {
        CatalogModelJson {
            id: m.id.to_string(),
            input_usd_per_mtok: m.input_usd_per_mtok,
            output_usd_per_mtok: m.output_usd_per_mtok,
            cost_tier: m.cost_tier.to_string(),
            cost_summary: format_cost_summary(m),
        }
    }
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
