//! Architecture rules (docs/architecture.md §4 and §7), checked on every test run and in CI.

use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

use cargo_metadata::{DependencyKind, MetadataCommand};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Workspace crate name -> its normal (non-dev, non-build) workspace dependencies, plus its manifest folder.
fn workspace_graph() -> HashMap<String, (BTreeSet<String>, PathBuf)> {
    let metadata = MetadataCommand::new()
        .manifest_path(workspace_root().join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata");
    let members: BTreeSet<String> = metadata
        .workspace_packages()
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    metadata
        .workspace_packages()
        .into_iter()
        .map(|p| {
            let deps = p
                .dependencies
                .iter()
                .filter(|d| d.kind == DependencyKind::Normal)
                .map(|d| d.name.clone())
                .filter(|name| members.contains(name))
                .collect();
            let dir = p
                .manifest_path
                .parent()
                .unwrap()
                .as_std_path()
                .to_path_buf();
            (p.name.to_string(), (deps, dir))
        })
        .collect()
}

fn is_module(dir: &Path) -> bool {
    dir.parent().is_some_and(|p| p.ends_with("modules"))
}

#[test]
fn crate_dependencies_point_inward() {
    let graph = workspace_graph();
    let modules: BTreeSet<&String> = graph
        .iter()
        .filter(|(_, (_, dir))| is_module(dir))
        .map(|(name, _)| name)
        .collect();
    assert!(
        !modules.is_empty(),
        "modules were not found under crates/modules"
    );
    let mut problems = Vec::new();
    for (name, (deps, dir)) in &graph {
        let forbidden: Vec<&String> = if is_module(dir) {
            // A module may use the core and other modules' public interfaces (Cargo rules out cycles),
            // never the adapters or the composition root.
            deps.iter()
                .filter(|d| d.as_str() != "toolkit-core" && !modules.contains(d))
                .collect()
        } else if name == "toolkit-core" {
            deps.iter().collect()
        } else if name == "integrations" {
            deps.iter()
                .filter(|d| d.as_str() != "toolkit-core")
                .collect()
        } else {
            Vec::new()
        };
        for dep in forbidden {
            problems.push(format!("{name} must not depend on {dep}"));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// `domain/` stays free of storage and web frameworks, including derives and full paths.
#[test]
fn domain_folders_do_not_mention_sqlx_or_axum() {
    let mut problems = Vec::new();
    let mut stack = vec![workspace_root().join("crates")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.components().any(|c| c.as_os_str() == "domain")
            {
                let text = fs::read_to_string(&path).unwrap();
                for word in ["sqlx", "axum"] {
                    if text.contains(word) {
                        problems.push(format!("{} mentions {word}", path.display()));
                    }
                }
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// External platform SDKs live only in `integrations`; modules and `core` get internal events instead.
#[test]
fn external_sdks_stay_in_integrations() {
    const SDKS: [&str; 2] = ["obws", "tokio-tungstenite"];
    let metadata = MetadataCommand::new()
        .manifest_path(workspace_root().join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata");
    let problems: Vec<String> = metadata
        .workspace_packages()
        .into_iter()
        .filter(|p| p.name.as_str() != "integrations")
        .flat_map(|p| {
            p.dependencies
                .iter()
                .filter(|d| d.kind == DependencyKind::Normal && SDKS.contains(&d.name.as_str()))
                .map(move |d| format!("{} must not depend on {}", p.name, d.name))
        })
        .collect();
    assert!(problems.is_empty(), "{problems:#?}");
}
