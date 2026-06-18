//! Typed configuration and validation.

use serde::Deserialize;

use crate::error::Error;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub backend: BackendConfig,
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackendConfig {
    pub base_url: String,
    pub api_key_env: String,
    #[serde(default = "default_backend_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub name: String,
    pub router: RouterConfig,
    pub panel: PanelConfig,
    pub aggregator: AggregatorConfig,
    #[serde(default)]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    #[serde(default = "default_router_policy")]
    pub policy: String,
    pub single_model: String,
    /// The cheap model used to score query difficulty when `policy` is `"classifier"`.
    ///
    /// Required when `policy = "classifier"`; validated by [`Config::validate`].
    /// Ignored for all other policies.
    #[serde(default)]
    pub classifier_model: Option<String>,
    /// Fuse when the difficulty score is greater than or equal to this value (range 0.0..=1.0).
    ///
    /// A score below the threshold routes to the single model; a score at or above it
    /// triggers the full panel-judge-synthesis path. Defaults to `0.5`.
    #[serde(default = "default_threshold")]
    pub threshold: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PanelConfig {
    pub members: Vec<String>,
    #[serde(default)]
    pub self_moa: bool,
    #[serde(default = "default_samples")]
    pub samples: usize,
    pub min_quorum: usize,
    #[serde(default = "default_panel_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AggregatorConfig {
    pub judge: String,
    pub synthesizer: String,
    /// Whether to anonymize panel responses before passing them to the judge and synthesizer.
    ///
    /// In M1 this flag is always effectively true: panel responses reach the aggregator
    /// without any model-identity header, so there is nothing to de-anonymize regardless
    /// of this setting. The flag is reserved for a future mode where per-source attribution
    /// is forwarded and needs to be explicitly stripped.
    #[serde(default = "default_true")]
    pub anonymize_sources: bool,
    #[serde(default = "default_true")]
    pub normalize_length: bool,
    /// Whether to include an explicit "do not let any single response dominate" instruction
    /// in the judge and synthesis prompts.
    ///
    /// When true, the instruction is appended to the base hardening text that is always
    /// present. When false, the base hardening (bias/incorrectness warnings, critical
    /// evaluation) still applies, but the single-source-dominance sentence is omitted.
    /// Defaults to true.
    #[serde(default = "default_true")]
    pub single_source_cap: bool,
    #[serde(default = "default_layers")]
    pub layers: usize,
    #[serde(default = "default_max_reference_chars")]
    pub max_reference_chars: usize,
}

fn default_max_concurrent() -> usize {
    64
}
fn default_backend_timeout_ms() -> u64 {
    120_000
}
fn default_panel_timeout_ms() -> u64 {
    90_000
}
fn default_router_policy() -> String {
    "always_fuse".into()
}
fn default_threshold() -> f32 {
    0.5
}
fn default_samples() -> usize {
    3
}
fn default_true() -> bool {
    true
}
fn default_layers() -> usize {
    1
}
fn default_max_reference_chars() -> usize {
    8_000
}

impl Config {
    /// Validate every profile: loop guard, quorum bounds, unique names, single layer.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Config`] if any profile fails validation.
    pub fn validate(&self) -> Result<(), Error> {
        let mut seen = std::collections::HashSet::new();
        for p in &self.profiles {
            if !seen.insert(p.name.as_str()) {
                return Err(Error::Config(format!("duplicate profile name: {}", p.name)));
            }
            for model in p.all_models() {
                if model.starts_with("fusion/") {
                    return Err(Error::Config(format!(
                        "profile {}: model {model} references a fusion alias (loop)",
                        p.name
                    )));
                }
            }
            let n = p.panel.members.len();
            if n == 0 {
                return Err(Error::Config(format!("profile {}: empty panel", p.name)));
            }
            if p.panel.min_quorum < 1 || p.panel.min_quorum > n {
                return Err(Error::Config(format!(
                    "profile {}: min_quorum {} out of range 1..={n}",
                    p.name, p.panel.min_quorum
                )));
            }
            if p.aggregator.layers != 1 {
                return Err(Error::Config(format!(
                    "profile {}: multi-layer is not implemented yet (layers must be 1)",
                    p.name
                )));
            }
            match p.router.policy.as_str() {
                "always_fuse" => {}
                "classifier" => {
                    if p.router.classifier_model.is_none() {
                        return Err(Error::Config(format!(
                            "profile {}: classifier policy requires router.classifier_model",
                            p.name
                        )));
                    }
                }
                other => {
                    return Err(Error::Config(format!(
                        "profile {}: unknown router policy {other}",
                        p.name
                    )));
                }
            }
            if !(0.0..=1.0).contains(&p.router.threshold) {
                return Err(Error::Config(format!(
                    "profile {}: router.threshold {} out of range 0.0..=1.0",
                    p.name, p.router.threshold
                )));
            }
        }
        Ok(())
    }

    /// Look up a profile by name.
    #[must_use]
    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }
}

impl Profile {
    /// Every model id referenced by this profile, for the loop guard.
    fn all_models(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.router.single_model.as_str())
            .chain(self.router.classifier_model.as_deref())
            .chain(self.panel.members.iter().map(String::as_str))
            .chain(std::iter::once(self.aggregator.judge.as_str()))
            .chain(std::iter::once(self.aggregator.synthesizer.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, members: Vec<&str>, quorum: usize) -> Profile {
        Profile {
            name: name.into(),
            router: RouterConfig {
                policy: "always_fuse".into(),
                single_model: "b/s".into(),
                classifier_model: None,
                threshold: 0.5,
            },
            panel: PanelConfig {
                members: members.into_iter().map(Into::into).collect(),
                self_moa: false,
                samples: 3,
                min_quorum: quorum,
                timeout_ms: 90_000,
            },
            aggregator: AggregatorConfig {
                judge: "b/j".into(),
                synthesizer: "b/syn".into(),
                anonymize_sources: true,
                normalize_length: true,
                single_source_cap: true,
                layers: 1,
                max_reference_chars: 8_000,
            },
            tools: vec![],
        }
    }

    fn cfg(profiles: Vec<Profile>) -> Config {
        Config {
            server: ServerConfig {
                bind: "0.0.0.0:8080".into(),
                max_concurrent_requests: 64,
            },
            backend: BackendConfig {
                base_url: "http://b/v1".into(),
                api_key_env: "K".into(),
                timeout_ms: 120_000,
            },
            profiles,
        }
    }

    #[test]
    fn valid_config_passes() {
        let c = cfg(vec![profile("research", vec!["b/a", "b/b"], 2)]);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn rejects_fusion_alias_in_panel() {
        let c = cfg(vec![profile("research", vec!["fusion/research", "b/b"], 2)]);
        assert!(matches!(c.validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_quorum_out_of_range() {
        let c = cfg(vec![profile("research", vec!["b/a", "b/b"], 3)]);
        assert!(matches!(c.validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_duplicate_names() {
        let c = cfg(vec![
            profile("research", vec!["b/a", "b/b"], 2),
            profile("research", vec!["b/a", "b/b"], 2),
        ]);
        assert!(matches!(c.validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_multi_layer() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.aggregator.layers = 2;
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }

    #[test]
    fn classifier_policy_requires_model() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.policy = "classifier".into();
        p.router.classifier_model = None;
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }

    #[test]
    fn classifier_policy_with_model_passes() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.policy = "classifier".into();
        p.router.classifier_model = Some("b/cheap".into());
        assert!(cfg(vec![p]).validate().is_ok());
    }

    #[test]
    fn rejects_unknown_router_policy() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.policy = "magic".into();
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_threshold_out_of_range() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.threshold = 1.5;
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_fusion_alias_in_classifier_model() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.policy = "classifier".into();
        p.router.classifier_model = Some("fusion/research".into());
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }
}
