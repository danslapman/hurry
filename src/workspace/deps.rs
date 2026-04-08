use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::{error, info, warn};
use tower_lsp::{Client, lsp_types::Url};

use crate::progress;
use crate::symbols::index::WorkspaceIndex;
use crate::workspace::scanner::index_file;

const PROGRESS_TOKEN: &str = "hurry/deps";

/// Unpack dependency sources for a Stack project into `<workspace>/.dep-srcs/`,
/// then index the new source files. Already-unpacked packages (recorded in
/// `.dep-srcs/.resolved.list`) are skipped, making this incremental.
///
/// Silently returns if `stack.yaml` is not found at the workspace root.
pub async fn unpack_dep_sources(workspace: &Path, index: Arc<WorkspaceIndex>, client: Option<&Client>) {
    if !workspace.join("stack.yaml").exists() {
        return;
    }

    info!("hurry: collecting Stack dependencies for {}", workspace.display());
    progress::begin(client, PROGRESS_TOKEN, "hurry", Some("Resolving Stack dependencies...".into())).await;

    // 1. Resolve current transitive deps
    let new_deps = match fetch_dep_srcs(workspace).await {
        Some(d) => d,
        None => {
            progress::end(client, PROGRESS_TOKEN, Some("Failed to resolve dependencies".into())).await;
            return;
        }
    };
    info!("hurry: {} external dependencies found", new_deps.len());

    // 2. Ensure .dep-srcs/ exists
    let dep_srcs = workspace.join(".dep-srcs");
    if let Err(e) = tokio::fs::create_dir_all(&dep_srcs).await {
        error!("hurry: failed to create {}: {e}", dep_srcs.display());
        return;
    }

    // 3. Read manifest
    let resolved_file = dep_srcs.join(".resolved.list");
    let old_deps: HashSet<String> = tokio::fs::read_to_string(&resolved_file)
        .await
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();

    // 4. Diff
    let to_remove: HashSet<String> = old_deps.difference(&new_deps).cloned().collect();
    let to_add: Vec<String> = {
        let mut v: Vec<String> = new_deps.difference(&old_deps).cloned().collect();
        v.sort();
        v
    };

    info!(
        "hurry: {} deps to add, {} deps to remove",
        to_add.len(),
        to_remove.len()
    );

    if to_add.is_empty() && to_remove.is_empty() {
        // Nothing changed on disk, but the in-memory index is empty on every server
        // restart — re-index the already-unpacked packages before returning.
        let existing: Vec<String> = old_deps.into_iter().collect();
        if !existing.is_empty() {
            progress::report(client, PROGRESS_TOKEN, format!("Indexing sources for {} packages...", existing.len())).await;
            scan_package_dirs(&dep_srcs, &existing, index).await;
        }
        progress::end(client, PROGRESS_TOKEN, Some("Dependencies up to date".into())).await;
        return;
    }

    // 5. Cleanup stale
    if !to_remove.is_empty() {
        progress::report(client, PROGRESS_TOKEN, format!("Removing {} stale packages...", to_remove.len())).await;
        cleanup_stale(&dep_srcs, &to_remove, &index).await;
    }

    // 6. Unpack new packages
    let newly_added = if !to_add.is_empty() {
        progress::report(client, PROGRESS_TOKEN, format!("Downloading {} packages...", to_add.len())).await;
        unpack_packages(&dep_srcs, &to_add).await
    } else {
        vec![]
    };

    // 7. Index newly unpacked files
    if !newly_added.is_empty() {
        progress::report(client, PROGRESS_TOKEN, format!("Indexing sources for {} packages...", newly_added.len())).await;
        scan_package_dirs(&dep_srcs, &newly_added, index.clone()).await;
    }

    // 8. Update manifest
    if !to_remove.is_empty() || !newly_added.is_empty() {
        let mut manifest: Vec<String> = old_deps
            .difference(&to_remove)
            .cloned()
            .chain(newly_added.iter().cloned())
            .collect();
        manifest.sort();
        manifest.dedup();
        let content = manifest.join("\n") + "\n";
        if let Err(e) = tokio::fs::write(&resolved_file, content).await {
            warn!("hurry: failed to write {}: {e}", resolved_file.display());
        }
    }

    info!("hurry: dependency source indexing complete");
    progress::end(client, PROGRESS_TOKEN, Some("Dependency sources ready".into())).await;
}

/// Run `stack ls dependencies --filter '$locals'` and return `name-version` strings.
/// Filters out `rts-*` (GHC built-in, not on Hackage).
async fn fetch_dep_srcs(workspace: &Path) -> Option<HashSet<String>> {
    let output = tokio::process::Command::new("stack")
        .args(["ls", "dependencies", "--filter", "$locals"])
        .current_dir(workspace)
        .output()
        .await
        .map_err(|e| error!("hurry: failed to spawn stack: {e}"))
        .ok()?;

    if !output.status.success() {
        error!(
            "hurry: stack ls dependencies failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let deps = parse_dep_srcs(&stdout);
    if deps.is_empty() {
        warn!("hurry: stack ls dependencies returned no packages — output format may have changed");
    }
    Some(deps)
}

/// Parse `stack ls dependencies` output (`"<name> <version>"` per line).
/// Returns `name-version` strings, filtering out `rts-*`.
fn parse_dep_srcs(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(2, ' ');
            let name = parts.next()?;
            let version = parts.next()?.trim();
            if name.is_empty() || version.is_empty() {
                return None;
            }
            if name.starts_with("rts") {
                return None;
            }
            Some(format!("{name}-{version}"))
        })
        .collect()
}

/// Evict index entries for stale packages and delete their directories.
async fn cleanup_stale(dep_srcs: &Path, to_remove: &HashSet<String>, index: &WorkspaceIndex) {
    for pkg in to_remove {
        let pkg_dir = dep_srcs.join(pkg);
        if !pkg_dir.exists() {
            continue;
        }
        // Evict from index before deleting from disk
        let files = {
            let dir = pkg_dir.clone();
            tokio::task::spawn_blocking(move || {
                let mut out = vec![];
                collect_hs_recursive(&dir, &mut out);
                out
            })
            .await
            .unwrap_or_default()
        };
        for path in &files {
            if let Ok(uri) = Url::from_file_path(path) {
                index.remove_file(&uri);
            }
        }
        if let Err(e) = tokio::fs::remove_dir_all(&pkg_dir).await {
            warn!("hurry: failed to remove {}: {e}", pkg_dir.display());
        } else {
            info!("hurry: removed stale dep sources for {pkg}");
        }
    }
}

/// Unpack packages via a single `stack unpack` call.
/// Returns only the packages whose directories actually landed on disk,
/// so partial network failures don't corrupt the manifest.
async fn unpack_packages(dep_srcs: &Path, packages: &[String]) -> Vec<String> {
    info!("hurry: unpacking {} packages...", packages.len());

    let mut cmd = tokio::process::Command::new("stack");
    cmd.arg("unpack").arg("--to").arg(dep_srcs);
    for pkg in packages {
        cmd.arg(pkg);
    }

    match cmd.output().await {
        Ok(out) if !out.status.success() => {
            warn!(
                "hurry: stack unpack exited {:?}:\n{}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .take(20)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        Err(e) => {
            error!("hurry: failed to spawn stack unpack: {e}");
            return vec![];
        }
        _ => {}
    }

    // Check which packages actually landed on disk regardless of exit code
    let dep_srcs = dep_srcs.to_path_buf();
    let packages = packages.to_vec();
    tokio::task::spawn_blocking(move || {
        packages
            .into_iter()
            .filter(|pkg| dep_srcs.join(pkg).is_dir())
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Index `.hs`/`.lhs` files in the given package directories concurrently.
async fn scan_package_dirs(dep_srcs: &Path, packages: &[String], index: Arc<WorkspaceIndex>) {
    let dirs: Vec<PathBuf> = packages.iter().map(|p| dep_srcs.join(p)).collect();
    let files = tokio::task::spawn_blocking(move || {
        let mut out = vec![];
        for dir in &dirs {
            collect_hs_recursive(dir, &mut out);
        }
        out
    })
    .await
    .unwrap_or_default();

    info!("hurry: indexing {} source files from new deps", files.len());

    let sem = Arc::new(tokio::sync::Semaphore::new(64));
    let mut handles = vec![];
    for path in files {
        let index = index.clone();
        let permit = sem.clone().acquire_owned().await.unwrap();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            index_file(&path, &index).await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }
}

fn collect_hs_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_hs_recursive(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("hs" | "lhs")
        ) {
            out.push(path);
        }
    }
}
