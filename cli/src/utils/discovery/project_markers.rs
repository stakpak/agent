use ignore::WalkBuilder;
use std::fmt::Write;
use std::path::{Path, PathBuf};

/// Markers that indicate a project root, with their associated language/framework.
static PROJECT_MARKERS: &[(&str, &str)] = &[
    ("package.json", "Node.js"),
    ("go.mod", "Go"),
    ("Cargo.toml", "Rust"),
    ("pyproject.toml", "Python"),
    ("setup.py", "Python"),
    ("requirements.txt", "Python"),
    ("pom.xml", "Java/Maven"),
    ("build.gradle", "Java/Gradle"),
    ("build.gradle.kts", "Kotlin/Gradle"),
    ("Gemfile", "Ruby"),
    ("composer.json", "PHP"),
    ("mix.exs", "Elixir"),
    ("pubspec.yaml", "Dart/Flutter"),
];

/// IaC and CI/CD markers.
static IAC_MARKERS: &[(&str, &str)] = &[
    ("*.tf", "Terraform"),
    ("terragrunt.hcl", "Terragrunt"),
    (".terraform.lock.hcl", "Terraform"),
    ("Pulumi.yaml", "Pulumi"),
    ("template.yaml", "CloudFormation/SAM"),
    ("cdk.json", "AWS CDK"),
    ("ansible.cfg", "Ansible"),
    ("Chart.yaml", "Helm"),
    ("helmfile.yaml", "Helmfile"),
    ("kustomization.yaml", "Kustomize"),
    ("skaffold.yaml", "Skaffold"),
    ("docker-compose.yml", "Docker Compose"),
    ("docker-compose.yaml", "Docker Compose"),
    ("compose.yml", "Docker Compose"),
    ("compose.yaml", "Docker Compose"),
    ("Dockerfile", "Docker"),
];

static CI_MARKERS: &[(&str, &str)] = &[
    (".github/workflows", "GitHub Actions"),
    (".gitlab-ci.yml", "GitLab CI"),
    ("Jenkinsfile", "Jenkins"),
    (".circleci/config.yml", "CircleCI"),
    ("bitbucket-pipelines.yml", "Bitbucket Pipelines"),
    (".buildkite/pipeline.yml", "Buildkite"),
    ("azure-pipelines.yml", "Azure Pipelines"),
    ("cloudbuild.yaml", "Cloud Build"),
];

/// Discover project markers, IaC, and CI/CD configs.
/// Scans $HOME broadly for Dockerfiles/compose, and cwd deeply for everything.
pub fn discover(_home: Option<&Path>, cwd: Option<&Path>) -> String {
    let mut out = String::with_capacity(2048);

    // Deep scan of cwd (if it looks like a project)
    if let Some(dir) = cwd {
        let _ = writeln!(out, "### Working Directory: {}\n", dir.display());

        // Project language markers
        let mut found_lang = false;
        for (marker, lang) in PROJECT_MARKERS {
            if dir.join(marker).exists() {
                if !found_lang {
                    let _ = writeln!(out, "Languages:");
                    found_lang = true;
                }
                let _ = writeln!(out, "  - {} ({})", lang, marker);
            }
        }

        // IaC markers — scan recursively
        let iac_hits = scan_for_markers(dir, IAC_MARKERS, 5);
        if !iac_hits.is_empty() {
            let _ = writeln!(out, "IaC:");
            for (tool, path) in &iac_hits {
                let _ = writeln!(out, "  - {} ({})", tool, path.display());
            }
        }

        // CI/CD markers
        let mut found_ci = false;
        for (marker, tool) in CI_MARKERS {
            let target = dir.join(marker);
            if target.exists() {
                if !found_ci {
                    let _ = writeln!(out, "CI/CD:");
                    found_ci = true;
                }
                if target.is_dir() {
                    let count = std::fs::read_dir(&target)
                        .map(|entries| {
                            entries
                                .flatten()
                                .filter(|e| {
                                    let name = e.file_name().to_string_lossy().to_string();
                                    name.ends_with(".yml") || name.ends_with(".yaml")
                                })
                                .count()
                        })
                        .unwrap_or(0);
                    let _ = writeln!(out, "  - {} ({} — {} files)", tool, marker, count);
                } else {
                    let _ = writeln!(out, "  - {} ({})", tool, marker);
                }
            }
        }

        // Dockerfiles in cwd
        let dockerfiles = find_files_by_name(dir, 5, |name| {
            name == "Dockerfile" || name.starts_with("Dockerfile.") || name.ends_with(".dockerfile")
        });
        if !dockerfiles.is_empty() {
            let _ = writeln!(out, "Dockerfiles:");
            for p in &dockerfiles {
                let _ = writeln!(out, "  - {}", p.display());
            }
        }

        // Monorepo indicators
        let mut monorepo = Vec::new();
        for ws in &["lerna.json", "pnpm-workspace.yaml", "turbo.json", "nx.json"] {
            if dir.join(ws).exists() {
                monorepo.push(*ws);
            }
        }
        if let Ok(content) = std::fs::read_to_string(dir.join("package.json"))
            && content.contains("\"workspaces\"")
        {
            monorepo.push("package.json workspaces");
        }
        if let Ok(content) = std::fs::read_to_string(dir.join("Cargo.toml"))
            && content.contains("[workspace]")
        {
            monorepo.push("Cargo workspace");
        }
        if !monorepo.is_empty() {
            let _ = writeln!(out, "Monorepo: {}", monorepo.join(", "));
        }

        // Env files (existence only)
        let env_files = find_files_by_name(dir, 4, |name| {
            name == ".env"
                || name.starts_with(".env.")
                || name == ".env.example"
                || name == ".env.sample"
                || name == ".env.template"
        });
        if !env_files.is_empty() {
            let _ = writeln!(out, "Env files:");
            for p in &env_files {
                let _ = writeln!(out, "  - {}", p.display());
            }
        }
    }

    out
}

/// Scan a directory for IaC markers, returning (tool_name, path) pairs.
fn scan_for_markers(
    dir: &Path,
    markers: &[(&str, &str)],
    max_depth: usize,
) -> Vec<(String, PathBuf)> {
    let mut hits = Vec::new();

    // Check simple file markers first
    for (marker, tool) in markers {
        if marker.starts_with('*') {
            // Glob-style — handled below via walker
            continue;
        }
        let target = dir.join(marker);
        if target.exists() {
            hits.push((tool.to_string(), target));
        }
    }

    // For glob markers (e.g. "*.tf"), walk the tree
    let glob_markers: Vec<(&str, &str)> = markers
        .iter()
        .filter(|(m, _)| m.starts_with('*'))
        .copied()
        .collect();

    if !glob_markers.is_empty() {
        let walker = WalkBuilder::new(dir)
            .hidden(true)
            .git_ignore(true)
            .max_depth(Some(max_depth))
            .build();

        let mut seen_tools: std::collections::HashSet<String> = std::collections::HashSet::new();
        for entry in walker.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            for (marker, tool) in &glob_markers {
                let ext = &marker[1..]; // e.g. ".tf"
                if name.ends_with(ext) && seen_tools.insert(tool.to_string()) {
                    hits.push((tool.to_string(), path.to_path_buf()));
                    break;
                }
            }
        }
    }

    hits
}

/// Find files matching a name predicate under a directory.
fn find_files_by_name<F>(dir: &Path, max_depth: usize, predicate: F) -> Vec<PathBuf>
where
    F: Fn(&str) -> bool,
{
    let walker = WalkBuilder::new(dir)
        .hidden(false)
        .git_ignore(true)
        .max_depth(Some(max_depth))
        .build();

    let mut results = Vec::new();
    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && predicate(name)
        {
            results.push(path.to_path_buf());
        }
    }
    results
}
