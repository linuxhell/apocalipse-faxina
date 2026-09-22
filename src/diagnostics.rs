use serde_json::{json, Value};
use std::{
    backtrace::Backtrace,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver},
        Mutex, OnceLock,
    },
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

static LOGGER: OnceLock<DiagnosticLogger> = OnceLock::new();
static SUPPRESS_NEXT_UI_STALL: AtomicBool = AtomicBool::new(false);

pub struct DiagnosticLogger {
    root: PathBuf,
    session_dir: PathBuf,
    session_id: String,
    started: Instant,
    file: Mutex<File>,
    event_count: AtomicU64,
    last_event: Mutex<String>,
}

pub struct ExportState {
    pub running: bool,
    pub status: String,
    pub last_path: Option<PathBuf>,
    pub last_ok: bool,
    rx: Option<Receiver<Result<PathBuf, String>>>,
}

impl Default for ExportState {
    fn default() -> Self {
        Self {
            running: false,
            status: String::new(),
            last_path: None,
            last_ok: true,
            rx: None,
        }
    }
}

impl ExportState {
    pub fn start(&mut self, portable_root: PathBuf) {
        if self.running {
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.running = true;
        self.last_ok = true;
        self.status = "Exportando diagnóstico em segundo plano…".into();
        operation_start("diagnostico", "export_zip", json!({}));

        thread::spawn(move || {
            let started = Instant::now();
            let result = export_zip_worker(&portable_root);
            match &result {
                Ok(path) => operation_end(
                    "diagnostico",
                    "export_zip",
                    true,
                    started.elapsed().as_millis(),
                    json!({"path": path.to_string_lossy()}),
                ),
                Err(error) => operation_end(
                    "diagnostico",
                    "export_zip",
                    false,
                    started.elapsed().as_millis(),
                    json!({"error": error}),
                ),
            }
            let _ = tx.send(result);
        });
    }

    pub fn poll(&mut self) {
        let result = self.rx.as_ref().and_then(|rx| rx.try_recv().ok());
        let Some(result) = result else {
            return;
        };

        self.running = false;
        self.rx = None;
        match result {
            Ok(path) => {
                self.last_ok = true;
                self.status = "Diagnóstico ZIP exportado com sucesso.".into();
                self.last_path = Some(path);
            }
            Err(error) => {
                self.last_ok = false;
                self.status = format!("Falha ao exportar diagnóstico: {error}");
                self.last_path = None;
            }
        }
    }
}

pub fn init(portable_root: &Path) {
    if LOGGER.get().is_some() {
        return;
    }

    let session_id = format!("{}-{}", unix_seconds(), std::process::id());
    let root = portable_root.join("logs").join("diagnostics");
    let session_dir = root.join(&session_id);
    let _ = fs::create_dir_all(&session_dir);
    rotate_sessions(&root, 12);

    let Ok(file) = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(session_dir.join("session.jsonl"))
    else {
        return;
    };

    let _ = LOGGER.set(DiagnosticLogger {
        root,
        session_dir,
        session_id,
        started: Instant::now(),
        file: Mutex::new(file),
        event_count: AtomicU64::new(0),
        last_event: Mutex::new(String::new()),
    });

    install_panic_hook();
    event(
        "session_start",
        "Apocalipse Faxina iniciado",
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "arch": std::env::consts::ARCH,
            "os": std::env::consts::OS
        }),
    );
}

pub fn event(kind: &str, message: &str, data: Value) {
    let Some(logger) = LOGGER.get() else {
        return;
    };

    let seq = logger.event_count.fetch_add(1, Ordering::Relaxed) + 1;
    let row = json!({
        "seq": seq,
        "unix_ms": unix_millis(),
        "elapsed_ms": logger.started.elapsed().as_millis() as u64,
        "kind": kind,
        "message": message,
        "data": data
    });

    if let Ok(mut last) = logger.last_event.lock() {
        *last = format!("{kind}: {message}");
    }
    if let Ok(mut file) = logger.file.lock() {
        let _ = writeln!(file, "{}", row);
        let _ = file.flush();
    }
}

pub fn ui_click(section: &str, x: f32, y: f32, secondary: bool) {
    event(
        "ui_click",
        "Clique na interface",
        json!({"section":section,"x":x,"y":y,"secondary":secondary}),
    );
}

pub fn suppress_next_ui_stall() {
    SUPPRESS_NEXT_UI_STALL.store(true, Ordering::Relaxed);
}

pub fn frame_finished(section: &str, elapsed_ms: u128) {
    if SUPPRESS_NEXT_UI_STALL.swap(false, Ordering::Relaxed) {
        return;
    }
    if elapsed_ms >= 350 {
        event(
            "ui_stall",
            "Frame da interface demorou acima do limite",
            json!({
                "section": section,
                "elapsed_ms": elapsed_ms,
                "severity": if elapsed_ms >= 2000 { "critical" } else { "warning" }
            }),
        );
    }
}

pub fn operation_start(area: &str, operation: &str, data: Value) {
    event("operation_start", &format!("{area}: {operation}"), data);
}

pub fn operation_end(area: &str, operation: &str, ok: bool, elapsed_ms: u128, data: Value) {
    event(
        if ok { "operation_success" } else { "operation_error" },
        &format!("{area}: {operation}"),
        json!({"ok":ok,"elapsed_ms":elapsed_ms,"details":data}),
    );
}

pub fn session_id() -> String {
    LOGGER
        .get()
        .map(|x| x.session_id.clone())
        .unwrap_or_else(|| "inativo".into())
}

pub fn session_dir() -> Option<PathBuf> {
    LOGGER.get().map(|x| x.session_dir.clone())
}

pub fn logs_root() -> Option<PathBuf> {
    LOGGER.get().map(|x| x.root.clone())
}

pub fn event_count() -> u64 {
    LOGGER
        .get()
        .map(|x| x.event_count.load(Ordering::Relaxed))
        .unwrap_or(0)
}

pub fn last_event() -> String {
    LOGGER
        .get()
        .and_then(|x| x.last_event.lock().ok().map(|v| v.clone()))
        .unwrap_or_default()
}

fn export_zip_worker(portable_root: &Path) -> Result<PathBuf, String> {
    let logger = LOGGER
        .get()
        .ok_or_else(|| "Diagnóstico não inicializado.".to_string())?;

    let out_dir = portable_root.join("diagnostics");
    fs::create_dir_all(&out_dir)
        .map_err(|error| format!("Falha ao criar pasta de diagnósticos: {error}"))?;

    let staging = out_dir.join(format!(".staging-{}", logger.session_id));
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Falha ao criar snapshot do diagnóstico: {error}"))?;

    let session_bytes = {
        let mut file = logger
            .file
            .lock()
            .map_err(|_| "Falha ao bloquear o arquivo de log para snapshot.".to_string())?;
        file.flush()
            .map_err(|error| format!("Falha ao sincronizar session.jsonl: {error}"))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|error| format!("Falha ao posicionar session.jsonl: {error}"))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("Falha ao ler snapshot de session.jsonl: {error}"))?;
        let _ = file.seek(SeekFrom::End(0));
        bytes
    };

    fs::write(staging.join("session.jsonl"), session_bytes)
        .map_err(|error| format!("Falha ao gravar snapshot de session.jsonl: {error}"))?;

    if let Ok(entries) = fs::read_dir(&logger.session_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.file_name().is_some_and(|name| name == "session.jsonl") {
                continue;
            }
            if let Some(name) = path.file_name() {
                let _ = fs::copy(&path, staging.join(name));
            }
        }
    }

    let inventory = staging.join("inventory");
    fs::create_dir_all(&inventory)
        .map_err(|error| format!("Falha ao criar inventário: {error}"))?;

    let captures = [
        (
            "system.json",
            "Get-ComputerInfo | Select WindowsProductName,WindowsVersion,OsBuildNumber,OsArchitecture,CsManufacturer,CsModel,CsTotalPhysicalMemory,BiosSMBIOSBIOSVersion | ConvertTo-Json -Depth 4",
        ),
        (
            "processes.json",
            "Get-Process | Sort ProcessName | Select ProcessName,Id,CPU,WorkingSet64,Path | ConvertTo-Json -Depth 4",
        ),
        (
            "services.json",
            "Get-CimInstance Win32_Service | Select Name,DisplayName,State,StartMode,StartName,PathName | ConvertTo-Json -Depth 4",
        ),
        (
            "startup.json",
            "Get-CimInstance Win32_StartupCommand | Select Name,Command,Location,User | ConvertTo-Json -Depth 4",
        ),
        (
            "tasks.json",
            "Get-ScheduledTask | Select TaskName,TaskPath,State,Author | ConvertTo-Json -Depth 4",
        ),
        (
            "storage.json",
            "$a=Get-Volume | Select DriveLetter,FileSystemLabel,FileSystem,HealthStatus,Size,SizeRemaining; $b=Get-Disk | Select Number,FriendlyName,BusType,HealthStatus,OperationalStatus,Size; $c=Get-PhysicalDisk | Select FriendlyName,MediaType,BusType,HealthStatus,Size; [PSCustomObject]@{Volumes=$a;Disks=$b;PhysicalDisks=$c}|ConvertTo-Json -Depth 5",
        ),
        (
            "recent-errors.json",
            "Get-WinEvent -FilterHashtable @{LogName='Application','System'; Level=1,2,3; StartTime=(Get-Date).AddDays(-2)} -ErrorAction SilentlyContinue | Select -First 250 TimeCreated,LogName,ProviderName,Id,LevelDisplayName,Message | ConvertTo-Json -Depth 4",
        ),
    ];

    for (name, script) in captures {
        let content = run_powershell_result(script)
            .unwrap_or_else(|error| format!("ERRO AO COLETAR:\n{error}"));
        let _ = fs::write(inventory.join(name), content);
    }

    let driver_store = run_program_result("pnputil.exe", &["/enum-drivers"])
        .unwrap_or_else(|error| format!("ERRO AO COLETAR:\n{error}"));
    let _ = fs::write(inventory.join("driver-store.txt"), driver_store);

    let manifest = json!({
        "product": "Apocalipse Faxina",
        "version": env!("CARGO_PKG_VERSION"),
        "session_id": logger.session_id,
        "event_count": event_count(),
        "generated_unix_ms": unix_millis(),
        "snapshot": true,
        "privacy": "Senhas, cookies, credenciais e tokens não são coletados intencionalmente. Caminhos e inventário técnico são incluídos."
    });
    let _ = fs::write(
        staging.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
    );

    let destination = out_dir.join(format!(
        "Apocalipse-Faxina-Diagnostico-{}.zip",
        logger.session_id
    ));
    let _ = fs::remove_file(&destination);

    let source = ps_quote(&staging.to_string_lossy());
    let destination_ps = ps_quote(&destination.to_string_lossy());
    let compress = format!(
        "Compress-Archive -Path '{}\\*' -DestinationPath '{}' -CompressionLevel Optimal -Force",
        source, destination_ps
    );
    let result = run_powershell_result(&compress);
    let _ = fs::remove_dir_all(&staging);

    result.map_err(|error| format!("Falha ao gerar ZIP: {error}"))?;
    if !destination.is_file() {
        return Err("Compress-Archive terminou sem criar o ZIP.".into());
    }

    event(
        "diagnostic_export",
        "Diagnóstico exportado",
        json!({"path":destination.to_string_lossy()}),
    );
    Ok(destination)
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = if let Some(value) = info.payload().downcast_ref::<&str>() {
            (*value).to_string()
        } else if let Some(value) = info.payload().downcast_ref::<String>() {
            value.clone()
        } else {
            "panic sem mensagem textual".into()
        };
        let location = info
            .location()
            .map(|value| format!("{}:{}:{}", value.file(), value.line(), value.column()))
            .unwrap_or_else(|| "desconhecido".into());
        let backtrace = Backtrace::force_capture().to_string();

        event(
            "panic",
            "Panic capturado",
            json!({"payload":payload,"location":location,"backtrace":backtrace}),
        );

        if let Some(logger) = LOGGER.get() {
            let _ = fs::write(
                logger.session_dir.join("crash.txt"),
                format!(
                    "Apocalipse Faxina {}\nSessão: {}\nLocal: {}\nMensagem: {}\n\nBacktrace:\n{}\n",
                    env!("CARGO_PKG_VERSION"),
                    logger.session_id,
                    location,
                    payload,
                    backtrace
                ),
            );
        }

        previous(info);
    }));
}

fn run_powershell_result(script: &str) -> Result<String, String> {
    run_program_result(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )
}

fn run_program_result(program: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|error| format!("Falha ao executar {program}: {error}"))?;
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
        Err(format!(
            "{program} terminou com código {:?}:\n{}",
            output.status.code(),
            text
        ))
    }
}

fn ps_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn rotate_sessions(root: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut directories: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();

    if directories.len() > keep {
        let remove_count = directories.len() - keep;
        for path in directories.into_iter().take(remove_count) {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}
