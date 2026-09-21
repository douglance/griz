use crate::{Package, Violation, deps, relative_path};
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeMap,
    fs,
    fs::DirEntry,
    path::{Path, PathBuf},
};
use toml::Value;

pub fn workspace_packages(root: &Path) -> Result<Vec<Package>> {
    let root_manifest = root.join("Cargo.toml");
    let root_value = read_manifest(&root_manifest)?;
    let mut manifests = Vec::new();
    if root_value.get("package").is_some() {
        manifests.push(root_manifest.clone());
    }
    for member in workspace_members(root, &root_value)? {
        manifests.push(member.join("Cargo.toml"));
    }
    manifests.sort();
    manifests.dedup();

    manifests
        .into_iter()
        .map(|manifest_path| {
            let manifest_path = manifest_path.canonicalize().unwrap_or(manifest_path);
            let value = read_manifest(&manifest_path)?;
            let name = value
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(Value::as_str)
                .with_context(|| {
                    format!("manifest has no package name: {}", manifest_path.display())
                })?
                .to_owned();
            let fallback_root = manifest_path
                .parent()
                .with_context(|| format!("manifest has no parent: {}", manifest_path.display()))?
                .to_path_buf();
            let package_root = manifest_path
                .parent()
                .with_context(|| format!("manifest has no parent: {}", manifest_path.display()))?
                .canonicalize()
                .unwrap_or(fallback_root);
            Ok(Package {
                name,
                manifest_path,
                root: package_root,
            })
        })
        .collect()
}

pub fn check_manifests(root: &Path, packages: &[Package]) -> Result<Vec<Violation>> {
    let mut violations = Vec::new();
    let values = packages
        .iter()
        .map(|package| Ok((package.name.clone(), read_manifest(&package.manifest_path)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;

    for package in packages {
        if !values
            .get(&package.name)
            .is_some_and(inherits_workspace_lints)
        {
            violations.push(Violation::new(
                relative_path(root, &package.manifest_path),
                0,
                "crate manifest must contain [lints] workspace = true",
            ));
        }
    }

    violations.extend(deps::check_workspace_dependencies(root, packages, &values));
    Ok(violations)
}

fn read_manifest(path: &Path) -> Result<Value> {
    let source =
        fs::read_to_string(path).with_context(|| format!("read manifest {}", path.display()))?;
    source
        .parse::<Value>()
        .with_context(|| format!("parse manifest {}", path.display()))
}

fn workspace_members(root: &Path, value: &Value) -> Result<Vec<PathBuf>> {
    let Some(members) = value
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let mut expanded = Vec::new();
    for member in members {
        let pattern = member
            .as_str()
            .context("workspace member must be a string")?;
        expanded.extend(expand_member(root, pattern)?);
    }
    Ok(expanded)
}

fn expand_member(root: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    if let Some(prefix) = pattern.strip_suffix("/*") {
        return expand_directory_members(&root.join(prefix));
    }
    if pattern.contains('*') {
        bail!("workspace member glob `{pattern}` is not supported by policy checker");
    }
    Ok(vec![root.join(pattern)])
}

fn expand_directory_members(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(dir)
        .with_context(|| format!("read workspace member glob {}", dir.display()))?;
    entries
        .map(|entry| member_dir(&entry?))
        .filter_map(Result::transpose)
        .collect()
}

fn member_dir(entry: &DirEntry) -> Result<Option<PathBuf>> {
    let path = entry.path();
    let is_member = entry.file_type()?.is_dir() && path.join("Cargo.toml").exists();
    Ok(is_member.then_some(path))
}

fn inherits_workspace_lints(value: &Value) -> bool {
    value
        .get("lints")
        .and_then(|lints| lints.get("workspace"))
        .and_then(Value::as_bool)
        == Some(true)
}
