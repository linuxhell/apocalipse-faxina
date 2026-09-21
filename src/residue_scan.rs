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
        for item in &self.items {
            if !item.checked || is_excluded(&item.path, exclusions) {
                continue;
            }
            if fs::remove_file(&item.path).is_ok() {
                removed = removed.saturating_add(item.size);
            }
        }
        self.items.retain(|item| item.path.exists());
        self.status = format!(
            "Limpeza de resíduos concluída • liberado {} • {} itens restantes na lista",
            fmt_bytes(removed),
            self.items.len()
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
            });
        }
    }
}

fn classify_file(path: &Path) -> Option<(ResidueRisk, String)> {
    let ext = path
        .extension()
        .map(|x| x.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let safe = ["tmp", "temp", "chk", "dmp", "mdmp", "wer", "crdownload", "part", "download"];
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
