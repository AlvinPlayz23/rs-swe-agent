use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        Ok(serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&path, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| format!("writing {}", path.display()))
    }

    pub fn api_key(&self) -> Result<String> {
        env::var("OPENAI_API_KEY")
            .ok()
            .or_else(|| self.api_key.clone())
            .ok_or_else(|| anyhow!("OPENAI_API_KEY is required, or run: mini-swe-agent-rs --config-set api_key YOUR_KEY"))
    }

    pub fn base_url(&self) -> String {
        env::var("OPENAI_BASE_URL")
            .ok()
            .or_else(|| self.base_url.clone())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string())
    }

    pub fn model(&self) -> String {
        env::var("MINI_SWE_MODEL")
            .ok()
            .or_else(|| self.model.clone())
            .unwrap_or_else(|| "gpt-4o-mini".to_string())
    }

    pub fn set(&mut self, key: &str, value: String) -> Result<()> {
        match key {
            "api_key" | "openai_api_key" => self.api_key = Some(value),
            "base_url" | "openai_base_url" => self.base_url = Some(value),
            "model" | "model_name" => self.model = Some(value),
            _ => {
                return Err(anyhow!(
                    "unknown config key {key:?}; use api_key, base_url, or model"
                ))
            }
        }
        Ok(())
    }
}

pub fn config_path() -> Result<PathBuf> {
    if let Ok(path) = env::var("MINI_SWE_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    let home = env::var("HOME").context("HOME is not set; set MINI_SWE_CONFIG to a config path")?;
    Ok(PathBuf::from(home).join(".config/mini-swe-agent-rs/config.json"))
}
