use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptimizationMode {
    Intelligent,
    Quick,
    CompleteHdd,
    ConsolidateFree,
    HddDefrag,
    Retrim,
}

impl OptimizationMode {
    pub const ALL: [Self; 6] = [
        Self::Intelligent,
        Self::Quick,
        Self::CompleteHdd,
        Self::ConsolidateFree,
        Self::HddDefrag,
        Self::Retrim,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Intelligent => "Inteligente / recomendado",
            Self::Quick => "Rápido",
            Self::CompleteHdd => "Completo para HDD",
            Self::ConsolidateFree => "Consolidar espaço livre",
            Self::HddDefrag => "Desfragmentar HDD",
            Self::Retrim => "ReTRIM SSD/NVMe",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Intelligent => "Usa /O: o Windows escolhe a otimização adequada ao tipo de mídia.",
            Self::Quick => "HDD: desfragmentação direta. SSD/NVMe: ReTRIM. Menos etapas.",
            Self::CompleteHdd => "HDD: desfragmenta arquivos e depois consolida o espaço livre.",
            Self::ConsolidateFree => "HDD: consolida espaço livre para reduzir fragmentação futura.",
            Self::HddDefrag => "HDD: desfragmentação tradicional de arquivos com progresso.",
            Self::Retrim => "SSD/NVMe: envia ReTRIM; não executa desfragmentação tradicional agressiva.",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct VolumeInfo {
    pub drive: String,
    pub label: String,
    pub file_system: String,
    pub health: String,
    pub media_type: String,
    pub bus_type: String,
    pub size: u64,
    pub free: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub info: VolumeInfo,
    pub fragmentation_percent: Option<f32>,
    pub fragmented_files: Option<u64>,
    pub sequential_read_mbps: Option<f64>,
    pub random_read_mbps: Option<f64>,
    pub analysis_output: String,
}

enum Event {
    Phase(String),
    Progress(f32),
    Output(String),
    Before(Snapshot),
    After(Snapshot),
    Finished(String),
    Failed(String),
    Cancelled,
}

pub struct DefragState {
    pub selected_mode: usize,
    pub running: bool,
    pub progress: f32,
    pub phase: String,
    pub status: String,
    pub output: String,
    pub before: Option<Snapshot>,
    pub after: Option<Snapshot>,
    rx: Option<Receiver<Event>>,
    cancel: Arc<AtomicBool>,
}

impl Default for DefragState {
    fn default() -> Self {
        Self {
            selected_mode: 0,
            running: false,
            progress: 0.0,
            phase: "Pronto".into(),
            status: "Analise uma unidade ou inicie uma otimização.".into(),
            output: String::new(),
            before: None,
            after: None,
            rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl DefragState {
    pub fn selected_mode(&self) -> OptimizationMode {
        OptimizationMode::ALL
            .get(self.selected_mode)
            .copied()
            .unwrap_or(OptimizationMode::Intelligent)
    }

    pub fn analyze(&mut self, drive: String) {
        if self.running {
            return;
        }
        self.begin();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let cancel = self.cancel.clone();

        thread::spawn(move || {
            let _ = tx.send(Event::Phase("Analisando fragmentação".into()));
            let _ = tx.send(Event::Progress(10.0));
            match capture_snapshot(&drive, false) {
                Ok(snapshot) => {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = tx.send(Event::Cancelled);
                        return;
                    }
                    let _ = tx.send(Event::Before(snapshot.clone()));
                    let _ = tx.send(Event::Progress(100.0));
                    let _ = tx.send(Event::Finished(format!(
                        "Análise concluída para {}.",
                        drive
                    )));
                }
                Err(error) => {
                    let _ = tx.send(Event::Failed(error));
                }
            }
        });
    }

    pub fn optimize(&mut self, drive: String, mode: OptimizationMode) {
        if self.running {
            return;
        }
        self.begin();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let cancel = self.cancel.clone();

        thread::spawn(move || {
            let started = Instant::now();
            let _ = tx.send(Event::Phase("Medição antes da otimização".into()));
            let _ = tx.send(Event::Progress(2.0));

            let before = match capture_snapshot(&drive, true) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    let _ = tx.send(Event::Failed(error));
                    return;
                }
            };
            if cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Event::Cancelled);
                return;
            }
            let _ = tx.send(Event::Before(before.clone()));
            let _ = tx.send(Event::Progress(10.0));

            if let Err(error) = validate_mode(mode, &before.info) {
                let _ = tx.send(Event::Failed(error));
                return;
            }

            let commands = commands_for(mode, &before.info, &drive);
            let command_count = commands.len().max(1);
            let mut combined = String::new();

            for (index, args) in commands.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    let _ = tx.send(Event::Cancelled);
                    return;
                }

                let _ = tx.send(Event::Phase(format!(
                    "{} — etapa {}/{}",
                    mode.label(),
                    index + 1,
                    command_count
                )));

                let step_start = 10.0 + (index as f32 / command_count as f32) * 72.0;
                let step_span = 72.0 / command_count as f32;
                match run_defrag_stream(&drive, args, &tx, &cancel, step_start, step_span) {
                    Ok(output) => {
                        combined.push_str(&output);
                        combined.push('\n');
                    }
                    Err(error) => {
                        let _ = tx.send(Event::Failed(error));
                        return;
                    }
                }
            }

            if cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Event::Cancelled);
                return;
            }

            let _ = tx.send(Event::Output(combined));
            let _ = tx.send(Event::Phase("Medição depois da otimização".into()));
            let _ = tx.send(Event::Progress(86.0));

            match capture_snapshot(&drive, true) {
                Ok(after) => {
                    let _ = tx.send(Event::After(after));
                    let _ = tx.send(Event::Progress(100.0));
                    let _ = tx.send(Event::Finished(format!(
                        "{} concluído em {:.1} min.",
                        mode.label(),
                        started.elapsed().as_secs_f64() / 60.0
                    )));
                }
                Err(error) => {
                    let _ = tx.send(Event::Failed(format!(
                        "A otimização terminou, mas a medição final falhou: {error}"
                    )));
                }
            }
        });
    }

    pub fn cancel(&mut self) {
        if self.running {
            self.cancel.store(true, Ordering::Relaxed);
            self.phase = "Solicitando parada…".into();
        }
    }

    pub fn poll(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }

        for event in events {
            match event {
                Event::Phase(phase) => self.phase = phase,
                Event::Progress(progress) => self.progress = progress.clamp(0.0, 100.0),
                Event::Output(output) => {
                    if !self.output.is_empty() {
                        self.output.push('\n');
                    }
                    self.output.push_str(&output);
                }
                Event::Before(snapshot) => self.before = Some(snapshot),
                Event::After(snapshot) => self.after = Some(snapshot),
                Event::Finished(status) => {
                    self.running = false;
                    self.phase = "Concluído".into();
                    self.status = status;
                    self.rx = None;
                }
                Event::Failed(error) => {
                    self.running = false;
                    self.phase = "Falha".into();
                    self.status = error;
                    self.rx = None;
                }
                Event::Cancelled => {
                    self.running = false;
                    self.phase = "Interrompido".into();
                    self.status = "Operação interrompida pelo usuário.".into();
                    self.rx = None;
                }
            }
        }
    }

    pub fn current_snapshot(&self) -> Option<&Snapshot> {
        self.after.as_ref().or(self.before.as_ref())
    }

    fn begin(&mut self) {
        self.cancel.store(false, Ordering::Relaxed);
        self.running = true;
        self.progress = 0.0;
        self.phase = "Preparando".into();
        self.status = "Operação em andamento.".into();
        self.output.clear();
        self.before = None;
        self.after = None;
        self.rx = None;
    }
}

fn validate_mode(mode: OptimizationMode, info: &VolumeInfo) -> Result<(), String> {
    let media = info.media_type.to_ascii_lowercase();
    let is_ssd = media.contains("ssd") || media.contains("solid");
    let is_hdd = media.contains("hdd") || media.contains("hard disk");

    match mode {
        OptimizationMode::CompleteHdd
        | OptimizationMode::ConsolidateFree
        | OptimizationMode::HddDefrag
            if is_ssd =>
        {
            Err(format!(
                "{} foi bloqueado porque {} foi identificado como SSD. Use Inteligente ou ReTRIM.",
                mode.label(),
                info.drive
            ))
        }
        OptimizationMode::Retrim if is_hdd => Err(format!(
            "ReTRIM foi bloqueado porque {} foi identificado como HDD.",
            info.drive
        )),
        _ => Ok(()),
    }
}

fn commands_for(mode: OptimizationMode, info: &VolumeInfo, _drive: &str) -> Vec<Vec<String>> {
    let media = info.media_type.to_ascii_lowercase();
    let is_ssd = media.contains("ssd") || media.contains("solid");

    match mode {
        OptimizationMode::Intelligent => vec![vec!["/O".into(), "/U".into(), "/V".into()]],
        OptimizationMode::Quick if is_ssd => vec![vec!["/L".into(), "/U".into()]],
        OptimizationMode::Quick => vec![vec!["/D".into(), "/U".into()]],
        OptimizationMode::CompleteHdd => vec![
            vec!["/D".into(), "/U".into(), "/V".into()],
            vec!["/X".into(), "/U".into(), "/V".into()],
        ],
        OptimizationMode::ConsolidateFree => {
            vec![vec!["/X".into(), "/U".into(), "/V".into()]]
        }
        OptimizationMode::HddDefrag => vec![vec!["/D".into(), "/U".into(), "/V".into()]],
        OptimizationMode::Retrim => vec![vec!["/L".into(), "/U".into(), "/V".into()]],
    }
}

fn run_defrag_stream(
    drive: &str,
    args: &[String],
    tx: &Sender<Event>,
    cancel: &Arc<AtomicBool>,
    progress_start: f32,
    progress_span: f32,
) -> Result<String, String> {
    let mut command = Command::new("defrag.exe");
    command.arg(drive).args(args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Falha ao iniciar defrag.exe: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Não foi possível capturar a saída do defrag.exe.".to_string())?;

    let mut collected = String::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.unwrap_or_default();
        if !line.trim().is_empty() {
            collected.push_str(&line);
            collected.push('\n');
        }

        if let Some(percent) = extract_percent(&line) {
            let scaled = progress_start + progress_span * (percent / 100.0);
            let _ = tx.send(Event::Progress(scaled));
        }

        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Operação interrompida pelo usuário.".into());
        }
    }

    let status = child
        .wait()
        .map_err(|error| format!("Falha ao aguardar defrag.exe: {error}"))?;
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    if !stderr.trim().is_empty() {
        collected.push_str(&stderr);
    }

    if status.success() {
        Ok(collected)
    } else {
        Err(format!(
            "defrag.exe terminou com código {:?}.\n{}",
            status.code(),
            collected
        ))
    }
}

fn capture_snapshot(drive: &str, include_benchmark: bool) -> Result<Snapshot, String> {
    let info = query_volume_info(drive)?;
    let analysis_output = run_capture("defrag.exe", &[drive, "/A", "/V"])?;
    let fragmentation_percent = parse_fragmentation_percent(&analysis_output);
    let fragmented_files = parse_fragmented_files(&analysis_output);

    let (sequential_read_mbps, random_read_mbps) = if include_benchmark {
        (
            run_winsat_read(drive, false),
            run_winsat_read(drive, true),
        )
    } else {
        (None, None)
    };

    Ok(Snapshot {
        info,
        fragmentation_percent,
        fragmented_files,
        sequential_read_mbps,
        random_read_mbps,
        analysis_output,
    })
}

fn query_volume_info(drive: &str) -> Result<VolumeInfo, String> {
    let letter = drive
        .chars()
        .find(|c| c.is_ascii_alphabetic())
        .ok_or_else(|| "Unidade inválida.".to_string())?
        .to_ascii_uppercase();

    let script = format!(
        r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$v = Get-Volume -DriveLetter '{letter}' -ErrorAction Stop
$p = Get-Partition -DriveLetter '{letter}' -ErrorAction SilentlyContinue
$disk = $null
$pd = $null
if($p) {{
  $disk = Get-Disk -Number $p.DiskNumber -ErrorAction SilentlyContinue
  if($disk) {{
    $pd = Get-PhysicalDisk -ErrorAction SilentlyContinue |
      Where-Object {{ ([string]$_.DeviceId -eq [string]$disk.Number) -or ($_.FriendlyName -eq $disk.FriendlyName) }} |
      Select-Object -First 1
  }}
}}
[PSCustomObject]@{{
  Drive = '{letter}:'
  Label = [string]$v.FileSystemLabel
  FileSystem = [string]$v.FileSystem
  Health = [string]$v.HealthStatus
  MediaType = if($pd) {{ [string]$pd.MediaType }} else {{ 'Unspecified' }}
  BusType = if($disk) {{ [string]$disk.BusType }} else {{ '' }}
  Size = [uint64]$v.Size
  Free = [uint64]$v.SizeRemaining
}} | ConvertTo-Json -Compress
"#
    );

    let output = run_capture("powershell.exe", &["-NoProfile", "-Command", &script])?;
    let value: Value =
        serde_json::from_str(output.trim()).map_err(|error| format!("Falha ao interpretar unidade: {error}\n{output}"))?;

    Ok(VolumeInfo {
        drive: value["Drive"].as_str().unwrap_or(drive).to_string(),
        label: value["Label"].as_str().unwrap_or("").to_string(),
        file_system: value["FileSystem"].as_str().unwrap_or("").to_string(),
        health: value["Health"].as_str().unwrap_or("").to_string(),
        media_type: value["MediaType"].as_str().unwrap_or("Unspecified").to_string(),
        bus_type: value["BusType"].as_str().unwrap_or("").to_string(),
        size: value["Size"].as_u64().unwrap_or(0),
        free: value["Free"].as_u64().unwrap_or(0),
    })
}

fn run_winsat_read(drive: &str, random: bool) -> Option<f64> {
    let letter = drive
        .chars()
        .find(|c| c.is_ascii_alphabetic())?
        .to_ascii_uppercase()
        .to_string();
    let mode = if random { "-ran" } else { "-seq" };
    let output = run_capture("winsat.exe", &["disk", mode, "-read", "-drive", &letter]).ok()?;
    extract_mbps(&output)
}

fn run_capture(program: &str, args: &[&str]) -> Result<String, String> {
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
        text.push('\n');
        text.push_str(&stderr);
    }
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "{} terminou com código {:?}.\n{}",
            program,
            output.status.code(),
            text
        ))
    }
}

fn parse_fragmentation_percent(output: &str) -> Option<f32> {
    let mut fallback = None;
    for line in output.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("fragment") || !line.contains('%') {
            continue;
        }
        if let Some(value) = extract_percent(line) {
            fallback = Some(value);
            if lower.contains("total")
                || lower.contains("space")
                || lower.contains("espa")
                || lower.contains("fragmenta")
            {
                return Some(value);
            }
        }
    }
    fallback
}

fn parse_fragmented_files(output: &str) -> Option<u64> {
    for line in output.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("fragmented files")
            || lower.contains("arquivos fragment")
            || lower.contains("ficheiros fragment")
        {
            return extract_first_integer(line);
        }
    }
    None
}

fn extract_percent(line: &str) -> Option<f32> {
    let pos = line.find('%')?;
    let left = &line[..pos];
    let token = left
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .filter(|x| !x.is_empty())
        .last()?;
    token.replace(',', ".").parse::<f32>().ok()
}

fn extract_first_integer(line: &str) -> Option<u64> {
    line.split(|c: char| !c.is_ascii_digit())
        .find(|x| !x.is_empty())?
        .parse::<u64>()
        .ok()
}

fn extract_mbps(output: &str) -> Option<f64> {
    for line in output.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("mb/s") {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        for (index, token) in tokens.iter().enumerate() {
            if token.to_ascii_lowercase().contains("mb/s") && index > 0 {
                if let Ok(value) = tokens[index - 1].replace(',', ".").parse::<f64>() {
                    return Some(value);
                }
            }
        }
        for token in tokens.iter().rev() {
            if let Ok(value) = token.replace(',', ".").parse::<f64>() {
                return Some(value);
            }
        }
    }
    None
}

pub fn format_bytes(value: u64) -> String {
    const K: f64 = 1024.0;
    let value = value as f64;
    if value >= K * K * K * K {
        format!("{:.2} TB", value / (K * K * K * K))
    } else if value >= K * K * K {
        format!("{:.2} GB", value / (K * K * K))
    } else if value >= K * K {
        format!("{:.2} MB", value / (K * K))
    } else {
        format!("{:.2} KB", value / K)
    }
}

pub fn performance_delta(before: Option<f64>, after: Option<f64>) -> String {
    match (before, after) {
        (Some(a), Some(b)) if a > 0.0 => {
            let pct = ((b - a) / a) * 100.0;
            format!("{a:.1} → {b:.1} MB/s ({pct:+.1}%)")
        }
        (Some(a), Some(b)) => format!("{a:.1} → {b:.1} MB/s"),
        (Some(a), None) => format!("{a:.1} MB/s → indisponível"),
        _ => "indisponível".into(),
    }
}

pub fn sleep_repaint_hint() -> Duration {
    Duration::from_millis(150)
}
