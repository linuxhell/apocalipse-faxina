use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserKind {
    Chrome,
    Edge,
    Firefox,
}

impl BrowserKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome Portable",
            Self::Edge => "Edge Portable",
            Self::Firefox => "Firefox Portable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupKind {
    Cache,
    Telemetry,
}

impl CleanupKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cache => "Cache",
            Self::Telemetry => "Telemetria",
        }
    }
}

#[derive(Clone, Debug)]
pub struct BrowserItem {
    pub browser: BrowserKind,
    pub kind: CleanupKind,
    pub path: PathBuf,
    pub size: u64,
    pub checked: bool,
    pub error: Option<String>,
}

pub struct BrowserState {
    pub chrome: String,
    pub edge: String,
    pub firefox: String,
    pub items: Vec<BrowserItem>,
    pub status: String,
}

impl BrowserState {
    pub fn load(config_dir: &Path) -> Self {
        let mut state = Self {
            chrome: String::new(),
            edge: String::new(),
            firefox: String::new(),
            items: Vec::new(),
            status: "Configure um executável portátil e clique em Analisar.".into(),
        };

        if let Ok(content) = fs::read_to_string(config_dir.join("portable-browsers.txt")) {
            for line in content.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    match key.trim().to_ascii_lowercase().as_str() {
                        "chrome" => state.chrome = value.trim().to_string(),
                        "edge" => state.edge = value.trim().to_string(),
                        "firefox" => state.firefox = value.trim().to_string(),
                        _ => {}
                    }
                }
            }
        }

        state
    }

    pub fn save(&self, config_dir: &Path) {
        let content = format!(
            "chrome={}\nedge={}\nfirefox={}\n",
            self.chrome, self.edge, self.firefox
        );
        let _ = fs::write(config_dir.join("portable-browsers.txt"), content);
    }

    pub fn analyze(&mut self, exclusions: &[String]) {
        let started = Instant::now();
        crate::diagnostics::operation_start(
            "navegadores_portateis",
            "analyze",
            serde_json::json!({
                "configured": {
                    "chrome": !self.chrome.trim().is_empty(),
                    "edge": !self.edge.trim().is_empty(),
                    "firefox": !self.firefox.trim().is_empty()
                }
            }),
        );
        self.items.clear();
        let configs = [
            (BrowserKind::Chrome, self.chrome.clone()),
            (BrowserKind::Edge, self.edge.clone()),
            (BrowserKind::Firefox, self.firefox.clone()),
        ];

        let mut seen = HashSet::new();
        let mut invalid = Vec::new();

        for (browser, configured) in configs {
            if configured.trim().is_empty() {
                continue;
            }
            let exe = PathBuf::from(configured.trim());
            if !exe.is_file() {
                invalid.push(browser.label());
                continue;
            }
            let Some(root) = exe.parent() else {
                invalid.push(browser.label());
                continue;
            };

            let mut candidates = Vec::new();
            collect_candidates(root, browser, exclusions, 0, &mut candidates);
            for (path, kind) in candidates {
                let key = path.to_string_lossy().to_ascii_lowercase();
                if !seen.insert(key) {
                    continue;
                }
                let size = dir_size(&path, exclusions);
                if size > 0 {
                    self.items.push(BrowserItem {
                        browser,
                        kind,
                        path,
                        size,
                        checked: true,
                        error: None,
                    });
                }
            }
        }

        self.items.sort_by(|a, b| {
            a.browser
                .label()
                .cmp(b.browser.label())
                .then_with(|| a.kind.label().cmp(b.kind.label()))
                .then_with(|| a.path.cmp(&b.path))
        });

        let total = self.total_size();
        self.status = if invalid.is_empty() {
            format!(
                "{} áreas de cache/telemetria encontradas • {}",
                self.items.len(),
                fmt_bytes(total)
            )
        } else {
            format!(
                "{} áreas encontradas • {} • caminho inválido em: {}",
                self.items.len(),
                fmt_bytes(total),
                invalid.join(", ")
            )
        };

        for item in &self.items {
            crate::diagnostics::event(
                "browser_analysis_item",
                "Área de navegador portátil detectada",
                serde_json::json!({
                    "browser": item.browser.label(),
                    "kind": item.kind.label(),
                    "path": item.path.to_string_lossy(),
                    "bytes": item.size
                }),
            );
        }
        crate::diagnostics::operation_end(
            "navegadores_portateis",
            "analyze",
            true,
            started.elapsed().as_millis(),
            serde_json::json!({
                "items": self.items.len(),
                "bytes": total,
                "invalid_paths": invalid
            }),
        );
    }

    pub fn clean_selected(&mut self, exclusions: &[String]) -> u64 {
        let started = Instant::now();
        let selected = self.items.iter().filter(|item| item.checked).count();
        crate::diagnostics::operation_start(
            "navegadores_portateis",
            "clean_selected",
            serde_json::json!({"selected": selected}),
        );

        let mut removed = 0u64;
        for item in self.items.iter_mut().filter(|item| item.checked) {
            item.error = None;
            let before = dir_size(&item.path, exclusions);
            let failures = clean_directory_contents_report(&item.path, exclusions);
            let after = dir_size(&item.path, exclusions);
            let freed = before.saturating_sub(after);
            removed = removed.saturating_add(freed);
            item.size = after;

            crate::diagnostics::event(
                "browser_cleanup_item",
                "Área de navegador portátil processada",
                serde_json::json!({
                    "browser": item.browser.label(),
                    "kind": item.kind.label(),
                    "path": item.path.to_string_lossy(),
                    "before_bytes": before,
                    "after_bytes": after,
                    "freed_bytes": freed,
                    "failures": failures.len()
                }),
            );

            if after > 0 {
                item.error = Some(if failures.is_empty() {
                    "Arquivos permaneceram ou foram recriados pelo navegador.".into()
                } else if failures.len() == 1 {
                    failures[0].clone()
                } else {
                    format!("{} falhas; primeira: {}", failures.len(), failures[0])
                });
            }
        }

        self.items
            .retain(|item| !item.checked || item.size > 0 || item.error.is_some());
        let failed = self
            .items
            .iter()
            .filter(|item| item.checked && item.error.is_some())
            .count();
        self.status = format!(
            "Limpeza dos navegadores concluída • liberado {} • {} área(s) permaneceram",
            fmt_bytes(removed),
            failed
        );
        crate::diagnostics::operation_end(
            "navegadores_portateis",
            "clean_selected",
            true,
            started.elapsed().as_millis(),
            serde_json::json!({
                "freed_bytes": removed,
                "remaining_areas": failed
            }),
        );
        removed
    }

    pub fn total_size(&self) -> u64 {
        self.items.iter().map(|item| item.size).sum()
    }
}

fn collect_candidates(
    root: &Path,
    browser: BrowserKind,
    exclusions: &[String],
    depth: usize,
    out: &mut Vec<(PathBuf, CleanupKind)>,
) {
    if depth > 11 || is_excluded(root, exclusions) {
        return;
    }

    if depth > 0 {
        if let Some(kind) = classify_directory(root, browser) {
            out.push((root.to_path_buf(), kind));
            return;
        }
    }

    let Ok(entries) = fs::read_dir(root) else {
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
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        collect_candidates(&path, browser, exclusions, depth + 1, out);
    }
}

fn classify_directory(path: &Path, browser: BrowserKind) -> Option<CleanupKind> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let full = path.to_string_lossy().replace('/', "\\").to_ascii_lowercase();

    let profileish = full.contains("\\user data\\")
        || full.contains("\\profiles\\")
        || full.contains("\\data\\profile")
        || matches!(browser, BrowserKind::Firefox);

    let cache_names = [
        "cache",
        "cache2",
        "code cache",
        "gpucache",
        "shadercache",
        "grshadercache",
        "dawncache",
        "startupcache",
        "cachestorage",
        "media cache",
    ];
    if profileish && cache_names.iter().any(|x| *x == name) {
        return Some(CleanupKind::Cache);
    }

    let telemetry_names = [
        "crashpad",
        "crash reports",
        "browsermetrics",
        "stability",
        "local traces",
        "webrtc logs",
        "webrtcvideostats",
        "videodecodestats",
        "datareporting",
        "saved-telemetry-pings",
        "crashes",
        "minidumps",
        "sessionstore-logs",
    ];
    if telemetry_names.iter().any(|x| *x == name)
        || full.ends_with("\\weave\\logs")
    {
        return Some(CleanupKind::Telemetry);
    }

    None
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
                    Ok(mut left) => {
                        if left.next().is_none() {
                            if let Err(error) = fs::remove_dir(&child) {
                                failures.push(format!("{}: {}", child.display(), error));
                            }
                        }
                    }
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
