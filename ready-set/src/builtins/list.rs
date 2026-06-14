//! `ready-set --list`.
//!
//! Walks PATH, resolves metadata via the cache → sidecar → `__describe`
//! waterfall, prints results in human or JSON.

use std::collections::BTreeSet;
use std::ffi::OsString;

use ready_set_sdk::ExitCode;
use ready_set_sdk::OutputMode;
use ready_set_sdk::describe::Platform;
use serde::Serialize;

use crate::cache::PluginCache;
use crate::discovery::list_all;
use crate::env::EnvContract;
use crate::metadata::resolve_metadata;

/// One row of `--list` output.
#[derive(Debug, Serialize)]
struct Row {
    kind: &'static str,
    name: String,
    description: Option<String>,
    version: Option<String>,
    stability: Option<String>,
    binary_path: Option<String>,
    platforms: Option<Vec<String>>,
}

/// Run `ready-set --list`.
pub fn run(args: &[OsString], contract: &EnvContract) -> ExitCode {
    let all = args.iter().any(|a| a == "--all");

    let mut rows: Vec<Row> = ["ready", "set", "go", "help", "version", "list"]
        .into_iter()
        .map(builtin_row)
        .collect();

    let cache_path = PluginCache::default_path();
    let mut cache = cache_path
        .as_deref()
        .map_or_else(PluginCache::default, PluginCache::load);
    let mut cache_dirty = false;

    let plugins = list_all();
    let here = Platform::current();
    for entry in plugins {
        let manifest = resolve_metadata(&entry, &mut cache);
        if manifest.is_some() {
            cache_dirty = true;
        }
        if let Some(m) = manifest.as_ref()
            && !all
            && here.is_some_and(|p| !m.platforms.contains(&p))
        {
            continue;
        }
        rows.push(Row {
            kind: "plugin",
            name: entry.name.clone(),
            description: manifest.as_ref().map(|m| m.description.clone()),
            version: manifest.as_ref().map(|m| m.version.to_string()),
            stability: manifest.as_ref().map(|m| match m.stability {
                ready_set_sdk::describe::Stability::Stable => "stable".into(),
                ready_set_sdk::describe::Stability::Experimental => "experimental".into(),
                ready_set_sdk::describe::Stability::Deprecated => "deprecated".into(),
            }),
            binary_path: Some(entry.binary_path.display().to_string()),
            platforms: manifest.as_ref().map(|m| {
                m.platforms
                    .iter()
                    .map(|p| match p {
                        Platform::Linux => "linux".into(),
                        Platform::Macos => "macos".into(),
                        Platform::Windows => "windows".into(),
                    })
                    .collect()
            }),
        });
        if let Some(m) = manifest.as_ref() {
            let mut seen_aliases = BTreeSet::new();
            for alias in &m.command_aliases {
                if !seen_aliases.insert(alias.name.clone()) {
                    continue;
                }
                rows.push(Row {
                    kind: "alias",
                    name: alias.name.clone(),
                    description: Some(alias.description.clone()),
                    version: Some(m.version.to_string()),
                    stability: Some(match m.stability {
                        ready_set_sdk::describe::Stability::Stable => "stable".into(),
                        ready_set_sdk::describe::Stability::Experimental => "experimental".into(),
                        ready_set_sdk::describe::Stability::Deprecated => "deprecated".into(),
                    }),
                    binary_path: Some(entry.binary_path.display().to_string()),
                    platforms: Some(
                        m.platforms
                            .iter()
                            .map(|p| match p {
                                Platform::Linux => "linux".into(),
                                Platform::Macos => "macos".into(),
                                Platform::Windows => "windows".into(),
                            })
                            .collect(),
                    ),
                });
            }
        }
    }

    if cache_dirty && let Some(path) = cache_path.as_deref() {
        drop(cache.save(path));
    }

    match contract.output {
        OutputMode::Json => match serde_json::to_string(&rows) {
            Ok(s) => {
                println!("{s}");
                ExitCode::Ok
            },
            Err(err) => {
                eprintln!("ready-set: failed to serialize --list output: {err}");
                ExitCode::SystemError
            },
        },
        OutputMode::Human => {
            print_human(&rows);
            ExitCode::Ok
        },
    }
}

fn builtin_row(name: &str) -> Row {
    let description = match name {
        "ready" => "Diagnose product capabilities and show the readiness matrix.",
        "set" => "Configure or reconcile required product capabilities.",
        "go" => "Execute provider-backed capability workflows.",
        "help" => "Print dispatcher help.",
        "version" => "Print the dispatcher version.",
        "list" => "List built-in and discovered plugin subcommands.",
        _ => unreachable!("unknown built-in row"),
    };

    Row {
        kind: "builtin",
        name: name.into(),
        description: Some(description.into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        stability: Some("stable".into()),
        binary_path: None,
        platforms: Some(vec!["linux".into(), "macos".into(), "windows".into()]),
    }
}

fn print_human(rows: &[Row]) {
    let name_width = rows.iter().map(|r| r.name.len()).max().unwrap_or(8).max(8);
    println!(
        "{:<width$}  KIND     DESCRIPTION",
        "NAME",
        width = name_width
    );
    for row in rows {
        let desc = row
            .description
            .as_deref()
            .unwrap_or("(metadata unavailable)");
        println!(
            "{:<width$}  {:<8} {desc}",
            row.name,
            row.kind,
            width = name_width
        );
    }
}
