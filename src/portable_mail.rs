use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailCleanupKind {
    Cache,
    Telemetry,
}

impl MailCleanupKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cache => "Cache",
            Self::Telemetry => "Telemetria",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MailItem {
    pub kind: MailCleanupKind,
    pub path: PathBuf,
    pub size: u64,
    pub checked: bool,
    pub error: Option<String>,
}

enum WorkerOutput {
    Analysis {
        items: Vec<MailItem>,
        elapsed_ms: u128,
    },
    Clean {
        items: Vec<MailItem>,
        removed: u64,
        failed: usize,
        elapsed_ms: u128,
    },
}

pub struct MailState {
    pub thunderbird: String,
    pub items: Vec<MailItem>,
    pub status: String,
    pub analyzing: bool,
    pub cleaning: bool,
    rx: Option<Receiver<Result<WorkerOutput, String>>>,
}

impl MailState {
    pub fn load(config_dir: &Path) -> Self {
        let mut state = Self {
            thunderbird: String::new(),
            items: Vec::new(),
            status: "Configure o Thunderbird Portable e clique em Analisar.".into(),
            analyzing: false,
            cleaning: false,
            rx: None,
        };

        if let Ok(content) = fs::read_to_string(config_dir.join("portable-mail-client.txt")) {
            for line in content.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    if key.trim().eq_ignore_ascii_case("thunderbird") {
                        state.thunderbird = value.trim().to_string();
                    }
                }
            }
        }
        state
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(config_dir)
            .map_err(|error| format!("Falha ao criar pasta de configuração: {error}"))?;
        fs::write(
            config_dir.join("portable-mail-client.txt"),
            format!("thunderbird={}\n", self.thunderbird),
        )
        .map_err(|error| format!("Falha ao salvar Thunderbird Portable: {error}"))
    }

    pub fn busy(&self) -> bool {
        self.analyzing || self.cleaning
    }

    pub fn start_analyze(&mut self, exclusions: &[String]) {
        if self.busy() {
            return;
        }

        let exe = self.thunderbird.trim().to_string();
        if exe.is_empty() {
            self.status = "Escolha ThunderbirdPortable.exe primeiro.".into();
            return;
        }

        self.items.clear();
        self.analyzing = true;
        self.status = "Analisando cache e telemetria do Thunderbird Portable…".into();
        let exclusions = exclusions.to_vec();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        crate::diagnostics::operation_start(
            "email_portatil",
            "analyze_thunderbird",
            serde_json::json!({"exe": exe}),
        );

        thread::spawn(move || {
            let started = Instant::now();
            let result = analyze_thunderbird(Path::new(&exe), &exclusions).map(|items| {
                WorkerOutput::Analysis {
                    items,
                    elapsed_ms: started.elapsed().as_millis(),
                }
            });
            let _ = tx.send(result);
        });
    }

    pub fn start_clean(&mut self, exclusions: &[String]) {
        if self.busy() {
            return;
        }
        let selected = self.items.iter().filter(|item| item.checked).count();
        if selected == 0 {
            self.status = "Nenhuma área selecionada para limpeza.".into();
            return;
        }

        self.cleaning = true;
        self.status = "Limpando cache/telemetria do Thunderbird Portable…".into();
        let exclusions = exclusions.to_vec();
        let mut items = self.items.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        crate::diagnostics::operation_start(
            "email_portatil",
            "clean_thunderbird",
            serde_json::json!({"selected": selected}),
        );

        thread::spawn(move || {
            let started = Instant::now();
            let mut removed = 0u64;

            for item in items.iter_mut().filter(|item| item.checked) {
                item.error = None;
                let before = dir_size(&item.path, &exclusions);
                let failures = clean_directory_contents_report(&item.path, &exclusions);
                let after = dir_size(&item.path, &exclusions);
                let freed = before.saturating_sub(after);
                removed = removed.saturating_add(freed);
                item.size = after;

                crate::diagnostics::event(
                    "email_cleanup_item",
                    "Thunderbird Portable: item processado",
                    serde_json::json!({
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
                        "Arquivos permaneceram ou foram recriados pelo Thunderbird.".into()
                    } else if failures.len() == 1 {
                        failures[0].clone()
                    } else {
                        format!("{} falhas; primeira: {}", failures.len(), failures[0])
                    });
                }
            }

            items.retain(|item| !item.checked || item.size > 0 || item.error.is_some());
            let failed = items
                .iter()
                .filter(|item| item.checked && item.error.is_some())
                .count();

            let _ = tx.send(Ok(WorkerOutput::Clean {
                items,
                removed,
                failed,
                elapsed_ms: started.elapsed().as_millis(),
            }));
        });
    }

    pub fn poll(&mut self) {
        let result = match self.rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Empty)) => return,
            Some(Err(TryRecvError::Disconnected)) => {
                let operation = if self.analyzing {
                    "analyze_thunderbird"
                } else {
                    "clean_thunderbird"
                };
                self.rx = None;
                self.analyzing = false;
                self.cleaning = false;
                self.status =
                    "O worker do Thunderbird terminou inesperadamente; a operação foi encerrada para não deixar a interface presa."
                        .into();
                crate::diagnostics::operation_end(
                    "email_portatil",
                    operation,
                    false,
                    0,
                    serde_json::json!({"error":"worker desconectado"}),
                );
                return;
            }
            None => return,
        };
        self.rx = None;

        match result {
            Ok(WorkerOutput::Analysis { mut items, elapsed_ms }) => {
                self.analyzing = false;
                self.cleaning = false;
                items.sort_by(|a, b| {
                    a.kind
                        .label()
                        .cmp(b.kind.label())
                        .then_with(|| a.path.cmp(&b.path))
                });
                let total = items.iter().map(|item| item.size).sum::<u64>();
                self.status = format!(
                    "{} área(s) encontrada(s) • {}",
                    items.len(),
                    fmt_bytes(total)
                );
                crate::diagnostics::operation_end(
                    "email_portatil",
                    "analyze_thunderbird",
                    true,
                    elapsed_ms,
                    serde_json::json!({"items": items.len(), "bytes": total}),
                );
                self.items = items;
            }
            Ok(WorkerOutput::Clean {
                items,
                removed,
                failed,
                elapsed_ms,
            }) => {
                self.analyzing = false;
                self.cleaning = false;
                self.status = format!(
                    "Limpeza concluída • liberado {} • {} área(s) permaneceram",
                    fmt_bytes(removed),
                    failed
                );
                crate::diagnostics::operation_end(
                    "email_portatil",
                    "clean_thunderbird",
                    true,
                    elapsed_ms,
                    serde_json::json!({"freed_bytes": removed, "remaining_areas": failed}),
                );
                self.items = items;
            }
            Err(error) => {
                let operation = if self.analyzing {
                    "analyze_thunderbird"
                } else {
                    "clean_thunderbird"
                };
                self.analyzing = false;
                self.cleaning = false;
                self.status = format!("Falha: {error}");
                crate::diagnostics::operation_end(
                    "email_portatil",
                    operation,
                    false,
                    0,
                    serde_json::json!({"error": error}),
                );
            }
        }
    }

    pub fn total_size(&self) -> u64 {
        self.items.iter().map(|item| item.size).sum()
    }
}

fn analyze_thunderbird(exe: &Path, exclusions: &[String]) -> Result<Vec<MailItem>, String> {
    if !exe.is_file() {
        return Err(format!("Executável não encontrado: {}", exe.display()));
    }

    let root = exe
        .parent()
        .ok_or_else(|| "Não foi possível localizar a pasta do Thunderbird Portable.".to_string())?;
    let data = root.join("Data");
    let preferred = data.join("profile");

    let mut profiles = Vec::new();
    if preferred.is_dir() {
        profiles.push(preferred);
    } else if data.is_dir() {
        find_profiles(&data, 0, &mut profiles);
    }

    if profiles.is_empty() {
        return Err(format!(
            "Nenhum perfil portátil foi localizado abaixo de {}. O Faxina procura o perfil ativo dentro da pasta Data do Thunderbird Portable.",
            root.display()
        ));
    }

    let mut seen = HashSet::new();
    let mut items = Vec::new();

    for profile in profiles {
        let mut candidates = Vec::new();
        collect_candidates(&profile, exclusions, 0, &mut candidates);
        for (path, kind) in candidates {
            let key = normalize_path(&path.to_string_lossy());
            if !seen.insert(key) {
                continue;
            }
            let size = dir_size(&path, exclusions);
            if size > 0 {
                items.push(MailItem {
                    kind,
                    path,
                    size,
                    checked: true,
                    error: None,
                });
            }
        }
    }

    Ok(items)
}

fn find_profiles(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    if root.join("prefs.js").is_file() {
        out.push(root.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(name.as_str(), "app" | "other" | "mail" | "imapmail" | "news") {
            continue;
        }
        find_profiles(&path, depth + 1, out);
    }
}

fn collect_candidates(
    root: &Path,
    exclusions: &[String],
    depth: usize,
    out: &mut Vec<(PathBuf, MailCleanupKind)>,
) {
    if depth > 6 || is_excluded(root, exclusions) {
        return;
    }

    if depth > 0 {
        if let Some(kind) = classify_directory(root) {
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

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(
            name.as_str(),
            "mail"
                | "imapmail"
                | "news"
                | "storage"
                | "extensions"
                | "calendar-data"
                | "bookmarkbackups"
        ) {
            continue;
        }

        collect_candidates(&path, exclusions, depth + 1, out);
    }
}

fn classify_directory(path: &Path) -> Option<MailCleanupKind> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if matches!(
        name.as_str(),
        "cache2" | "startupcache" | "shader-cache" | "gpucache"
    ) {
        return Some(MailCleanupKind::Cache);
    }

    if matches!(
        name.as_str(),
        "datareporting"
            | "saved-telemetry-pings"
            | "crashes"
            | "crash reports"
            | "minidumps"
            | "pending pings"
    ) {
        return Some(MailCleanupKind::Telemetry);
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
