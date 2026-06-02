use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    ConfigCommand, QualityHint, SearchLevel, SearchProvider, read_to_string, write_file,
};

pub(crate) fn default_config_path() -> PathBuf {
    default_config_home().join(crate::DEFAULT_CONFIG_RELATIVE_PATH)
}

pub(crate) fn default_project_root() -> PathBuf {
    default_config_home().join("research/projects")
}

pub(crate) fn default_config_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config"))
        .unwrap_or_else(|| PathBuf::from(".config"))
}

pub(crate) fn expand_tilde(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" || raw.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(raw.trim_start_matches("~/"));
        }
    }
    path
}

pub(crate) fn parse_u32(key: &str, value: &str) -> Result<u32> {
    value.parse::<u32>().with_context(|| format!("invalid value for {key}: expected u32"))
}

pub(crate) fn parse_u16(key: &str, value: &str) -> Result<u16> {
    value.parse::<u16>().with_context(|| format!("invalid value for {key}: expected u16"))
}

pub(crate) fn parse_bool(key: &str, value: &str) -> Result<bool> {
    value.parse::<bool>().with_context(|| format!("invalid value for {key}: expected true or false"))
}

pub(crate) fn parse_string_list(value: &str) -> Result<Vec<String>> {
    let parsed: Value = serde_json::from_str(value)
        .with_context(|| "expected JSON array of strings, e.g. [\"a\",\"b\"]".to_string())?;
    parsed
        .as_array()
        .context("expected JSON array of strings")?
        .iter()
        .map(|item| item.as_str().map(str::to_string).context("expected string item"))
        .collect()
}

pub(crate) fn parse_path_list(value: &str) -> Result<Vec<PathBuf>> {
    Ok(parse_string_list(value)?
        .into_iter()
        .map(PathBuf::from)
        .map(expand_tilde)
        .collect())
}

pub(crate) fn unique_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::BTreeSet::new();
    paths
        .into_iter()
        .map(expand_tilde)
        .filter(|path| seen.insert(path.to_string_lossy().to_string()))
        .collect()
}

pub(crate) fn parse_value_enum<T>(value: &str) -> Result<T>
where
    T: clap::ValueEnum + Clone + Send + Sync + 'static,
{
    T::from_str(value, true).map_err(anyhow::Error::msg)
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ResearchConfig {
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub local_project: LocalProjectConfig,
    #[serde(default)]
    pub profiles: ProfileConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub timeouts: TimeoutConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub reranker: RerankerConfig,
    /// Named AI providers: e.g. "siliconflow" -> {api_url, api_key, models}
    #[serde(default)]
    pub ai_providers: HashMap<String, AiProviderEntry>,
    /// Per-role agent config: e.g. "planner" -> {provider: "siliconflow", model: "fast"}
    #[serde(default)]
    pub agents: HashMap<String, AgentRoleConfig>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub root: Option<PathBuf>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub search_provider: Option<SearchProvider>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub exa_api_key: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub code_api_key: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub code_enabled: Option<bool>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub code_auto: Option<bool>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub code_tokens_num: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub default_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub model_api_url: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub model_api_key: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub zhipu_mcp_url: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub zhipu_api_key: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub zhipu_tool: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub zhipu_content_size: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub minimax_api_key: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub minimax_api_host: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub minimax_mcp_command: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub minimax_mcp_args: Option<Vec<String>>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub kimi_api_key: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub kimi_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub local_project_paths: Option<Vec<PathBuf>>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub local_project_max_files: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub planner_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub reader_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub linker_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub writer_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub default_quality_hint: Option<QualityHint>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub default_level: Option<SearchLevel>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub max_rounds: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub max_sources: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub max_runtime_minutes: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub quick_min_accepted_sources: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub quick_max_accepted_sources: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub quick_max_rounds: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub quick_max_runtime_minutes: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub deep_min_accepted_sources: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub deep_max_accepted_sources: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub deep_max_rounds: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub deep_max_runtime_minutes: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub research_min_accepted_sources: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub research_max_accepted_sources: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub research_max_rounds: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub research_max_runtime_minutes: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub serve_host: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub serve_port: Option<u16>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct StorageConfig {
    #[serde(default)]
    pub root: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SearchConfig {
    #[serde(default)]
    pub provider: Option<SearchProvider>,
    #[serde(default)]
    pub default_level: Option<SearchLevel>,
    #[serde(default)]
    pub default_quality_hint: Option<QualityHint>,
    #[serde(default)]
    pub providers: SearchProvidersConfig,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SearchProvidersConfig {
    #[serde(default)]
    pub exa: ExaConfig,
    #[serde(default)]
    pub code: CodeConfig,
    #[serde(default)]
    pub zhipu: ZhipuConfig,
    #[serde(default)]
    pub minimax: MinimaxConfig,
    #[serde(default)]
    pub kimi: KimiConfig,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ExaConfig {
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CodeConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub auto: Option<bool>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub tokens_num: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ZhipuConfig {
    #[serde(default)]
    pub mcp_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub content_size: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MinimaxConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_host: Option<String>,
    #[serde(default)]
    pub mcp_command: Option<String>,
    #[serde(default)]
    pub mcp_args: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KimiConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AiConfig {
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub planner_model: Option<String>,
    #[serde(default)]
    pub reader_model: Option<String>,
    #[serde(default)]
    pub linker_model: Option<String>,
    #[serde(default)]
    pub writer_model: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct EmbeddingConfig {
    /// OpenAI-compatible embedding endpoint, e.g. "https://api.openai.com/v1/embeddings"
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    /// Embedding model name, e.g. "text-embedding-3-small"
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RerankerConfig {
    /// Reranker API endpoint, e.g. "https://api.siliconflow.cn/v1/rerank"
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    /// Reranker model name, e.g. "Qwen/Qwen3-Reranker-0.6B"
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AiProviderEntry {
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    /// Model aliases: e.g. {"fast": "Qwen/Qwen3-8B", "smart": "Qwen/Qwen3-32B"}
    #[serde(default)]
    pub models: HashMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AgentRoleConfig {
    /// Provider name, references ai_providers key. Falls back to "default" provider.
    #[serde(default)]
    pub provider: Option<String>,
    /// Model alias within the provider, or full model name. Falls back to provider's first model.
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LocalProjectConfig {
    #[serde(default)]
    pub paths: Option<Vec<PathBuf>>,
    #[serde(default)]
    pub max_files: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProfileConfig {
    #[serde(default)]
    pub default: ProfileBudgetConfig,
    #[serde(default)]
    pub quick: ProfileBudgetConfig,
    #[serde(default)]
    pub deep: ProfileBudgetConfig,
    #[serde(default)]
    pub research: ProfileBudgetConfig,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProfileBudgetConfig {
    #[serde(default)]
    pub min_accepted_sources: Option<u32>,
    #[serde(default)]
    pub max_accepted_sources: Option<u32>,
    #[serde(default)]
    pub max_rounds: Option<u32>,
    #[serde(default)]
    pub max_runtime_minutes: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TimeoutConfig {
    /// Timeout in seconds for MCP calls (code search, zhipu, minimax). Default: 45.
    #[serde(default = "default_mcp_timeout")]
    pub mcp_timeout_secs: u64,
    /// Timeout in seconds for AI synthesis calls. Default: 120.
    #[serde(default = "default_ai_timeout")]
    pub ai_timeout_secs: u64,
    /// Timeout in milliseconds for viewer health check. Default: 500.
    #[serde(default = "default_viewer_timeout")]
    pub viewer_timeout_ms: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            mcp_timeout_secs: default_mcp_timeout(),
            ai_timeout_secs: default_ai_timeout(),
            viewer_timeout_ms: default_viewer_timeout(),
        }
    }
}

fn default_mcp_timeout() -> u64 { 45 }
fn default_ai_timeout() -> u64 { 120 }
fn default_viewer_timeout() -> u64 { 500 }

#[derive(Clone, Debug)]
pub struct ResolvedConfig {
    pub path: PathBuf,
    pub values: ResearchConfig,
}

impl ResolvedConfig {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        #[cfg(test)]
        if path.is_none() {
            return Ok(Self {
                path: PathBuf::from("test-config.json"),
                values: ResearchConfig::default(),
            });
        }
        let path = path.map(expand_tilde).unwrap_or_else(default_config_path);
        let mut values = if path.exists() {
            serde_json::from_str(&read_to_string(&path)?)
                .with_context(|| format!("failed to parse config {}", path.display()))?
        } else {
            ResearchConfig::default()
        };
        values.normalize();
        Ok(Self { path, values })
    }

    pub fn save(&self) -> Result<()> {
        let mut values = self.values.clone();
        values.normalize();
        write_file(&self.path, &serde_json::to_string_pretty(&values)?)
    }

    pub fn run(mut self, command: ConfigCommand) -> Result<Value> {
        match command {
            ConfigCommand::Path => Ok(json!({
                "action": "config.path",
                "path": self.path,
            })),
            ConfigCommand::Show => Ok(json!({
                "action": "config.show",
                "path": self.path,
                "config": self.values,
            })),
            ConfigCommand::Set { key, value } => {
                self.values.set(&key, &value)?;
                self.save()?;
                Ok(json!({
                    "action": "config.set",
                    "path": self.path,
                    "key": key,
                    "config": self.values,
                }))
            }
            ConfigCommand::Unset { key } => {
                self.values.unset(&key)?;
                self.save()?;
                Ok(json!({
                    "action": "config.unset",
                    "path": self.path,
                    "key": key,
                    "config": self.values,
                }))
            }
        }
    }
}

// Bidirectional sync: flat legacy field ↔ nested config field.
// Pattern: if nested is None, copy from flat; always copy nested back to flat.
macro_rules! sync_field {
    ($self:expr, $nested:expr, $flat:expr) => {
        if $nested.is_none() {
            $nested = $flat.clone();
        }
        $flat = $nested.clone();
    };
}

// Generates match arms for set() — parses value to the correct type and assigns to nested field.
macro_rules! config_key_set {
    ($key:expr, $value:expr, { $( $alias:literal | $dotted:literal => $setter:expr ),+ $(,)? }) => {
        match $key {
            $( $alias | $dotted => { $setter; } )+
            _ => anyhow::bail!(
                "unknown config key: '{}'. Available top-level keys: \
                 search, ai, ai_providers, agents, embedding, reranker, timeouts, \
                 local_project, profiles, server, storage. \
                 Use 'research config list' to see all.",
                $key
            ),
        }
    };
}

// Generates match arms for unset() — sets both flat and nested fields to None.
macro_rules! config_key_unset {
    ($key:expr, $self:expr, { $( $alias:literal | $dotted:literal => $flat_path:expr, $nested_path:expr ),+ $(,)? }) => {
        match $key {
            $( $alias | $dotted => { $flat_path = None; $nested_path = None; } )+
            _ => anyhow::bail!(
                "unknown config key: '{}'. Available top-level keys: \
                 search, ai, ai_providers, agents, embedding, reranker, timeouts, \
                 local_project, profiles, server, storage. \
                 Use 'research config list' to see all.",
                $key
            ),
        }
    };
}

impl ResearchConfig {
    pub fn normalize(&mut self) {
        // storage
        sync_field!(self, self.storage.root, self.root);
        // search
        sync_field!(self, self.search.provider, self.search_provider);
        sync_field!(self, self.search.default_level, self.default_level);
        sync_field!(self, self.search.default_quality_hint, self.default_quality_hint);
        // search.providers.exa
        sync_field!(self, self.search.providers.exa.api_key, self.exa_api_key);
        // search.providers.code
        sync_field!(self, self.search.providers.code.api_key, self.code_api_key);
        sync_field!(self, self.search.providers.code.enabled, self.code_enabled);
        sync_field!(self, self.search.providers.code.auto, self.code_auto);
        sync_field!(self, self.search.providers.code.tokens_num, self.code_tokens_num);
        // search.providers.zhipu
        sync_field!(self, self.search.providers.zhipu.mcp_url, self.zhipu_mcp_url);
        sync_field!(self, self.search.providers.zhipu.api_key, self.zhipu_api_key);
        sync_field!(self, self.search.providers.zhipu.tool, self.zhipu_tool);
        sync_field!(self, self.search.providers.zhipu.content_size, self.zhipu_content_size);
        // search.providers.minimax
        sync_field!(self, self.search.providers.minimax.api_key, self.minimax_api_key);
        sync_field!(self, self.search.providers.minimax.api_host, self.minimax_api_host);
        sync_field!(self, self.search.providers.minimax.mcp_command, self.minimax_mcp_command);
        sync_field!(self, self.search.providers.minimax.mcp_args, self.minimax_mcp_args);
        // search.providers.kimi
        sync_field!(self, self.search.providers.kimi.api_key, self.kimi_api_key);
        sync_field!(self, self.search.providers.kimi.model, self.kimi_model);
        // ai
        sync_field!(self, self.ai.default_model, self.default_model);
        sync_field!(self, self.ai.api_url, self.model_api_url);
        sync_field!(self, self.ai.api_key, self.model_api_key);
        sync_field!(self, self.ai.planner_model, self.planner_model);
        sync_field!(self, self.ai.reader_model, self.reader_model);
        sync_field!(self, self.ai.linker_model, self.linker_model);
        sync_field!(self, self.ai.writer_model, self.writer_model);
        // local_project
        sync_field!(self, self.local_project.paths, self.local_project_paths);
        sync_field!(self, self.local_project.max_files, self.local_project_max_files);
        // profiles
        sync_field!(self, self.profiles.default.max_rounds, self.max_rounds);
        sync_field!(self, self.profiles.default.max_accepted_sources, self.max_sources);
        sync_field!(self, self.profiles.default.max_runtime_minutes, self.max_runtime_minutes);
        sync_field!(self, self.profiles.quick.min_accepted_sources, self.quick_min_accepted_sources);
        sync_field!(self, self.profiles.quick.max_accepted_sources, self.quick_max_accepted_sources);
        sync_field!(self, self.profiles.quick.max_rounds, self.quick_max_rounds);
        sync_field!(self, self.profiles.quick.max_runtime_minutes, self.quick_max_runtime_minutes);
        sync_field!(self, self.profiles.deep.min_accepted_sources, self.deep_min_accepted_sources);
        sync_field!(self, self.profiles.deep.max_accepted_sources, self.deep_max_accepted_sources);
        sync_field!(self, self.profiles.deep.max_rounds, self.deep_max_rounds);
        sync_field!(self, self.profiles.deep.max_runtime_minutes, self.deep_max_runtime_minutes);
        sync_field!(self, self.profiles.research.min_accepted_sources, self.research_min_accepted_sources);
        sync_field!(self, self.profiles.research.max_accepted_sources, self.research_max_accepted_sources);
        sync_field!(self, self.profiles.research.max_rounds, self.research_max_rounds);
        sync_field!(self, self.profiles.research.max_runtime_minutes, self.research_max_runtime_minutes);
        // server
        sync_field!(self, self.server.host, self.serve_host);
        sync_field!(self, self.server.port, self.serve_port);
    }

    /// Resolve an agent role to (api_url, api_key, model_name).
    /// Priority: agents.<role> -> ai_providers.<provider> -> ai fallback.
    pub fn resolve_agent(&self, role: &str) -> Option<(String, String, String)> {
        // Try agents.<role> -> ai_providers.<provider>
        if let Some(agent) = self.agents.get(role) {
            let provider_name = agent.provider.as_deref().unwrap_or("default");
            if let Some(provider) = self.ai_providers.get(provider_name) {
                let api_url = provider.api_url.as_deref()?;
                let api_key = provider.api_key.as_deref()?;
                let model = if let Some(model_alias) = &agent.model {
                    // Look up alias in provider's models map, or use as-is
                    provider.models.get(model_alias).cloned().unwrap_or_else(|| model_alias.clone())
                } else {
                    // No model specified: use first model in provider
                    provider.models.values().next().cloned()?
                };
                return Some((api_url.to_string(), api_key.to_string(), model));
            }
        }
        // Fallback: ai config (legacy)
        let api_url = self.ai.api_url.as_deref().or(self.model_api_url.as_deref())?;
        let api_key = self.ai.api_key.as_deref().or(self.model_api_key.as_deref())?;
        let model = self.ai.default_model.as_deref().or(self.default_model.as_deref())?;
        Some((api_url.to_string(), api_key.to_string(), model.to_string()))
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        config_key_set!(key, value, {
            // storage
            "root" | "storage.root" => self.storage.root = Some(expand_tilde(PathBuf::from(value))),
            // search
            "search_provider" | "search.provider" => self.search.provider = Some(parse_value_enum(value)?),
            "default_level" | "search.default_level" => self.search.default_level = Some(parse_value_enum(value)?),
            "default_quality_hint" | "search.default_quality_hint" => self.search.default_quality_hint = Some(parse_value_enum(value)?),
            // search.providers.exa
            "exa_api_key" | "search.providers.exa.api_key" => self.search.providers.exa.api_key = Some(value.to_string()),
            // search.providers.code
            "code_api_key" | "search.providers.code.api_key" => self.search.providers.code.api_key = Some(value.to_string()),
            "code_enabled" | "search.providers.code.enabled" => self.search.providers.code.enabled = Some(parse_bool(key, value)?),
            "code_auto" | "search.providers.code.auto" => self.search.providers.code.auto = Some(parse_bool(key, value)?),
            "code_tokens_num" | "search.providers.code.tokens_num" => self.search.providers.code.tokens_num = Some(parse_u32(key, value)?),
            // search.providers.zhipu
            "zhipu_mcp_url" | "search.providers.zhipu.mcp_url" => self.search.providers.zhipu.mcp_url = Some(value.to_string()),
            "zhipu_api_key" | "search.providers.zhipu.api_key" => self.search.providers.zhipu.api_key = Some(value.to_string()),
            "zhipu_tool" | "search.providers.zhipu.tool" => self.search.providers.zhipu.tool = Some(value.to_string()),
            "zhipu_content_size" | "search.providers.zhipu.content_size" => self.search.providers.zhipu.content_size = Some(value.to_string()),
            // search.providers.minimax
            "minimax_api_key" | "search.providers.minimax.api_key" => self.search.providers.minimax.api_key = Some(value.to_string()),
            "minimax_api_host" | "search.providers.minimax.api_host" => self.search.providers.minimax.api_host = Some(value.to_string()),
            "minimax_mcp_command" | "search.providers.minimax.mcp_command" => self.search.providers.minimax.mcp_command = Some(value.to_string()),
            "minimax_mcp_args" | "search.providers.minimax.mcp_args" => self.search.providers.minimax.mcp_args = Some(parse_string_list(value)?),
            // search.providers.kimi
            "kimi_api_key" | "search.providers.kimi.api_key" => self.search.providers.kimi.api_key = Some(value.to_string()),
            "kimi_model" | "search.providers.kimi.model" => self.search.providers.kimi.model = Some(value.to_string()),
            // ai
            "default_model" | "ai.default_model" => self.ai.default_model = Some(value.to_string()),
            "model_api_url" | "ai.api_url" => self.ai.api_url = Some(value.to_string()),
            "model_api_key" | "ai.api_key" => self.ai.api_key = Some(value.to_string()),
            "planner_model" | "ai.planner_model" => self.ai.planner_model = Some(value.to_string()),
            "reader_model" | "ai.reader_model" => self.ai.reader_model = Some(value.to_string()),
            "linker_model" | "ai.linker_model" => self.ai.linker_model = Some(value.to_string()),
            "writer_model" | "ai.writer_model" => self.ai.writer_model = Some(value.to_string()),
            // local_project
            "local_project_paths" | "local_project.paths" => self.local_project.paths = Some(parse_path_list(value)?),
            "local_project_max_files" | "local_project.max_files" => self.local_project.max_files = Some(parse_u32(key, value)?),
            // profiles.default
            "max_rounds" | "profiles.default.max_rounds" => self.profiles.default.max_rounds = Some(parse_u32(key, value)?),
            "max_sources" | "profiles.default.max_accepted_sources" => self.profiles.default.max_accepted_sources = Some(parse_u32(key, value)?),
            "max_runtime_minutes" | "profiles.default.max_runtime_minutes" => self.profiles.default.max_runtime_minutes = Some(parse_u32(key, value)?),
            // profiles.quick
            "quick_min_accepted_sources" | "profiles.quick.min_accepted_sources" => self.profiles.quick.min_accepted_sources = Some(parse_u32(key, value)?),
            "quick_max_accepted_sources" | "profiles.quick.max_accepted_sources" => self.profiles.quick.max_accepted_sources = Some(parse_u32(key, value)?),
            "quick_max_rounds" | "profiles.quick.max_rounds" => self.profiles.quick.max_rounds = Some(parse_u32(key, value)?),
            "quick_max_runtime_minutes" | "profiles.quick.max_runtime_minutes" => self.profiles.quick.max_runtime_minutes = Some(parse_u32(key, value)?),
            // profiles.deep
            "deep_min_accepted_sources" | "profiles.deep.min_accepted_sources" => self.profiles.deep.min_accepted_sources = Some(parse_u32(key, value)?),
            "deep_max_accepted_sources" | "profiles.deep.max_accepted_sources" => self.profiles.deep.max_accepted_sources = Some(parse_u32(key, value)?),
            "deep_max_rounds" | "profiles.deep.max_rounds" => self.profiles.deep.max_rounds = Some(parse_u32(key, value)?),
            "deep_max_runtime_minutes" | "profiles.deep.max_runtime_minutes" => self.profiles.deep.max_runtime_minutes = Some(parse_u32(key, value)?),
            // profiles.research
            "research_min_accepted_sources" | "profiles.research.min_accepted_sources" => self.profiles.research.min_accepted_sources = Some(parse_u32(key, value)?),
            "research_max_accepted_sources" | "profiles.research.max_accepted_sources" => self.profiles.research.max_accepted_sources = Some(parse_u32(key, value)?),
            "research_max_rounds" | "profiles.research.max_rounds" => self.profiles.research.max_rounds = Some(parse_u32(key, value)?),
            "research_max_runtime_minutes" | "profiles.research.max_runtime_minutes" => self.profiles.research.max_runtime_minutes = Some(parse_u32(key, value)?),
            // server
            "serve_host" | "server.host" => self.server.host = Some(value.to_string()),
            "serve_port" | "server.port" => self.server.port = Some(parse_u16(key, value)?),
            // timeouts
            "mcp_timeout_secs" | "timeouts.mcp_timeout_secs" => self.timeouts.mcp_timeout_secs = parse_u32(key, value)? as u64,
            "ai_timeout_secs" | "timeouts.ai_timeout_secs" => self.timeouts.ai_timeout_secs = parse_u32(key, value)? as u64,
            "viewer_timeout_ms" | "timeouts.viewer_timeout_ms" => self.timeouts.viewer_timeout_ms = parse_u32(key, value)? as u64,
            // embedding
            "emb_api_url" | "embedding.api_url" => self.embedding.api_url = Some(value.to_string()),
            "emb_api_key" | "embedding.api_key" => self.embedding.api_key = Some(value.to_string()),
            "emb_model" | "embedding.model" => self.embedding.model = Some(value.to_string()),
            // reranker
            "reranker_api_url" | "reranker.api_url" => self.reranker.api_url = Some(value.to_string()),
            "reranker_api_key" | "reranker.api_key" => self.reranker.api_key = Some(value.to_string()),
            "reranker_model" | "reranker.model" => self.reranker.model = Some(value.to_string())
        });
        // Handle dotted ai_providers.* and agents.* keys
        if key.starts_with("ai_providers.") {
            let rest = &key["ai_providers.".len()..];
            if let Some((provider_name, field)) = rest.split_once('.') {
                let provider = self.ai_providers.entry(provider_name.to_string()).or_default();
                if field == "api_url" {
                    provider.api_url = Some(value.to_string());
                } else if field == "api_key" {
                    provider.api_key = Some(value.to_string());
                } else if field.starts_with("models.") {
                    let alias = &field["models.".len()..];
                    provider.models.insert(alias.to_string(), value.to_string());
                } else {
                    anyhow::bail!(
                        "unknown ai_providers key: '{key}'. \
                         Available fields: api_url, api_key, models.<alias>"
                    );
                }
            } else {
                anyhow::bail!(
                    "ai_providers key must be ai_providers.<name>.<field>: '{key}'. \
                     Example: ai_providers.openai.api_key"
                );
            }
        } else if key.starts_with("agents.") {
            let rest = &key["agents.".len()..];
            if let Some((role_name, field)) = rest.split_once('.') {
                let agent = self.agents.entry(role_name.to_string()).or_default();
                if field == "provider" {
                    agent.provider = Some(value.to_string());
                } else if field == "model" {
                    agent.model = Some(value.to_string());
                } else {
                    anyhow::bail!(
                        "unknown agents key: '{key}'. \
                         Available fields: provider, model"
                    );
                }
            } else {
                anyhow::bail!("agents key must be agents.<role>.<field>: {key}");
            }
        }
        self.normalize();
        Ok(())
    }

    pub fn unset(&mut self, key: &str) -> Result<()> {
        // Timeouts use non-Option fields, handle separately from the macro
        match key {
            "mcp_timeout_secs" | "timeouts.mcp_timeout_secs" => {
                self.timeouts.mcp_timeout_secs = default_mcp_timeout();
                self.normalize();
                return Ok(());
            }
            "ai_timeout_secs" | "timeouts.ai_timeout_secs" => {
                self.timeouts.ai_timeout_secs = default_ai_timeout();
                self.normalize();
                return Ok(());
            }
            "viewer_timeout_ms" | "timeouts.viewer_timeout_ms" => {
                self.timeouts.viewer_timeout_ms = default_viewer_timeout();
                self.normalize();
                return Ok(());
            }
            // Embedding uses Option<String> fields, no legacy flat keys
            "embedding.api_url" => {
                self.embedding.api_url = None;
                self.normalize();
                return Ok(());
            }
            "embedding.api_key" => {
                self.embedding.api_key = None;
                self.normalize();
                return Ok(());
            }
            "embedding.model" => {
                self.embedding.model = None;
                self.normalize();
                return Ok(());
            }
            // Reranker uses Option<String> fields, no legacy flat keys
            "reranker.api_url" => {
                self.reranker.api_url = None;
                self.normalize();
                return Ok(());
            }
            "reranker.api_key" => {
                self.reranker.api_key = None;
                self.normalize();
                return Ok(());
            }
            "reranker.model" => {
                self.reranker.model = None;
                self.normalize();
                return Ok(());
            }
            // ai_providers and agents use nested HashMaps
            k if k.starts_with("ai_providers.") => {
                let rest = &k["ai_providers.".len()..];
                if let Some((provider_name, field)) = rest.split_once('.') {
                    if let Some(provider) = self.ai_providers.get_mut(provider_name) {
                        if field == "api_url" {
                            provider.api_url = None;
                        } else if field == "api_key" {
                            provider.api_key = None;
                        } else if field.starts_with("models.") {
                            let alias = &field["models.".len()..];
                            provider.models.remove(alias);
                        } else {
                            anyhow::bail!(
                                "unknown ai_providers key: '{k}'. \
                                 Available fields: api_url, api_key, models.<alias>"
                            );
                        }
                    }
                } else {
                    // Remove entire provider
                    self.ai_providers.remove(rest);
                }
                self.normalize();
                return Ok(());
            }
            k if k.starts_with("agents.") => {
                let rest = &k["agents.".len()..];
                if let Some((role_name, field)) = rest.split_once('.') {
                    if let Some(agent) = self.agents.get_mut(role_name) {
                        if field == "provider" {
                            agent.provider = None;
                        } else if field == "model" {
                            agent.model = None;
                        } else {
                            anyhow::bail!(
                                "unknown agents key: '{k}'. \
                                 Available fields: provider, model"
                            );
                        }
                    }
                } else {
                    // Remove entire agent
                    self.agents.remove(rest);
                }
                self.normalize();
                return Ok(());
            }
            _ => {}
        }
        config_key_unset!(key, self, {
            // storage
            "root" | "storage.root" => self.root, self.storage.root,
            // search
            "search_provider" | "search.provider" => self.search_provider, self.search.provider,
            "default_level" | "search.default_level" => self.default_level, self.search.default_level,
            "default_quality_hint" | "search.default_quality_hint" => self.default_quality_hint, self.search.default_quality_hint,
            // search.providers.exa
            "exa_api_key" | "search.providers.exa.api_key" => self.exa_api_key, self.search.providers.exa.api_key,
            // search.providers.code
            "code_api_key" | "search.providers.code.api_key" => self.code_api_key, self.search.providers.code.api_key,
            "code_enabled" | "search.providers.code.enabled" => self.code_enabled, self.search.providers.code.enabled,
            "code_auto" | "search.providers.code.auto" => self.code_auto, self.search.providers.code.auto,
            "code_tokens_num" | "search.providers.code.tokens_num" => self.code_tokens_num, self.search.providers.code.tokens_num,
            // search.providers.zhipu
            "zhipu_mcp_url" | "search.providers.zhipu.mcp_url" => self.zhipu_mcp_url, self.search.providers.zhipu.mcp_url,
            "zhipu_api_key" | "search.providers.zhipu.api_key" => self.zhipu_api_key, self.search.providers.zhipu.api_key,
            "zhipu_tool" | "search.providers.zhipu.tool" => self.zhipu_tool, self.search.providers.zhipu.tool,
            "zhipu_content_size" | "search.providers.zhipu.content_size" => self.zhipu_content_size, self.search.providers.zhipu.content_size,
            // search.providers.minimax
            "minimax_api_key" | "search.providers.minimax.api_key" => self.minimax_api_key, self.search.providers.minimax.api_key,
            "minimax_api_host" | "search.providers.minimax.api_host" => self.minimax_api_host, self.search.providers.minimax.api_host,
            "minimax_mcp_command" | "search.providers.minimax.mcp_command" => self.minimax_mcp_command, self.search.providers.minimax.mcp_command,
            "minimax_mcp_args" | "search.providers.minimax.mcp_args" => self.minimax_mcp_args, self.search.providers.minimax.mcp_args,
            // search.providers.kimi
            "kimi_api_key" | "search.providers.kimi.api_key" => self.kimi_api_key, self.search.providers.kimi.api_key,
            "kimi_model" | "search.providers.kimi.model" => self.kimi_model, self.search.providers.kimi.model,
            // ai
            "default_model" | "ai.default_model" => self.default_model, self.ai.default_model,
            "model_api_url" | "ai.api_url" => self.model_api_url, self.ai.api_url,
            "model_api_key" | "ai.api_key" => self.model_api_key, self.ai.api_key,
            "planner_model" | "ai.planner_model" => self.planner_model, self.ai.planner_model,
            "reader_model" | "ai.reader_model" => self.reader_model, self.ai.reader_model,
            "linker_model" | "ai.linker_model" => self.linker_model, self.ai.linker_model,
            "writer_model" | "ai.writer_model" => self.writer_model, self.ai.writer_model,
            // local_project
            "local_project_paths" | "local_project.paths" => self.local_project_paths, self.local_project.paths,
            "local_project_max_files" | "local_project.max_files" => self.local_project_max_files, self.local_project.max_files,
            // profiles.default
            "max_rounds" | "profiles.default.max_rounds" => self.max_rounds, self.profiles.default.max_rounds,
            "max_sources" | "profiles.default.max_accepted_sources" => self.max_sources, self.profiles.default.max_accepted_sources,
            "max_runtime_minutes" | "profiles.default.max_runtime_minutes" => self.max_runtime_minutes, self.profiles.default.max_runtime_minutes,
            // profiles.quick
            "quick_min_accepted_sources" | "profiles.quick.min_accepted_sources" => self.quick_min_accepted_sources, self.profiles.quick.min_accepted_sources,
            "quick_max_accepted_sources" | "profiles.quick.max_accepted_sources" => self.quick_max_accepted_sources, self.profiles.quick.max_accepted_sources,
            "quick_max_rounds" | "profiles.quick.max_rounds" => self.quick_max_rounds, self.profiles.quick.max_rounds,
            "quick_max_runtime_minutes" | "profiles.quick.max_runtime_minutes" => self.quick_max_runtime_minutes, self.profiles.quick.max_runtime_minutes,
            // profiles.deep
            "deep_min_accepted_sources" | "profiles.deep.min_accepted_sources" => self.deep_min_accepted_sources, self.profiles.deep.min_accepted_sources,
            "deep_max_accepted_sources" | "profiles.deep.max_accepted_sources" => self.deep_max_accepted_sources, self.profiles.deep.max_accepted_sources,
            "deep_max_rounds" | "profiles.deep.max_rounds" => self.deep_max_rounds, self.profiles.deep.max_rounds,
            "deep_max_runtime_minutes" | "profiles.deep.max_runtime_minutes" => self.deep_max_runtime_minutes, self.profiles.deep.max_runtime_minutes,
            // profiles.research
            "research_min_accepted_sources" | "profiles.research.min_accepted_sources" => self.research_min_accepted_sources, self.profiles.research.min_accepted_sources,
            "research_max_accepted_sources" | "profiles.research.max_accepted_sources" => self.research_max_accepted_sources, self.profiles.research.max_accepted_sources,
            "research_max_rounds" | "profiles.research.max_rounds" => self.research_max_rounds, self.profiles.research.max_rounds,
            "research_max_runtime_minutes" | "profiles.research.max_runtime_minutes" => self.research_max_runtime_minutes, self.profiles.research.max_runtime_minutes,
            // server
            "serve_host" | "server.host" => self.serve_host, self.server.host,
            "serve_port" | "server.port" => self.serve_port, self.server.port
        });
        self.normalize();
        Ok(())
    }
}
