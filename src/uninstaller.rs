use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UninstallerTab {
    Programs,
    WindowsApps,
    Forced,
    Monitor,
    History,
}

impl UninstallerTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Programs => "Programas instalados",
            Self::WindowsApps => "Windows Apps",
            Self::Forced => "Desinstalação forçada",
            Self::Monitor => "Instalação monitorada",
            Self::History => "Histórico",
        }
    }

    pub const ALL: [Self; 5] = [
        Self::Programs,
        Self::WindowsApps,
        Self::Forced,
        Self::Monitor,
        Self::History,
    ];
}

#[derive(Clone, Debug, Deserialize)]
pub struct InstalledProgram {
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "Version", default)]
    pub version: String,
    #[serde(rename = "Publisher", default)]
    pub publisher: String,
    #[serde(rename = "InstallLocation", default)]
    pub install_location: String,
    #[serde(rename = "UninstallString", default)]
    pub uninstall_string: String,
    #[serde(rename = "QuietUninstallString", default)]
    pub quiet_uninstall_string: String,
    #[serde(rename = "DisplayIcon", default)]
    pub display_icon: String,
    #[serde(rename = "RegistryPath", default)]
    pub registry_path: String,
    #[serde(rename = "EstimatedSizeKb", default)]
    pub estimated_size_kb: u64,
    #[serde(rename = "WindowsInstaller", default)]
    pub windows_installer: bool,
    #[serde(skip)]
    pub checked: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WindowsApp {
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "PackageFullName", default)]
    pub package_full_name: String,
    #[serde(rename = "Version", default)]
    pub version: String,
    #[serde(rename = "Publisher", default)]
    pub publisher: String,
    #[serde(rename = "InstallLocation", default)]
    pub install_location: String,
    #[serde(rename = "NonRemovable", default)]
    pub non_removable: bool,
    #[serde(rename = "IsFramework", default)]
    pub is_framework: bool,
    #[serde(skip)]
    pub checked: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProcessItem {
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "Id", default)]
    pub id: u32,
    #[serde(rename = "Path", default)]
    pub path: String,
    #[serde(skip)]
    pub checked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResidueKind {
    RegistryKey,
    RegistryValue,
    Folder,
    File,
    Service,
    Task,
}

impl ResidueKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::RegistryKey => "Chave de Registro",
            Self::RegistryValue => "Valor de Registro",
            Self::Folder => "Pasta",
            Self::File => "Arquivo",
            Self::Service => "Serviço",
            Self::Task => "Tarefa agendada",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResidueItem {
    pub risk: ResidueRisk,
    pub kind: ResidueKind,
    pub path: String,
    pub value_name: String,
    pub reason: String,
    pub evidence: String,
    pub size: u64,
    #[serde(skip)]
    pub checked: bool,
    #[serde(skip)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProgramIdentity {
    name: String,
    publisher: String,
    install_location: String,
    uninstall_string: String,
    display_icon: String,
    registry_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct InstallSnapshot {
    created: u64,
    uninstall_keys: Vec<String>,
    program_dirs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub time: u64,
    pub action: String,
    pub target: String,
    pub detail: String,
    pub success: bool,
}

enum WorkerOutput {
    Programs(Vec<InstalledProgram>),
    Apps(Vec<WindowsApp>),
    Processes(Vec<ProcessItem>),
    Residues {
        target: String,
        items: Vec<ResidueItem>,
    },
    Uninstall {
        programs: Vec<InstalledProgram>,
        results: Vec<(String, bool, String)>,
    },
    RemoveResidues {
        removed: usize,
        failed: Vec<String>,
        quarantined_bytes: u64,
        backup_dir: PathBuf,
    },
    RemoveApps {
        removed: usize,
        failed: Vec<String>,
    },
    SnapshotCreated {
        path: PathBuf,
        snapshot: InstallSnapshot,
    },
    SnapshotCompared {
        new_uninstall_keys: Vec<String>,
        new_program_dirs: Vec<String>,
    },
}

pub struct UninstallerState {
    pub tab: UninstallerTab,
    pub programs: Vec<InstalledProgram>,
    pub apps: Vec<WindowsApp>,
    pub processes: Vec<ProcessItem>,
    pub residues: Vec<ResidueItem>,
    pub history: Vec<HistoryEntry>,
    pub status: String,
    pub filter: String,
    pub forced_path: String,
    pub process_filter: String,
    pub monitor_result: Vec<String>,
    pub busy: bool,
    pub last_target: String,
    rx: Option<Receiver<Result<WorkerOutput, String>>>,
}

impl Default for UninstallerState {
    fn default() -> Self {
        Self {
            tab: UninstallerTab::Programs,
            programs: Vec::new(),
            apps: Vec::new(),
            processes: Vec::new(),
            residues: Vec::new(),
            history: Vec::new(),
            status: "Clique em Atualizar programas para iniciar.".into(),
            filter: String::new(),
            forced_path: String::new(),
            process_filter: String::new(),
            monitor_result: Vec::new(),
            busy: false,
            last_target: String::new(),
            rx: None,
        }
    }
}

impl UninstallerState {
    pub fn load(root: &Path) -> Self {
        let mut state = Self::default();
        state.history = load_history(root);
        state
    }

    pub fn selected_programs(&self) -> usize {
        self.programs.iter().filter(|p| p.checked).count()
    }

    pub fn selected_apps(&self) -> usize {
        self.apps.iter().filter(|p| p.checked).count()
    }

    pub fn selected_residues(&self) -> usize {
        self.residues.iter().filter(|r| r.checked).count()
    }

    pub fn selected_residue_size(&self) -> u64 {
        self.residues
            .iter()
            .filter(|r| r.checked)
            .map(|r| r.size)
            .sum()
    }

    pub fn start_refresh_programs(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = "Lendo programas instalados…".into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        crate::diagnostics::operation_start(
            "faxina_uninstaller",
            "inventory_programs",
            serde_json::json!({}),
        );
        thread::spawn(move || {
            let result = scan_installed_programs().map(WorkerOutput::Programs);
            let _ = tx.send(result);
        });
    }

    pub fn start_refresh_apps(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = "Lendo aplicativos Windows do usuário atual…".into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        crate::diagnostics::operation_start(
            "faxina_uninstaller",
            "inventory_appx",
            serde_json::json!({}),
        );
        thread::spawn(move || {
            let result = scan_windows_apps().map(WorkerOutput::Apps);
            let _ = tx.send(result);
        });
    }

    pub fn start_refresh_processes(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = "Lendo processos com caminho conhecido…".into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let result = scan_processes().map(WorkerOutput::Processes);
            let _ = tx.send(result);
        });
    }

    pub fn use_checked_process_as_target(&mut self) {
        if let Some(process) = self.processes.iter().find(|p| p.checked && !p.path.is_empty()) {
            self.forced_path = process.path.clone();
            self.tab = UninstallerTab::Forced;
            self.status = format!(
                "Modo Alvo: {} (PID {}) selecionado. Clique em Analisar instalação forçada.",
                process.name, process.id
            );
        }
    }

    pub fn start_scan_selected_program_residues(&mut self) {
        if self.busy {
            return;
        }
        let selected: Vec<_> = self.programs.iter().filter(|p| p.checked).cloned().collect();
        if selected.len() != 1 {
            self.status = "Marque exatamente um programa para analisar sobras.".into();
            return;
        }
        let identity = identity_from_program(&selected[0]);
        self.start_residue_scan(identity);
    }

    pub fn start_forced_scan(&mut self) {
        if self.busy {
            return;
        }
        let raw = self.forced_path.trim();
        if raw.is_empty() {
            self.status = "Informe um EXE ou pasta para a desinstalação forçada.".into();
            return;
        }
        let path = PathBuf::from(raw);
        let root = if path.is_file() {
            path.parent().unwrap_or(&path).to_path_buf()
        } else {
            path.clone()
        };
        if !root.exists() {
            self.status = format!("Caminho não encontrado: {}", root.display());
            return;
        }

        let name = path
            .file_stem()
            .or_else(|| root.file_name())
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_else(|| "Programa".into());

        let identity = ProgramIdentity {
            name,
            publisher: String::new(),
            install_location: root.to_string_lossy().to_string(),
            uninstall_string: String::new(),
            display_icon: if path.is_file() {
                path.to_string_lossy().to_string()
            } else {
                String::new()
            },
            registry_path: String::new(),
        };
        self.start_residue_scan(identity);
    }

    fn start_residue_scan(&mut self, identity: ProgramIdentity) {
        self.busy = true;
        self.last_target = identity.name.clone();
        self.residues.clear();
        self.status = format!("Analisando sobras relacionadas a {}…", identity.name);
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        crate::diagnostics::operation_start(
            "faxina_uninstaller",
            "scan_residues",
            serde_json::json!({
                "name": identity.name,
                "publisher": identity.publisher,
                "install_location": identity.install_location
            }),
        );

        thread::spawn(move || {
            let target = identity.name.clone();
            let result = scan_residues(&identity).map(|items| WorkerOutput::Residues {
                target,
                items,
            });
            let _ = tx.send(result);
        });
    }

    pub fn start_uninstall_selected(&mut self) {
        if self.busy {
            return;
        }
        let selected: Vec<_> = self.programs.iter().filter(|p| p.checked).cloned().collect();
        if selected.is_empty() {
            self.status = "Marque pelo menos um programa.".into();
            return;
        }
        self.busy = true;
        self.status = format!(
            "Fila de desinstalação iniciada: {} programa(s). Conclua cada desinstalador quando ele aparecer.",
            selected.len()
        );
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        crate::diagnostics::operation_start(
            "faxina_uninstaller",
            "uninstall_queue",
            serde_json::json!({"count": selected.len()}),
        );
        thread::spawn(move || {
            let mut results = Vec::new();
            for program in &selected {
                let command = official_uninstall_command(program);
                match command {
                    Some(command) => {
                        let result = run_interactive_uninstall(&command);
                        results.push((
                            program.name.clone(),
                            result.is_ok(),
                            result.unwrap_or_else(|e| e),
                        ));
                    }
                    None => results.push((
                        program.name.clone(),
                        false,
                        "Nenhum comando de desinstalação foi encontrado.".into(),
                    )),
                }
            }
            let _ = tx.send(Ok(WorkerOutput::Uninstall {
                programs: selected,
                results,
            }));
        });
    }

    pub fn start_remove_selected_residues(&mut self, root: &Path) {
        if self.busy {
            return;
        }
        let selected: Vec<_> = self.residues.iter().filter(|r| r.checked).cloned().collect();
        if selected.is_empty() {
            self.status = "Nenhuma sobra marcada.".into();
            return;
        }
        let backup_dir = root
            .join("backups")
            .join("uninstaller")
            .join(timestamp_slug());
        self.busy = true;
        self.status = format!(
            "Criando backup/quarentena e removendo {} sobra(s)…",
            selected.len()
        );
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        crate::diagnostics::operation_start(
            "faxina_uninstaller",
            "remove_residues",
            serde_json::json!({
                "count": selected.len(),
                "backup": backup_dir.to_string_lossy()
            }),
        );
        thread::spawn(move || {
            let result = remove_residues(selected, backup_dir).map(|(removed, failed, bytes, dir)| {
                WorkerOutput::RemoveResidues {
                    removed,
                    failed,
                    quarantined_bytes: bytes,
                    backup_dir: dir,
                }
            });
            let _ = tx.send(result);
        });
    }

    pub fn start_remove_selected_apps(&mut self) {
        if self.busy {
            return;
        }
        let selected: Vec<_> = self
            .apps
            .iter()
            .filter(|app| app.checked && !app.non_removable && !app.is_framework)
            .cloned()
            .collect();
        if selected.is_empty() {
            self.status = "Nenhum Windows App removível selecionado.".into();
            return;
        }
        self.busy = true;
        self.status = format!("Removendo {} Windows App(s)…", selected.len());
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let mut removed = 0usize;
            let mut failed = Vec::new();
            for app in selected {
                let package = ps_quote(&app.package_full_name);
                let script = format!(
                    "Remove-AppxPackage -Package '{}' -ErrorAction Stop",
                    package
                );
                match run_powershell(&script, Duration::from_secs(120)) {
                    Ok(_) => removed += 1,
                    Err(error) => failed.push(format!("{}: {}", app.name, error)),
                }
            }
            let _ = tx.send(Ok(WorkerOutput::RemoveApps { removed, failed }));
        });
    }

    pub fn start_create_snapshot(&mut self, root: &Path) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = "Criando snapshot antes da instalação…".into();
        let path = root.join("config").join("uninstaller-install-snapshot.json");
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let snapshot = capture_install_snapshot()?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(
                &path,
                serde_json::to_vec_pretty(&snapshot).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let _ = tx.send(Ok(WorkerOutput::SnapshotCreated { path, snapshot }));
            Ok::<(), String>(())
        });
    }

    pub fn start_compare_snapshot(&mut self, root: &Path) {
        if self.busy {
            return;
        }
        let path = root.join("config").join("uninstaller-install-snapshot.json");
        if !path.is_file() {
            self.status = "Crie o snapshot antes de instalar o programa.".into();
            return;
        }
        self.busy = true;
        self.status = "Comparando sistema com o snapshot anterior…".into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let old: InstallSnapshot = serde_json::from_slice(
                &fs::read(&path).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let now = capture_install_snapshot()?;
            let old_keys: HashSet<_> = old.uninstall_keys.into_iter().collect();
            let old_dirs: HashSet<_> = old.program_dirs.into_iter().collect();
            let new_uninstall_keys = now
                .uninstall_keys
                .into_iter()
                .filter(|x| !old_keys.contains(x))
                .collect();
            let new_program_dirs = now
                .program_dirs
                .into_iter()
                .filter(|x| !old_dirs.contains(x))
                .collect();
            let _ = tx.send(Ok(WorkerOutput::SnapshotCompared {
                new_uninstall_keys,
                new_program_dirs,
            }));
            Ok::<(), String>(())
        });
    }

    pub fn mark_safe_residues(&mut self) {
        for item in &mut self.residues {
            item.checked = item.risk == ResidueRisk::Safe;
        }
    }

    pub fn clear_residue_selection(&mut self) {
        for item in &mut self.residues {
            item.checked = false;
        }
    }

    pub fn poll(&mut self, root: &Path) {
        let result = match self.rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Empty)) => return,
            Some(Err(TryRecvError::Disconnected)) => {
                self.rx = None;
                self.busy = false;
                self.status = "O worker do Faxina Uninstaller terminou inesperadamente.".into();
                return;
            }
            None => return,
        };
        self.rx = None;
        self.busy = false;

        match result {
            Ok(WorkerOutput::Programs(mut programs)) => {
                programs.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
                let count = programs.len();
                self.programs = programs;
                self.status = format!("{} programa(s) instalado(s) encontrado(s).", count);
                crate::diagnostics::operation_end(
                    "faxina_uninstaller",
                    "inventory_programs",
                    true,
                    0,
                    serde_json::json!({"count": count}),
                );
            }
            Ok(WorkerOutput::Apps(mut apps)) => {
                apps.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
                let count = apps.len();
                self.apps = apps;
                self.status = format!("{} Windows App(s) removível(eis)/visível(eis).", count);
                crate::diagnostics::operation_end(
                    "faxina_uninstaller",
                    "inventory_appx",
                    true,
                    0,
                    serde_json::json!({"count": count}),
                );
            }
            Ok(WorkerOutput::Processes(mut processes)) => {
                processes.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
                self.status = format!("{} processo(s) com caminho conhecido.", processes.len());
                self.processes = processes;
            }
            Ok(WorkerOutput::Residues { target, mut items }) => {
                for item in &mut items {
                    item.checked = item.risk == ResidueRisk::Safe;
                }
                items.sort_by(|a, b| {
                    risk_rank(a.risk)
                        .cmp(&risk_rank(b.risk))
                        .then_with(|| a.kind.label().cmp(b.kind.label()))
                        .then_with(|| a.path.cmp(&b.path))
                });
                let safe = items.iter().filter(|x| x.risk == ResidueRisk::Safe).count();
                let review = items.len().saturating_sub(safe);
                self.status = format!(
                    "{}: {} sobra(s) segura(s) + {} para revisão.",
                    target, safe, review
                );
                self.residues = items;
                crate::diagnostics::operation_end(
                    "faxina_uninstaller",
                    "scan_residues",
                    true,
                    0,
                    serde_json::json!({"target": target, "safe": safe, "review": review}),
                );
            }
            Ok(WorkerOutput::Uninstall { programs, results }) => {
                let mut success = 0usize;
                for (name, ok, detail) in &results {
                    if *ok {
                        success += 1;
                    }
                    append_history(
                        root,
                        HistoryEntry {
                            time: now_epoch(),
                            action: "Desinstalação oficial".into(),
                            target: name.clone(),
                            detail: detail.clone(),
                            success: *ok,
                        },
                    );
                }
                self.history = load_history(root);
                self.status = format!(
                    "Fila concluída: {}/{} desinstalador(es) terminaram com sucesso. Atualizando lista…",
                    success,
                    results.len()
                );
                crate::diagnostics::operation_end(
                    "faxina_uninstaller",
                    "uninstall_queue",
                    success == results.len(),
                    0,
                    serde_json::json!({"success": success, "total": results.len(), "results": results}),
                );
                self.programs = programs;
                self.start_refresh_programs();
            }
            Ok(WorkerOutput::RemoveResidues {
                removed,
                failed,
                quarantined_bytes,
                backup_dir,
            }) => {
                append_history(
                    root,
                    HistoryEntry {
                        time: now_epoch(),
                        action: "Remoção de sobras".into(),
                        target: self.last_target.clone(),
                        detail: format!(
                            "{} removidas; {} falhas; quarentena {}",
                            removed,
                            failed.len(),
                            backup_dir.display()
                        ),
                        success: failed.is_empty(),
                    },
                );
                self.history = load_history(root);
                self.status = format!(
                    "{} sobra(s) removida(s) • {} em quarentena • {} falha(s).",
                    removed,
                    fmt_bytes(quarantined_bytes),
                    failed.len()
                );
                for item in &mut self.residues {
                    item.checked = false;
                }
                self.residues.retain(|item| {
                    !failed.iter().any(|failure| failure.contains(&item.path))
                });
                crate::diagnostics::operation_end(
                    "faxina_uninstaller",
                    "remove_residues",
                    failed.is_empty(),
                    0,
                    serde_json::json!({
                        "removed": removed,
                        "failed": failed,
                        "quarantined_bytes": quarantined_bytes,
                        "backup": backup_dir.to_string_lossy()
                    }),
                );
            }
            Ok(WorkerOutput::RemoveApps { removed, failed }) => {
                self.status = format!(
                    "{} Windows App(s) removido(s) • {} falha(s).",
                    removed,
                    failed.len()
                );
                append_history(
                    root,
                    HistoryEntry {
                        time: now_epoch(),
                        action: "Remoção Windows App".into(),
                        target: format!("{} pacote(s)", removed),
                        detail: failed.join(" | "),
                        success: failed.is_empty(),
                    },
                );
                self.history = load_history(root);
                self.start_refresh_apps();
            }
            Ok(WorkerOutput::SnapshotCreated { path, snapshot }) => {
                self.status = format!(
                    "Snapshot salvo: {} • {} chaves de uninstall • {} diretórios-base.",
                    path.display(),
                    snapshot.uninstall_keys.len(),
                    snapshot.program_dirs.len()
                );
            }
            Ok(WorkerOutput::SnapshotCompared {
                new_uninstall_keys,
                new_program_dirs,
            }) => {
                self.monitor_result.clear();
                for key in &new_uninstall_keys {
                    self.monitor_result.push(format!("Nova entrada de desinstalação: {key}"));
                }
                for dir in &new_program_dirs {
                    self.monitor_result.push(format!("Novo diretório: {dir}"));
                }
                self.status = format!(
                    "Comparação concluída: {} nova(s) chave(s) de uninstall e {} novo(s) diretório(s).",
                    new_uninstall_keys.len(),
                    new_program_dirs.len()
                );
            }
            Err(error) => {
                self.status = format!("Falha: {error}");
                crate::diagnostics::event(
                    "uninstaller_error",
                    "Faxina Uninstaller",
                    serde_json::json!({"error": error}),
                );
            }
        }
    }
}

fn risk_rank(risk: ResidueRisk) -> u8 {
    match risk {
        ResidueRisk::Safe => 0,
        ResidueRisk::Review => 1,
    }
}

fn identity_from_program(program: &InstalledProgram) -> ProgramIdentity {
    ProgramIdentity {
        name: program.name.clone(),
        publisher: program.publisher.clone(),
        install_location: program.install_location.clone(),
        uninstall_string: program.uninstall_string.clone(),
        display_icon: program.display_icon.clone(),
        registry_path: program.registry_path.clone(),
    }
}

fn scan_installed_programs() -> Result<Vec<InstalledProgram>, String> {
    const SCRIPT: &str = r#"
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$roots=@(
 'Registry::HKEY_LOCAL_MACHINE\Software\Microsoft\Windows\CurrentVersion\Uninstall',
 'Registry::HKEY_LOCAL_MACHINE\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
 'Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Uninstall'
)
$items=foreach($root in $roots){
 if(Test-Path -LiteralPath $root){
  Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue | ForEach-Object {
   try{
    $p=Get-ItemProperty -LiteralPath $_.PSPath -ErrorAction Stop
    if(-not [string]::IsNullOrWhiteSpace([string]$p.DisplayName) -and -not $p.SystemComponent){
     [PSCustomObject]@{
      Name=[string]$p.DisplayName
      Version=[string]$p.DisplayVersion
      Publisher=[string]$p.Publisher
      InstallLocation=[string]$p.InstallLocation
      UninstallString=[string]$p.UninstallString
      QuietUninstallString=[string]$p.QuietUninstallString
      DisplayIcon=[string]$p.DisplayIcon
      RegistryPath=[string]$_.Name
      EstimatedSizeKb=[uint64]($p.EstimatedSize -as [uint64])
      WindowsInstaller=([int]$p.WindowsInstaller -eq 1)
     }
    }
   }catch{}
  }
 }
}
$json=foreach($item in @($items)){ConvertTo-Json -InputObject $item -Compress -Depth 4}
[Console]::Write('['+($json -join ',')+']')
"#;
    run_ps_json(SCRIPT, Duration::from_secs(25))
}

fn scan_windows_apps() -> Result<Vec<WindowsApp>, String> {
    const SCRIPT: &str = r#"
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$items=Get-AppxPackage -ErrorAction SilentlyContinue | Where-Object { $_.Name -and -not $_.IsResourcePackage } | ForEach-Object {
 [PSCustomObject]@{
  Name=[string]$_.Name
  PackageFullName=[string]$_.PackageFullName
  Version=[string]$_.Version
  Publisher=[string]$_.Publisher
  InstallLocation=[string]$_.InstallLocation
  NonRemovable=[bool]$_.NonRemovable
  IsFramework=[bool]$_.IsFramework
 }
}
$json=foreach($item in @($items)){ConvertTo-Json -InputObject $item -Compress -Depth 4}
[Console]::Write('['+($json -join ',')+']')
"#;
    run_ps_json(SCRIPT, Duration::from_secs(30))
}

fn scan_processes() -> Result<Vec<ProcessItem>, String> {
    const SCRIPT: &str = r#"
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$items=Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | ForEach-Object {
 if(-not [string]::IsNullOrWhiteSpace([string]$_.ExecutablePath)){
  [PSCustomObject]@{Name=[string]$_.Name;Id=[uint32]$_.ProcessId;Path=[string]$_.ExecutablePath}
 }
}
$json=foreach($item in @($items)){ConvertTo-Json -InputObject $item -Compress -Depth 3}
[Console]::Write('['+($json -join ',')+']')
"#;
    run_ps_json(SCRIPT, Duration::from_secs(20))
}

fn scan_residues(identity: &ProgramIdentity) -> Result<Vec<ResidueItem>, String> {
    let mut items = Vec::new();
    let install = expand_env(&identity.install_location);
    let display_icon = extract_path_from_command(&identity.display_icon).unwrap_or_default();

    if !identity.registry_path.trim().is_empty() {
        let exists = reg_exists(&identity.registry_path);
        if exists {
            items.push(ResidueItem {
                risk: ResidueRisk::Safe,
                kind: ResidueKind::RegistryKey,
                path: identity.registry_path.clone(),
                value_name: String::new(),
                reason: "Entrada de desinstalação ainda presente.".into(),
                evidence: "A chave é exatamente a entrada Uninstall do programa selecionado.".into(),
                size: 0,
                checked: true,
                error: None,
            });
        }
    }

    let install_path = PathBuf::from(&install);
    if !install.is_empty() && install_path.exists() {
        items.push(ResidueItem {
            risk: ResidueRisk::Review,
            kind: if install_path.is_dir() {
                ResidueKind::Folder
            } else {
                ResidueKind::File
            },
            path: install.clone(),
            value_name: String::new(),
            reason: "Pasta/arquivo original da instalação ainda existe.".into(),
            evidence: "É o InstallLocation registrado pelo programa; pode conter dados do usuário, por isso exige revisão.".into(),
            size: path_size(&install_path),
            checked: false,
            error: None,
        });
    }

    let name_tokens = identity_tokens(&identity.name, &identity.publisher);
    for base in candidate_user_data_roots() {
        if !base.is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&base) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = path
                    .file_name()
                    .map(|x| normalize_token(&x.to_string_lossy()))
                    .unwrap_or_default();
                if file_name.len() < 3 {
                    continue;
                }
                if name_tokens.iter().any(|token| token.len() >= 3 && file_name == *token) {
                    items.push(ResidueItem {
                        risk: ResidueRisk::Review,
                        kind: if path.is_dir() {
                            ResidueKind::Folder
                        } else {
                            ResidueKind::File
                        },
                        path: path.to_string_lossy().to_string(),
                        value_name: String::new(),
                        reason: "Dados remanescentes com nome correspondente ao programa/fabricante.".into(),
                        evidence: format!("Nome da entrada coincide com identidade normalizada: {}", file_name),
                        size: path_size(&path),
                        checked: false,
                        error: None,
                    });
                }
            }
        }
    }

    let script = build_registry_residue_script(identity, &install, &display_icon);
    if let Ok(extra) = run_ps_json::<ResidueItem>(&script, Duration::from_secs(30)) {
        items.extend(extra);
    }

    dedupe_residues(&mut items);
    Ok(items)
}

fn build_registry_residue_script(
    identity: &ProgramIdentity,
    install: &str,
    display_icon: &str,
) -> String {
    let install = ps_quote(install);
    let icon = ps_quote(display_icon);
    let name = ps_quote(&identity.name);
    let publisher = ps_quote(&identity.publisher);
    format!(
        r#"
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$install='{install}'
$icon='{icon}'
$name='{name}'
$publisher='{publisher}'
$items=New-Object System.Collections.ArrayList
function Add-R([string]$risk,[string]$kind,[string]$path,[string]$value,[string]$reason,[string]$evidence){{
 [void]$items.Add([PSCustomObject]@{{risk=$risk;kind=$kind;path=$path;value_name=$value;reason=$reason;evidence=$evidence;size=0}})
}}
function Exp([string]$s){{if([string]::IsNullOrWhiteSpace($s)){{return ''}};[Environment]::ExpandEnvironmentVariables($s.Trim())}}
function CmdPath([string]$s){{
 $s=Exp $s
 if($s -match '^\s*"([^"]+\.(exe|com|cmd|bat))"'){{return $matches[1]}}
 if($s -match '^\s*([^\s,]+\.(exe|com|cmd|bat))'){{return $matches[1]}}
 return ''
}}
$runRoots=@(
 @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Run','HKCU\Software\Microsoft\Windows\CurrentVersion\Run'),
 @('HKLM:\Software\Microsoft\Windows\CurrentVersion\Run','HKLM\Software\Microsoft\Windows\CurrentVersion\Run'),
 @('HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run','HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run')
)
foreach($r in $runRoots){{
 $k=Get-Item -LiteralPath $r[0] -ErrorAction SilentlyContinue
 if($k){{foreach($vn in $k.GetValueNames()){{$raw=[string]$k.GetValue($vn);$p=CmdPath $raw;if($p -and -not(Test-Path -LiteralPath $p) -and (($install -and $raw -like ('*'+$install+'*')) -or ($icon -and $p -eq $icon))){{Add-R 'Safe' 'RegistryValue' $r[1] $vn 'Inicialização órfã do programa' 'O comando referencia o programa selecionado e o executável não existe.'}}}}}}
}}
$appRoots=@(
 @('HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths','HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths'),
 @('HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths','HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths'),
 @('HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths','HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths')
)
foreach($r in $appRoots){{
 if(Test-Path -LiteralPath $r[0]){{Get-ChildItem -LiteralPath $r[0] -ErrorAction SilentlyContinue|ForEach-Object{{$v=[string](Get-Item -LiteralPath $_.PSPath).GetValue('');$ev=Exp $v;if($ev -and -not(Test-Path -LiteralPath $ev) -and (($install -and $ev -like ($install+'*')) -or ($icon -and $ev -eq $icon))){{Add-R 'Safe' 'RegistryKey' ($r[1]+'\'+$_.PSChildName) '' 'App Paths órfão' 'O executável registrado não existe e pertence ao programa selecionado.'}}}}}}
}}
$serviceItems=Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | Where-Object {{ $install -and $_.PathName -like ('*'+$install+'*') }}
foreach($s in $serviceItems){{Add-R 'Review' 'Service' ([string]$s.Name) '' 'Serviço relacionado à instalação' ([string]$s.PathName)}}
$taskItems=Get-ScheduledTask -ErrorAction SilentlyContinue
foreach($task in $taskItems){{foreach($a in @($task.Actions)){{if($install -and [string]$a.Execute -like ($install+'*')){{Add-R 'Review' 'Task' ([string]$task.TaskPath+[string]$task.TaskName) '' 'Tarefa agendada relacionada à instalação' ([string]$a.Execute);break}}}}}}
$json=foreach($item in @($items)){{
 $o=[PSCustomObject]@{{risk=[string]$item.risk;kind=[string]$item.kind;path=[string]$item.path;value_name=[string]$item.value_name;reason=[string]$item.reason;evidence=[string]$item.evidence;size=[uint64]$item.size}}
 ConvertTo-Json -InputObject $o -Compress -Depth 4
}}
[Console]::Write('['+($json -join ',')+']')
"#
    )
}

fn dedupe_residues(items: &mut Vec<ResidueItem>) {
    let mut seen = HashSet::new();
    items.retain(|item| {
        seen.insert(format!(
            "{:?}|{:?}|{}|{}",
            item.kind,
            item.risk,
            item.path.to_ascii_lowercase(),
            item.value_name.to_ascii_lowercase()
        ))
    });
}

fn official_uninstall_command(program: &InstalledProgram) -> Option<String> {
    let mut command = program.uninstall_string.trim().to_string();
    if command.is_empty() && program.windows_installer {
        if let Some(product) = program.registry_path.rsplit('\\').next() {
            if product.starts_with('{') && product.ends_with('}') {
                command = format!("MsiExec.exe /X {}", product);
            }
        }
    }
    if command.is_empty() {
        return None;
    }

    let lower = command.to_ascii_lowercase();
    if lower.contains("msiexec") {
        command = replace_msi_install_with_uninstall(&command);
    }
    Some(command)
}

fn replace_msi_install_with_uninstall(command: &str) -> String {
    let mut out = command.to_string();
    for pattern in ["/I{", "/i{", "/I ", "/i "] {
        if let Some(pos) = out.find(pattern) {
            let replacement = if pattern.ends_with('{') { "/X{" } else { "/X " };
            out.replace_range(pos..pos + pattern.len(), replacement);
            break;
        }
    }
    out
}

fn run_interactive_uninstall(command: &str) -> Result<String, String> {
    let status = Command::new("cmd.exe")
        .args(["/C", command])
        .status()
        .map_err(|e| format!("Falha ao iniciar desinstalador: {e}"))?;
    if status.success() {
        Ok(format!("Concluído com código {:?}", status.code()))
    } else {
        Err(format!("Desinstalador terminou com código {:?}", status.code()))
    }
}

fn remove_residues(
    selected: Vec<ResidueItem>,
    backup_dir: PathBuf,
) -> Result<(usize, Vec<String>, u64, PathBuf), String> {
    fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
    let reg_dir = backup_dir.join("registry");
    let quarantine = backup_dir.join("quarantine");
    fs::create_dir_all(&reg_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&quarantine).map_err(|e| e.to_string())?;

    let mut removed = 0usize;
    let mut failed = Vec::new();
    let mut quarantined_bytes = 0u64;
    let mut exported = HashSet::new();

    for (index, item) in selected.iter().enumerate() {
        let result = match item.kind {
            ResidueKind::RegistryKey | ResidueKind::RegistryValue => {
                if exported.insert(item.path.clone()) {
                    let backup = reg_dir.join(format!("reg-{:03}.reg", exported.len()));
                    let args = vec![
                        "export".to_string(),
                        item.path.clone(),
                        backup.to_string_lossy().to_string(),
                        "/y".to_string(),
                    ];
                    if let Err(error) = run_reg(&args) {
                        Err(format!("backup bloqueou remoção: {error}"))
                    } else {
                        delete_registry_residue(item)
                    }
                } else {
                    delete_registry_residue(item)
                }
            }
            ResidueKind::Folder | ResidueKind::File => {
                let source = PathBuf::from(&item.path);
                if !source.exists() {
                    Ok(())
                } else {
                    let bytes = path_size(&source);
                    let name = source
                        .file_name()
                        .map(|x| x.to_string_lossy().to_string())
                        .unwrap_or_else(|| format!("item-{index}"));
                    let destination = quarantine.join(format!("{:03}-{}", index + 1, sanitize_name(&name)));
                    match quarantine_path(&source, &destination) {
                        Ok(()) => {
                            quarantined_bytes = quarantined_bytes.saturating_add(bytes);
                            Ok(())
                        }
                        Err(error) => Err(error),
                    }
                }
            }
            ResidueKind::Service => {
                run_process_hidden("sc.exe", &["delete", &item.path]).map(|_| ())
            }
            ResidueKind::Task => {
                run_process_hidden("schtasks.exe", &["/Delete", "/TN", &item.path, "/F"]).map(|_| ())
            }
        };

        match result {
            Ok(()) => removed += 1,
            Err(error) => failed.push(format!("{}: {}", item.path, error)),
        }
    }

    let manifest = serde_json::to_vec_pretty(&serde_json::json!({
        "created": now_epoch(),
        "removed": removed,
        "failed": failed,
        "items": selected
    }))
    .map_err(|e| e.to_string())?;
    fs::write(backup_dir.join("manifest.json"), manifest).map_err(|e| e.to_string())?;

    Ok((removed, failed, quarantined_bytes, backup_dir))
}

fn delete_registry_residue(item: &ResidueItem) -> Result<(), String> {
    let mut args = vec!["delete".to_string(), item.path.clone()];
    if item.kind == ResidueKind::RegistryValue {
        if item.value_name.is_empty() {
            args.push("/ve".into());
        } else {
            args.push("/v".into());
            args.push(item.value_name.clone());
        }
    }
    args.push("/f".into());
    run_reg(&args).map(|_| ())
}

fn quarantine_path(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if fs::rename(source, destination).is_ok() {
        return Ok(());
    }

    if source.is_dir() {
        copy_dir_recursive(source, destination)?;
        fs::remove_dir_all(source).map_err(|e| e.to_string())?;
    } else {
        fs::copy(source, destination).map_err(|e| e.to_string())?;
        fs::remove_file(source).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src = entry.path();
        let dst = destination.join(entry.file_name());
        let meta = fs::symlink_metadata(&src).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            copy_dir_recursive(&src, &dst)?;
        } else {
            fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn capture_install_snapshot() -> Result<InstallSnapshot, String> {
    let programs = scan_installed_programs()?;
    let mut uninstall_keys = programs
        .into_iter()
        .map(|p| p.registry_path)
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>();
    uninstall_keys.sort();
    uninstall_keys.dedup();

    let mut program_dirs = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA", "APPDATA"] {
        if let Some(root) = env::var_os(var).map(PathBuf::from) {
            if let Ok(entries) = fs::read_dir(root) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        program_dirs.push(entry.path().to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    program_dirs.sort();
    program_dirs.dedup();

    Ok(InstallSnapshot {
        created: now_epoch(),
        uninstall_keys,
        program_dirs,
    })
}

fn candidate_user_data_roots() -> Vec<PathBuf> {
    ["LOCALAPPDATA", "APPDATA", "PROGRAMDATA"]
        .into_iter()
        .filter_map(|name| env::var_os(name).map(PathBuf::from))
        .collect()
}

fn identity_tokens(name: &str, publisher: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for value in [name, publisher] {
        let normalized = normalize_token(value);
        if normalized.len() >= 3 {
            tokens.push(normalized);
        }
    }
    tokens.sort();
    tokens.dedup();
    tokens
}

fn normalize_token(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn expand_env(value: &str) -> String {
    let value = value.trim().trim_matches('"');
    if value.is_empty() {
        return String::new();
    }
    let script = format!(
        "[Console]::Write([Environment]::ExpandEnvironmentVariables('{}'))",
        ps_quote(value)
    );
    run_powershell(&script, Duration::from_secs(5))
        .unwrap_or_else(|_| value.to_string())
        .trim()
        .to_string()
}

fn extract_path_from_command(value: &str) -> Option<String> {
    let expanded = expand_env(value);
    if expanded.is_empty() {
        return None;
    }
    let s = expanded.trim();
    if let Some(rest) = s.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_string());
        }
    }
    for ext in [".exe", ".dll", ".com", ".bat", ".cmd"] {
        if let Some(pos) = s.to_ascii_lowercase().find(ext) {
            return Some(s[..pos + ext.len()].trim().to_string());
        }
    }
    Some(s.to_string())
}

fn reg_exists(path: &str) -> bool {
    run_reg(&["query".to_string(), path.to_string()]).is_ok()
}

fn run_reg(args: &[String]) -> Result<String, String> {
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_process_hidden("reg.exe", &refs)
}

fn run_process_hidden(program: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    if output.status.success() {
        Ok(text)
    } else {
        Err(text.trim().to_string())
    }
}

fn run_powershell(script: &str, timeout: Duration) -> Result<String, String> {
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_string(&mut stdout);
                }
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                if status.success() {
                    return Ok(stdout.trim_start_matches('\u{feff}').trim().to_string());
                }
                return Err(if stderr.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    stderr.trim().to_string()
                });
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("Tempo limite de {} s excedido.", timeout.as_secs()));
                }
                thread::sleep(Duration::from_millis(80));
            }
            Err(error) => {
                let _ = child.kill();
                return Err(error.to_string());
            }
        }
    }
}

fn run_ps_json<T>(script: &str, timeout: Duration) -> Result<Vec<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    let text = run_powershell(script, timeout)?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&text).map_err(|e| format!("JSON inválido: {e}\n{text}"))
}

fn path_size(path: &Path) -> u64 {
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
            total = total.saturating_add(path_size(&entry.path()));
        }
    }
    total
}

fn append_history(root: &Path, entry: HistoryEntry) {
    let dir = root.join("logs");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("uninstaller-history.jsonl");
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        if let Ok(line) = serde_json::to_string(&entry) {
            let _ = writeln!(file, "{line}");
        }
    }
}

fn load_history(root: &Path) -> Vec<HistoryEntry> {
    let path = root.join("logs").join("uninstaller-history.jsonl");
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut entries = text
        .lines()
        .filter_map(|line| serde_json::from_str::<HistoryEntry>(line).ok())
        .collect::<Vec<_>>();
    if entries.len() > 200 {
        entries.drain(0..entries.len() - 200);
    }
    entries.reverse();
    entries
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect()
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn timestamp_slug() -> String {
    format!("backup-{}", now_epoch())
}

fn ps_quote(value: &str) -> String {
    value.replace('\'', "''")
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
