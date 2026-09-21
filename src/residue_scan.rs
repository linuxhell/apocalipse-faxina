use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResidueRisk {
    Safe,
    Review,
}

impl ResidueRisk {
    pub fn label(self) -> &'static str {
        match self {
            Self::Safe => "Seguro",
            Self::Review => "Revisar",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResidueItem {
    pub path: PathBuf,
    pub size: u64,
    pub age_days: u64,
    pub risk: ResidueRisk,
    pub reason: String,
    pub checked: bool,
    pub is_dir: bool,
    pub error: Option<String>,
}

pub struct ResidueState {
    pub root: String,
    pub min_age_days: u64,
    pub items: Vec<ResidueItem>,
    pub status: String,
}

impl Default for ResidueState {
    fn default() -> Self {
        let root = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".into());
        Self {
            root,
            min_age_days: 14,
            items: Vec::new(),
            status: "Escolha uma pasta/unidade e analise resíduos por extensão, idade e localização.".into(),
        }
    }
}

impl ResidueState {
    pub fn analyze(&mut self, exclusions: &[String]) {
        self.items.clear();
        let root = PathBuf::from(self.root.trim());
        if !root.exists() {
            self.status = "O caminho informado não existe.".into();
            return;
        }
        let now = SystemTime::now();
        scan_dir(
            &root,
            exclusions,
            self.min_age_days,
            now,
            0,
            &mut self.items,
        );
        self.items.sort_by(|a, b| {
            a.risk
                .label()
                .cmp(b.risk.label())
                .then_with(|| b.size.cmp(&a.size))
                .then_with(|| a.path.cmp(&b.path))
        });
        self.mark_safe();
        self.status = format!(
            "{} resíduos encontrados • seguros marcados: {} • total listado: {}",
            self.items.len(),
            self.items.iter().filter(|x| x.checked).count(),
            fmt_bytes(self.items.iter().map(|x| x.size).sum())
        );
    }

    pub fn mark_safe(&mut self) {
        for item in &mut self.items {
            item.checked = item.risk == ResidueRisk::Safe;
        }
    }

    pub fn mark_all(&mut self) {
        for item in &mut self.items {
            item.checked = true;
        }
    }

    pub fn clear(&mut self) {
        for item in &mut self.items {
            item.checked = false;
        }
    }

    pub fn selected_size(&self) -> u64 {
        self.items
            .iter()
            .filter(|x| x.checked)
            .map(|x| x.size)
            .sum()
    }

    pub fn delete_selected(&mut self, exclusions: &[String]) -> u64 {
        let mut removed = 0u64;
        for item in self.items.iter_mut().filter(|item| item.checked) {
            item.error = None;
            if is_excluded(&item.path, exclusions) {
                item.error = Some("Protegido pelas Exclusões globais.".into());
                continue;
            }

            if item.is_dir {
                let before = dir_size(&item.path, exclusions);
                let failures = clean_directory_contents_report(&item.path, exclusions);
                let after = dir_size(&item.path, exclusions);
                removed = removed.saturating_add(before.saturating_sub(after));
                item.size = after;
                if after == 0 {
                    if let Err(error) = fs::remove_dir(&item.path) {
                        if item.path.exists() {
                            item.error = Some(format!("Não foi possível remover a pasta: {error}"));
                        }
                    }
                } else {
                    item.error = Some(if failures.is_empty() {
                        "Parte do conteúdo permaneceu em uso ou foi recriada.".into()
                    } else if failures.len() == 1 {
                        failures[0].clone()
                    } else {
                        format!("{} falhas; primeira: {}", failures.len(), failures[0])
                    });
                }
            } else {
                match fs::remove_file(&item.path) {
                    Ok(_) => removed = removed.saturating_add(item.size),
                    Err(error) => item.error = Some(error.to_string()),
                }
            }
        }

        self.items
            .retain(|item| !item.checked || item.path.exists() || item.error.is_some());
        let failed = self
            .items
            .iter()
            .filter(|item| item.checked && item.error.is_some())
            .count();
        self.status = format!(
            "Limpeza de resíduos concluída • liberado {} • {} item(ns) não puderam ser removidos",
            fmt_bytes(removed),
            failed
        );
        removed
    }
}

fn scan_dir(
    dir: &Path,
    exclusions: &[String],
    min_age_days: u64,
    now: SystemTime,
    depth: usize,
    out: &mut Vec<ResidueItem>,
) {
    if depth > 64 || is_excluded(dir, exclusions) || should_skip_directory(dir) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if is_excluded(&path, exclusions) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let age_days = meta
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .unwrap_or(Duration::ZERO)
                .as_secs()
                / 86_400;
            if age_days >= min_age_days && is_python_cache_dir(&path) {
                let size = dir_size(&path, exclusions);
                if size > 0 {
                    out.push(ResidueItem {
                        path,
                        size,
                        age_days,
                        risk: ResidueRisk::Safe,
                        reason: "Cache regenerável de Python/ferramenta de desenvolvimento".into(),
                        checked: false,
                        is_dir: true,
                        error: None,
                    });
                }
                continue;
            }
            scan_dir(
                &path,
                exclusions,
                min_age_days,
                now,
                depth + 1,
                out,
            );
            continue;
        }
        if !meta.is_file() {
            continue;
        }

        let age_days = meta
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .unwrap_or(Duration::ZERO)
            .as_secs()
            / 86_400;

        if age_days < min_age_days {
            continue;
        }

        if let Some((risk, reason)) = classify_file(&path) {
            out.push(ResidueItem {
                path,
                size: meta.len(),
                age_days,
                risk,
                reason,
                checked: false,
                is_dir: false,
                error: None,
            });
        }
    }
}

fn classify_file(path: &Path) -> Option<(ResidueRisk, String)> {
    let ext = path
        .extension()
        .map(|x| x.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let safe = ["tmp", "temp", "chk", "dmp", "mdmp", "wer", "crdownload", "part", "download", "pyc", "pyo"];
    if safe.contains(&ext.as_str()) {
        return Some((
            ResidueRisk::Safe,
            format!("Extensão residual conhecida: .{ext}"),
        ));
    }

    let review = ["old", "bak", "backup", "bk", "etl", "log"];
    if review.contains(&ext.as_str()) {
        return Some((
            ResidueRisk::Review,
            format!("Possível resíduo/backup: .{ext} — exige confirmação"),
        ));
    }

    None
}

fn is_python_cache_dir(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|x| x.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        name.as_str(),
        "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache"
    )
}

fn dir_size(path: &Path, exclusions: &[String]) -> u64 {
    if is_excluded(path, exclusions) {
        return 0;
    }
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            total = total.saturating_add(dir_size(&entry.path(), exclusions));
        }
    }
    total
}

fn clean_directory_contents_report(path: &Path, exclusions: &[String]) -> Vec<String> {
    fn visit(path: &Path, exclusions: &[String], failures: &mut Vec<String>) {
        if is_excluded(path, exclusions) {
            return;
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                failures.push(format!("{}: {}", path.display(), error));
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    failures.push(format!("Falha ao ler item: {error}"));
                    continue;
                }
            };
            let child = entry.path();
            if is_excluded(&child, exclusions) {
                continue;
            }
            let meta = match fs::symlink_metadata(&child) {
                Ok(meta) => meta,
                Err(error) => {
                    failures.push(format!("{}: {}", child.display(), error));
                    continue;
                }
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                visit(&child, exclusions, failures);
                match fs::read_dir(&child) {
                    Ok(mut left) if left.next().is_none() => {
                        if let Err(error) = fs::remove_dir(&child) {
                            failures.push(format!("{}: {}", child.display(), error));
                        }
                    }
                    Ok(_) => {}
                    Err(error) => failures.push(format!("{}: {}", child.display(), error)),
                }
            } else if let Err(error) = fs::remove_file(&child) {
                failures.push(format!("{}: {}", child.display(), error));
            }
        }
    }

    let mut failures = Vec::new();
    visit(path, exclusions, &mut failures);
    failures
}

fn should_skip_directory(path: &Path) -> bool {
    let p = normalize_path(&path.to_string_lossy());
    let protected = [
        "\\windows\\winsxs",
        "\\windows\\system32",
        "\\windows\\syswow64",
        "\\windows\\servicing",
        "\\windows\\systemapps",
        "\\program files",
        "\\program files (x86)",
        "\\programdata\\package cache",
        "\\system volume information",
        "\\$recycle.bin",
        "\\recovery",
    ];
    protected.iter().any(|part| p.ends_with(part) || p.contains(&format!("{part}\\")))
}

fn normalize_path(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn is_excluded(path: &Path, exclusions: &[String]) -> bool {
    let path = normalize_path(&path.to_string_lossy());
    exclusions.iter().any(|entry| {
        let entry = normalize_path(entry);
        !entry.is_empty()
            && (path == entry
                || path
                    .strip_prefix(&entry)
                    .is_some_and(|rest| rest.starts_with('\\')))
    })
}

fn fmt_bytes(value: u64) -> String {
    const K: f64 = 1024.0;
    let value = value as f64;
    if value >= K * K * K {
        format!("{:.2} GB", value / (K * K * K))
    } else if value >= K * K {
        format!("{:.2} MB", value / (K * K))
    } else if value >= K {
        format!("{:.2} KB", value / K)
    } else {
        format!("{} B", value as u64)
    }
}
