//! Core capability registry and readiness matrix renderers.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use ready_set_sdk::config::{Config, load_config};
use ready_set_sdk::describe::Platform;
use ready_set_sdk::manifest::Manifest;
use ready_set_sdk::{
    CapabilityDescriptor, CapabilityId, CapabilityRelevance, CapabilityReport, CapabilityState,
    CapabilityVerb, ProviderId, Result,
};

use crate::cache::PluginCache;
use crate::discovery::list_all;
use crate::metadata::resolve_metadata;

/// One effective capability row after applying project configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredCapability {
    /// Stable capability id.
    pub id: CapabilityId,
    /// Human label for matrix and help output.
    pub title: String,
    /// Effective provider id.
    pub provider: ProviderId,
    /// Supported lifecycle verbs.
    pub verbs: Vec<CapabilityVerb>,
    /// Effective product relevance.
    pub relevance: CapabilityRelevance,
}

impl RegisteredCapability {
    fn from_descriptor(descriptor: CapabilityDescriptor, config: Option<&Config>) -> Self {
        let capability_config = config.and_then(|cfg| cfg.capabilities.get(descriptor.id.as_str()));
        let relevance = capability_config
            .and_then(|cfg| cfg.relevance)
            .unwrap_or(descriptor.default_relevance);
        let provider = capability_config
            .and_then(|cfg| cfg.provider.clone())
            .unwrap_or(descriptor.provider);

        Self {
            id: descriptor.id,
            title: descriptor.title,
            provider,
            verbs: descriptor.verbs,
            relevance,
        }
    }
}

/// Sorted collection of registered capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRegistry {
    capabilities: Vec<RegisteredCapability>,
}

impl CapabilityRegistry {
    /// Build a registry from optional config and already-resolved plugin
    /// manifests.
    ///
    /// Plugin capabilities with duplicate ids keep the first discovered
    /// descriptor unless `.ready-set.toml` selects a later provider.
    pub fn from_parts(
        config: Option<&Config>,
        plugin_manifests: impl IntoIterator<Item = Manifest>,
    ) -> Self {
        let mut descriptors: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();

        for manifest in plugin_manifests {
            for descriptor in manifest.capabilities {
                let id = descriptor.id.as_str().to_owned();
                match descriptors.entry(id) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(descriptor);
                    },
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        let selected = provider_override(config, entry.key())
                            .is_some_and(|provider| provider == &descriptor.provider);
                        if selected {
                            entry.insert(descriptor);
                        }
                    },
                }
            }
        }

        let capabilities = descriptors
            .into_values()
            .map(|descriptor| RegisteredCapability::from_descriptor(descriptor, config))
            .collect();

        Self { capabilities }
    }

    /// Discover project config and installed plugins, then build a registry.
    ///
    /// # Errors
    ///
    /// Returns config loading errors from `.ready-set.toml` parsing or I/O.
    pub fn discover(cwd: &Path) -> Result<Self> {
        let config = load_config(cwd)?;
        let mut cache = PluginCache::default_path()
            .as_deref()
            .map_or_else(PluginCache::default, PluginCache::load);
        let current_platform = Platform::current();
        let mut manifests = Vec::new();

        for entry in list_all() {
            let Some(manifest) = resolve_metadata(&entry, &mut cache) else {
                continue;
            };
            if current_platform.is_some_and(|platform| !manifest.platforms.contains(&platform)) {
                continue;
            }
            manifests.push(manifest);
        }

        Ok(Self::from_parts(config.as_ref(), manifests))
    }

    /// Borrow the sorted registered capabilities.
    #[must_use]
    pub fn capabilities(&self) -> &[RegisteredCapability] {
        &self.capabilities
    }

    /// Render unevaluated placeholder reports for every registered capability.
    #[must_use]
    pub fn reports_unevaluated(&self) -> Vec<CapabilityReport> {
        self.capabilities
            .iter()
            .map(|capability| {
                let (state, summary) = match capability.relevance {
                    CapabilityRelevance::NotNeeded => {
                        (CapabilityState::NotNeeded, "capability marked not needed")
                    },
                    CapabilityRelevance::Required | CapabilityRelevance::Optional => {
                        (CapabilityState::Blocked, "readiness not evaluated yet")
                    },
                };
                CapabilityReport {
                    id: capability.id.clone(),
                    title: capability.title.clone(),
                    provider: capability.provider.clone(),
                    state,
                    relevance: capability.relevance,
                    summary: summary.into(),
                    next_action: None,
                }
            })
            .collect()
    }
}

/// Render capability reports as a human-readable readiness matrix.
#[must_use]
pub fn render_human_matrix(reports: &[CapabilityReport]) -> String {
    let capability_width = reports
        .iter()
        .map(|report| report.id.as_str().len())
        .max()
        .unwrap_or(0)
        .max("capability".len());
    let state_width = reports
        .iter()
        .map(|report| state_label(report.state).len())
        .max()
        .unwrap_or(0)
        .max("state".len());
    let action_width = reports
        .iter()
        .map(|report| {
            report
                .next_action
                .as_ref()
                .map_or(0, |action| action.command.len())
        })
        .max()
        .unwrap_or(0)
        .max("next action".len());

    let mut out = String::new();
    writeln!(
        &mut out,
        "{:<capability_width$}  {:<state_width$}  {:<action_width$}  summary",
        "capability", "state", "next action"
    )
    .expect("writing to a string cannot fail");
    for report in reports {
        let next_action = report
            .next_action
            .as_ref()
            .map_or("", |action| action.command.as_str());
        writeln!(
            &mut out,
            "{:<capability_width$}  {:<state_width$}  {:<action_width$}  {}",
            report.id.as_str(),
            state_label(report.state),
            next_action,
            report.summary
        )
        .expect("writing to a string cannot fail");
    }
    out
}

/// Render capability reports as JSON using the SDK report shape.
///
/// # Errors
///
/// Returns a JSON serialization error if a report cannot be serialized.
pub fn render_json_matrix(reports: &[CapabilityReport]) -> Result<String> {
    serde_json::to_string(reports).map_err(Into::into)
}

fn provider_override<'a>(config: Option<&'a Config>, id: &str) -> Option<&'a ProviderId> {
    config?
        .capabilities
        .get(id)
        .and_then(|capability| capability.provider.as_ref())
}

const fn state_label(state: CapabilityState) -> &'static str {
    match state {
        CapabilityState::Ready => "ready",
        CapabilityState::Missing => "missing",
        CapabilityState::Incomplete => "incomplete",
        CapabilityState::Blocked => "blocked",
        CapabilityState::Stale => "stale",
        CapabilityState::Optional => "optional",
        CapabilityState::NotNeeded => "not-needed",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use ready_set_sdk::config::{CapabilityConfig, ProjectMeta};
    use ready_set_sdk::describe::{Platform, Stability};

    use super::*;

    fn config_with(
        overrides: impl IntoIterator<
            Item = (
                &'static str,
                Option<CapabilityRelevance>,
                Option<&'static str>,
            ),
        >,
    ) -> Config {
        let capabilities = overrides
            .into_iter()
            .map(|(id, relevance, provider)| {
                (
                    id.to_string(),
                    CapabilityConfig {
                        relevance,
                        provider: provider.map(ProviderId::from),
                        unknown_keys: Vec::new(),
                    },
                )
            })
            .collect();

        Config {
            path: PathBuf::from(".ready-set.toml"),
            ready_set: ProjectMeta {
                schema_version: 2,
                profile: "rust-workspace".into(),
            },
            capabilities,
            plugins: BTreeMap::new(),
            unknown_keys: Vec::new(),
        }
    }

    fn plugin_manifest(capabilities: Vec<CapabilityDescriptor>) -> Manifest {
        Manifest {
            description: "Plugin".into(),
            version: "0.1.0".parse().unwrap(),
            stability: Stability::Stable,
            min_dispatcher_version: "0.1.0".parse().unwrap(),
            platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
            requires_cargo_workspace: false,
            capabilities,
        }
    }

    fn plugin_descriptor(id: &str, title: &str, provider: &str) -> CapabilityDescriptor {
        plugin_descriptor_with_relevance(id, title, provider, CapabilityRelevance::Required)
    }

    fn plugin_descriptor_with_relevance(
        id: &str,
        title: &str,
        provider: &str,
        default_relevance: CapabilityRelevance,
    ) -> CapabilityDescriptor {
        CapabilityDescriptor {
            id: id.into(),
            title: title.into(),
            provider: provider.into(),
            verbs: vec![CapabilityVerb::Ready, CapabilityVerb::Set],
            default_relevance,
        }
    }

    fn ids(registry: &CapabilityRegistry) -> Vec<&str> {
        registry
            .capabilities()
            .iter()
            .map(|capability| capability.id.as_str())
            .collect()
    }

    #[test]
    fn empty_registry_contains_no_core_capabilities() {
        let registry = CapabilityRegistry::from_parts(None, Vec::<Manifest>::new());

        assert!(registry.capabilities().is_empty());
    }

    #[test]
    fn config_only_unknown_capability_ids_are_not_registered() {
        let config = config_with([(
            "unknown",
            Some(CapabilityRelevance::Required),
            Some("missing-provider"),
        )]);
        let registry = CapabilityRegistry::from_parts(Some(&config), Vec::<Manifest>::new());

        assert!(registry.capabilities().is_empty());
    }

    #[test]
    fn registry_output_is_sorted_by_capability_id() {
        let manifest = plugin_manifest(vec![
            plugin_descriptor("zzz", "Zzz", "plugin"),
            plugin_descriptor("aaa", "Aaa", "plugin"),
        ]);
        let registry = CapabilityRegistry::from_parts(None, [manifest]);

        assert_eq!(ids(&registry), vec!["aaa", "zzz"]);
    }

    #[test]
    fn config_relevance_override_changes_effective_relevance() {
        let config = config_with([("linting", Some(CapabilityRelevance::Optional), None)]);
        let manifest = plugin_manifest(vec![plugin_descriptor("linting", "Linting", "rust")]);
        let registry = CapabilityRegistry::from_parts(Some(&config), [manifest]);
        let linting = registry
            .capabilities()
            .iter()
            .find(|capability| capability.id.as_str() == "linting")
            .unwrap();

        assert_eq!(linting.relevance, CapabilityRelevance::Optional);
    }

    #[test]
    fn config_provider_override_changes_effective_provider() {
        let config = config_with([("formatting", None, Some("external-formatting"))]);
        let manifest = plugin_manifest(vec![plugin_descriptor("formatting", "Formatting", "rust")]);
        let registry = CapabilityRegistry::from_parts(Some(&config), [manifest]);
        let formatting = registry
            .capabilities()
            .iter()
            .find(|capability| capability.id.as_str() == "formatting")
            .unwrap();

        assert_eq!(formatting.provider.as_str(), "external-formatting");
    }

    #[test]
    fn unique_plugin_capability_descriptors_are_preserved() {
        let manifest = plugin_manifest(vec![plugin_descriptor_with_relevance(
            "security",
            "Security",
            "scan",
            CapabilityRelevance::Optional,
        )]);
        let registry = CapabilityRegistry::from_parts(None, [manifest]);
        let security = registry
            .capabilities()
            .iter()
            .find(|capability| capability.id.as_str() == "security")
            .unwrap();

        assert_eq!(security.title, "Security");
        assert_eq!(security.provider.as_str(), "scan");
        assert_eq!(security.relevance, CapabilityRelevance::Optional);
    }

    #[test]
    fn duplicate_plugin_capability_keeps_first_without_provider_override() {
        let first = plugin_manifest(vec![plugin_descriptor("linting", "First linting", "first")]);
        let second = plugin_manifest(vec![plugin_descriptor(
            "linting",
            "Second linting",
            "second",
        )]);
        let registry = CapabilityRegistry::from_parts(None, [first, second]);
        let linting = registry
            .capabilities()
            .iter()
            .find(|capability| capability.id.as_str() == "linting")
            .unwrap();

        assert_eq!(linting.title, "First linting");
        assert_eq!(linting.provider.as_str(), "first");
    }

    #[test]
    fn duplicate_plugin_capability_is_used_when_provider_override_selects_it() {
        let config = config_with([("linting", None, Some("second"))]);
        let first = plugin_manifest(vec![plugin_descriptor("linting", "First linting", "first")]);
        let second = plugin_manifest(vec![plugin_descriptor(
            "linting",
            "Second linting",
            "second",
        )]);
        let registry = CapabilityRegistry::from_parts(Some(&config), [first, second]);
        let linting = registry
            .capabilities()
            .iter()
            .find(|capability| capability.id.as_str() == "linting")
            .unwrap();

        assert_eq!(linting.title, "Second linting");
        assert_eq!(linting.provider.as_str(), "second");
    }

    #[test]
    fn json_matrix_round_trips_as_capability_reports() {
        let manifest = plugin_manifest(vec![plugin_descriptor("linting", "Linting", "rust")]);
        let reports = CapabilityRegistry::from_parts(None, [manifest]).reports_unevaluated();
        let json = render_json_matrix(&reports).unwrap();
        let round_tripped: Vec<CapabilityReport> = serde_json::from_str(&json).unwrap();

        assert_eq!(round_tripped, reports);
    }

    #[test]
    fn human_matrix_contains_expected_columns() {
        let manifest = plugin_manifest(vec![plugin_descriptor("linting", "Linting", "rust")]);
        let reports = CapabilityRegistry::from_parts(None, [manifest]).reports_unevaluated();
        let human = render_human_matrix(&reports);

        assert!(human.contains("capability"));
        assert!(human.contains("state"));
        assert!(human.contains("next action"));
        assert!(human.contains("summary"));
    }

    #[test]
    fn not_needed_relevance_produces_not_needed_placeholder_report() {
        let config = config_with([("workspace", Some(CapabilityRelevance::NotNeeded), None)]);
        let manifest = plugin_manifest(vec![plugin_descriptor("workspace", "Workspace", "rust")]);
        let reports =
            CapabilityRegistry::from_parts(Some(&config), [manifest]).reports_unevaluated();
        let workspace = reports
            .iter()
            .find(|report| report.id.as_str() == "workspace")
            .unwrap();

        assert_eq!(workspace.state, CapabilityState::NotNeeded);
        assert_eq!(workspace.summary, "capability marked not needed");
        assert!(workspace.next_action.is_none());
    }
}
