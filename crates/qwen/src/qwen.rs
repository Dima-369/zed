use anyhow::Result;
use serde::{Deserialize, Serialize};
use strum::EnumIter;

#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, EnumIter)]
pub enum Model {
    #[serde(rename = "qwen3-coder-plus")]
    #[default]
    Qwen3CoderPlus,
    #[serde(rename = "qwen3-coder-flash")]
    Qwen3CoderFlash,
    #[serde(rename = "custom")]
    Custom {
        name: String,
        /// The name displayed in the UI, such as in the assistant panel model dropdown menu.
        display_name: Option<String>,
        max_tokens: u64,
        max_output_tokens: Option<u64>,
        max_completion_tokens: Option<u64>,
        supports_images: Option<bool>,
        supports_tools: Option<bool>,
        parallel_tool_calls: Option<bool>,
    },
}

impl Model {
    pub fn default_fast() -> Self {
        Self::Qwen3CoderFlash
    }

    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "qwen3-coder-plus" => Ok(Self::Qwen3CoderPlus),
            "qwen3-coder-flash" => Ok(Self::Qwen3CoderFlash),
            _ => anyhow::bail!("invalid model id '{id}'"),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Qwen3CoderPlus => "qwen3-coder-plus",
            Self::Qwen3CoderFlash => "qwen3-coder-flash",
            Self::Custom { name, .. } => name,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Qwen3CoderPlus => "Qwen3 Coder Plus",
            Self::Qwen3CoderFlash => "Qwen3 Coder Flash",
            Self::Custom {
                name, display_name, ..
            } => display_name.as_ref().unwrap_or(name),
        }
    }

    pub fn max_token_count(&self) -> u64 {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => 1_000_000,
            Self::Custom { max_tokens, .. } => *max_tokens,
        }
    }

    pub fn max_output_tokens(&self) -> Option<u64> {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => Some(8_192),
            Self::Custom {
                max_output_tokens, ..
            } => *max_output_tokens,
        }
    }

    pub fn supports_parallel_tool_calls(&self) -> bool {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => true,
            Self::Custom {
                parallel_tool_calls: Some(support),
                ..
            } => *support,
            Self::Custom { .. } => false,
        }
    }

    pub fn supports_prompt_cache_key(&self) -> bool {
        false
    }

    pub fn supports_tool(&self) -> bool {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => true,
            Self::Custom {
                supports_tools: Some(support),
                ..
            } => *support,
            Self::Custom { .. } => false,
        }
    }

    pub fn supports_images(&self) -> bool {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => false,
            Self::Custom {
                supports_images: Some(support),
                ..
            } => *support,
            Self::Custom { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_from_id() {
        assert_eq!(
            Model::from_id("qwen3-coder-plus").unwrap(),
            Model::Qwen3CoderPlus
        );
        assert_eq!(
            Model::from_id("qwen3-coder-flash").unwrap(),
            Model::Qwen3CoderFlash
        );
        assert!(Model::from_id("invalid").is_err());
    }

    #[test]
    fn test_model_display_names() {
        assert_eq!(Model::Qwen3CoderPlus.display_name(), "Qwen3 Coder Plus");
        assert_eq!(Model::Qwen3CoderFlash.display_name(), "Qwen3 Coder Flash");
    }

    #[test]
    fn test_model_max_tokens() {
        assert_eq!(Model::Qwen3CoderPlus.max_token_count(), 1_000_000);
        assert_eq!(Model::Qwen3CoderFlash.max_token_count(), 1_000_000);
    }
}
