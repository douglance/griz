use crate::{Package, Violation, relative_path};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use toml::Value;

const ALLOWED_WORKSPACE_DEPS: &[(&str, &str)] = &[
    ("griz", "griz-core"),
    ("griz", "griz-store"),
    ("griz-store", "griz-core"),
];

pub fn check_workspace_dependencies(
    root: &Path,
    packages: &[Package],
    values: &BTreeMap<String, Value>,
) -> Vec<Violation> {
    let graph = workspace_dependency_graph(packages, values);
    let mut violations = check_dependency_whitelist(root, packages, &graph);
    violations.extend(check_cycles(root, packages, &graph));
    violations
}

fn workspace_dependency_graph(
    packages: &[Package],
    values: &BTreeMap<String, Value>,
) -> BTreeMap<String, BTreeSet<String>> {
    let names: BTreeSet<_> = packages
        .iter()
        .map(|package| package.name.clone())
        .collect();
    let roots: BTreeMap<_, _> = packages
        .iter()
        .map(|package| (package.root.clone(), package.name.clone()))
        .collect();
    packages
        .iter()
        .map(|package| {
            let edges = package_edges(package, values.get(&package.name), &names, &roots);
            (package.name.clone(), edges)
        })
        .collect()
}

fn package_edges(
    package: &Package,
    value: Option<&Value>,
    names: &BTreeSet<String>,
    roots: &BTreeMap<PathBuf, String>,
) -> BTreeSet<String> {
    let mut edges = BTreeSet::new();
    for deps in dependency_tables(value) {
        for (dep_name, dep_value) in deps {
            insert_name_edge(dep_name, names, &mut edges);
            insert_path_edge(package, dep_value, roots, &mut edges);
        }
    }
    edges
}

fn dependency_tables(value: Option<&Value>) -> Vec<&toml::map::Map<String, Value>> {
    let Some(value) = value else {
        return Vec::new();
    };
    ["dependencies", "dev-dependencies", "build-dependencies"]
        .into_iter()
        .filter_map(|section| value.get(section).and_then(Value::as_table))
        .collect()
}

fn insert_name_edge(dep_name: &str, names: &BTreeSet<String>, edges: &mut BTreeSet<String>) {
    if names.contains(dep_name) {
        edges.insert(dep_name.to_owned());
    }
}

fn insert_path_edge(
    package: &Package,
    dep_value: &Value,
    roots: &BTreeMap<PathBuf, String>,
    edges: &mut BTreeSet<String>,
) {
    let Some(path) = dep_value.get("path").and_then(Value::as_str) else {
        return;
    };
    let dep_manifest = package.root.join(path).join("Cargo.toml");
    let dep_root = dep_manifest
        .parent()
        .and_then(|path| path.canonicalize().ok());
    if let Some(name) = dep_root.and_then(|root| roots.get(&root)) {
        edges.insert(name.clone());
    }
}

fn check_dependency_whitelist(
    root: &Path,
    packages: &[Package],
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<Violation> {
    let manifest_by_name: BTreeMap<_, _> = packages
        .iter()
        .map(|package| (&package.name, &package.manifest_path))
        .collect();
    graph
        .iter()
        .flat_map(|(from, deps)| {
            let from = from.clone();
            let from_for_filter = from.clone();
            let manifest = manifest_by_name[&from].clone();
            deps.iter()
                .filter(move |to| !is_allowed_workspace_dep(&from_for_filter, to))
                .map(move |to| {
                    Violation::new(
                        relative_path(root, &manifest),
                        0,
                        format!("workspace dependency `{from} -> {to}` is not whitelisted"),
                    )
                })
        })
        .collect()
}

fn is_allowed_workspace_dep(from: &str, to: &str) -> bool {
    ALLOWED_WORKSPACE_DEPS.contains(&(from, to))
}

fn check_cycles(
    root: &Path,
    packages: &[Package],
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<Violation> {
    let manifests: BTreeMap<_, _> = packages
        .iter()
        .map(|package| (package.name.clone(), package.manifest_path.clone()))
        .collect();
    let mut detector = CycleDetector {
        root,
        graph,
        manifests,
        violations: Vec::new(),
    };
    for package in packages {
        let mut stack = Vec::new();
        let mut seen = BTreeSet::new();
        detector.find(&package.name, &package.name, &mut stack, &mut seen);
    }
    detector.violations
}

struct CycleDetector<'a> {
    root: &'a Path,
    graph: &'a BTreeMap<String, BTreeSet<String>>,
    manifests: BTreeMap<String, PathBuf>,
    violations: Vec<Violation>,
}

impl<'a> CycleDetector<'a> {
    fn find(
        &mut self,
        start: &'a str,
        current: &'a str,
        stack: &mut Vec<&'a str>,
        seen: &mut BTreeSet<&'a str>,
    ) {
        if !seen.insert(current) {
            return;
        }
        stack.push(current);
        for dep in self.graph.get(current).into_iter().flatten() {
            self.visit_dependency(start, dep, stack, seen);
        }
        stack.pop();
    }

    fn visit_dependency(
        &mut self,
        start: &'a str,
        dep: &'a str,
        stack: &mut Vec<&'a str>,
        seen: &mut BTreeSet<&'a str>,
    ) {
        if dep == start {
            self.record_cycle(start, stack);
            return;
        }
        self.find(start, dep, stack, seen);
    }

    fn record_cycle(&mut self, start: &str, stack: &[&str]) {
        if let Some(manifest) = self.manifests.get(start) {
            let mut cycle = stack.join(" -> ");
            cycle.push_str(" -> ");
            cycle.push_str(start);
            self.violations.push(Violation::new(
                relative_path(self.root, manifest),
                0,
                format!("workspace dependency cycle: {cycle}"),
            ));
        }
    }
}
