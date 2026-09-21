use serde::Deserialize;
use std::{
    collections::HashSet,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Instant,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug, Deserialize)]
pub struct TaskEntry {
    #[serde(rename = "TaskName")]
    pub task_name: String,
    #[serde(rename = "TaskPath")]
    pub task_path: String,
    #[serde(rename = "Author", default)]
    pub author: String,
    #[serde(rename = "State", default)]
    pub state: String,
    #[serde(rename = "Actions", default)]
    pub actions: String,
    #[serde(rename = "IsWindows", default)]
    pub is_windows: bool,
    #[serde(skip)]
    pub checked: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ShellEntry {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Context")]
    pub context: String,
    #[serde(rename = "Kind")]
    pub kind: String,
    #[serde(rename = "RegPath")]
    pub reg_path: String,
    #[serde(rename = "Clsid", default)]
    pub clsid: String,
    #[serde(rename = "Command", default)]
    pub command: String,
    #[serde(rename = "Company", default)]
    pub company: String,
    #[serde(rename = "Enabled", default)]
    pub enabled: bool,
    #[serde(rename = "IsWindows", default)]
    pub is_windows: bool,
    #[serde(skip)]
    pub checked: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RegistryOrphan {
    #[serde(rename = "Category", default)]
    pub category: String,
    #[serde(rename = "Kind")]
    pub kind: String,
    #[serde(rename = "RegPath")]
    pub reg_path: String,
    #[serde(rename = "ValueName", default)]
    pub value_name: String,
    #[serde(rename = "Target", default)]
    pub target: String,
    #[serde(rename = "Reason", default)]
    pub reason: String,
    #[serde(rename = "Evidence", default)]
    pub evidence: String,
    #[serde(skip)]
    pub checked: bool,
}

#[derive(Default)]
pub struct WindowsInventory {
    pub tasks: Vec<TaskEntry>,
    pub shell: Vec<ShellEntry>,
    pub registry_orphans: Vec<RegistryOrphan>,
    pub task_status: String,
    pub shell_status: String,
    pub registry_status: String,
    pub registry_scanning: bool,
    registry_scan_rx: Option<Receiver<Result<Vec<RegistryOrphan>, String>>>,
    registry_scan_started: Option<Instant>,
}

impl WindowsInventory {
    pub fn scan_tasks(&mut self) {
        const SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$items = @(
  Get-ScheduledTask -ErrorAction SilentlyContinue |
    ForEach-Object {
      $actions = @($_.Actions | ForEach-Object {
        $e = [string]$_.Execute
        $a = [string]$_.Arguments
        ($e + ' ' + $a).Trim()
      }) -join ' | '
      [PSCustomObject]@{
        TaskName = [string]$_.TaskName
        TaskPath = [string]$_.TaskPath
        Author   = [string]$_.Author
        State    = [string]$_.State
        Actions  = [string]$actions
        IsWindows = [bool]([string]$_.TaskPath -like '\Microsoft\*')
      }
    }
)
[Console]::Write((ConvertTo-Json -InputObject $items -Compress -Depth 5))
"#;
        match run_json::<TaskEntry>(SCRIPT) {
            Ok(mut items) => {
                items.sort_by(|a, b| {
                    a.is_windows
                        .cmp(&b.is_windows)
                        .then_with(|| a.task_path.cmp(&b.task_path))
                        .then_with(|| a.task_name.cmp(&b.task_name))
                });
                let windows = items.iter().filter(|x| x.is_windows).count();
                let programs = items.len().saturating_sub(windows);
                self.task_status = format!(
                    "{} usuário/programas • {} Windows",
                    programs, windows
                );
                self.tasks = items;
            }
            Err(error) => self.task_status = error,
        }
    }

    pub fn task_action_script(&self, enable: bool, windows_scope: bool) -> Option<String> {
        let selected: Vec<&TaskEntry> = self
            .tasks
            .iter()
            .filter(|x| x.checked && x.is_windows == windows_scope)
            .collect();
        if selected.is_empty() {
            return None;
        }
        let verb = if enable {
            "Enable-ScheduledTask"
        } else {
            "Disable-ScheduledTask"
        };
        Some(
            selected
                .into_iter()
                .map(|task| {
                    format!(
                        "{} -TaskPath '{}' -TaskName '{}' -ErrorAction Continue | Out-Null",
                        verb,
                        ps_quote(&task.task_path),
                        ps_quote(&task.task_name)
                    )
                })
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    pub fn scan_shell(&mut self) {
        const SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

function Get-Company([string]$Command) {
  if ([string]::IsNullOrWhiteSpace($Command)) { return '' }
  $expanded = [Environment]::ExpandEnvironmentVariables($Command)
  $exe = $null
  if ($expanded -match '^\s*"([^"]+\.exe)"') { $exe = $matches[1] }
  elseif ($expanded -match '^\s*([^\s]+\.exe)') { $exe = $matches[1] }
  if ($exe -and (Test-Path -LiteralPath $exe)) {
    try { return [string](Get-Item -LiteralPath $exe).VersionInfo.CompanyName } catch {}
  }
  return ''
}

function Is-WindowsItem([string]$Name, [string]$Command, [string]$Company, [string]$Server) {
  if ($Company -match 'Microsoft') { return $true }
  if ($Command -match '(?i)\\Windows\\|explorer\.exe|shell32\.dll|rundll32\.exe\s+shell32') { return $true }
  if ($Server -match '(?i)\\Windows\\|shell32\.dll') { return $true }
  if ($Name -match '^(?i)(open|opennewwindow|print|printto|runas|pintohome|share|properties)$') { return $true }
  return $false
}

$staticRoots = @(
  @{ Context='Arquivos'; PS='Registry::HKEY_CLASSES_ROOT\*\shell'; Reg='HKCR\*\shell' },
  @{ Context='Pastas'; PS='Registry::HKEY_CLASSES_ROOT\Directory\shell'; Reg='HKCR\Directory\shell' },
  @{ Context='Fundo da pasta'; PS='Registry::HKEY_CLASSES_ROOT\Directory\Background\shell'; Reg='HKCR\Directory\Background\shell' },
  @{ Context='Unidades'; PS='Registry::HKEY_CLASSES_ROOT\Drive\shell'; Reg='HKCR\Drive\shell' },
  @{ Context='Folder'; PS='Registry::HKEY_CLASSES_ROOT\Folder\shell'; Reg='HKCR\Folder\shell' },
  @{ Context='Desktop'; PS='Registry::HKEY_CLASSES_ROOT\DesktopBackground\Shell'; Reg='HKCR\DesktopBackground\Shell' }
)

$items = @()
foreach ($root in $staticRoots) {
  if (-not (Test-Path -LiteralPath $root.PS)) { continue }
  foreach ($key in Get-ChildItem -LiteralPath $root.PS -ErrorAction SilentlyContinue) {
    try {
      $keyObj = Get-Item -LiteralPath $key.PSPath -ErrorAction Stop
      $label = [string]$keyObj.GetValue('')
      if ([string]::IsNullOrWhiteSpace($label)) { $label = [string]$key.PSChildName }
      $cmdObj = Get-Item -LiteralPath ($key.PSPath + '\command') -ErrorAction SilentlyContinue
      $command = if ($cmdObj) { [string]$cmdObj.GetValue('') } else { '' }
      $company = Get-Company $command
      $legacy = $null -ne $keyObj.GetValue('LegacyDisable', $null)
      $isWindows = Is-WindowsItem $key.PSChildName $command $company ''
      $items += [PSCustomObject]@{
        Name = $label
        Context = $root.Context
        Kind = 'Static'
        RegPath = ($root.Reg + '\' + $key.PSChildName)
        Clsid = ''
        Command = $command
        Company = $company
        Enabled = (-not $legacy)
        IsWindows = [bool]$isWindows
      }
    } catch {}
  }
}

$blockedKey = Get-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked' -ErrorAction SilentlyContinue
$handlerRoots = @(
  @{ Context='Arquivos'; PS='Registry::HKEY_CLASSES_ROOT\*\shellex\ContextMenuHandlers'; Reg='HKCR\*\shellex\ContextMenuHandlers' },
  @{ Context='Todos os arquivos'; PS='Registry::HKEY_CLASSES_ROOT\AllFilesystemObjects\shellex\ContextMenuHandlers'; Reg='HKCR\AllFilesystemObjects\shellex\ContextMenuHandlers' },
  @{ Context='Pastas'; PS='Registry::HKEY_CLASSES_ROOT\Directory\shellex\ContextMenuHandlers'; Reg='HKCR\Directory\shellex\ContextMenuHandlers' },
  @{ Context='Fundo da pasta'; PS='Registry::HKEY_CLASSES_ROOT\Directory\Background\shellex\ContextMenuHandlers'; Reg='HKCR\Directory\Background\shellex\ContextMenuHandlers' },
  @{ Context='Unidades'; PS='Registry::HKEY_CLASSES_ROOT\Drive\shellex\ContextMenuHandlers'; Reg='HKCR\Drive\shellex\ContextMenuHandlers' },
  @{ Context='Folder'; PS='Registry::HKEY_CLASSES_ROOT\Folder\shellex\ContextMenuHandlers'; Reg='HKCR\Folder\shellex\ContextMenuHandlers' }
)

foreach ($root in $handlerRoots) {
  if (-not (Test-Path -LiteralPath $root.PS)) { continue }
  foreach ($key in Get-ChildItem -LiteralPath $root.PS -ErrorAction SilentlyContinue) {
    try {
      $keyObj = Get-Item -LiteralPath $key.PSPath -ErrorAction Stop
      $clsid = [string]$keyObj.GetValue('')
      if ([string]::IsNullOrWhiteSpace($clsid)) { continue }
      $serverObj = Get-Item -LiteralPath ("Registry::HKEY_CLASSES_ROOT\CLSID\$clsid\InprocServer32") -ErrorAction SilentlyContinue
      $server = if ($serverObj) { [string]$serverObj.GetValue('') } else { '' }
      $company = ''
      if ($server -and (Test-Path -LiteralPath $server)) {
        try { $company = [string](Get-Item -LiteralPath $server).VersionInfo.CompanyName } catch {}
      }
      $blocked = $false
      if ($blockedKey) { $blocked = $null -ne $blockedKey.GetValue($clsid, $null) }
      $isWindows = Is-WindowsItem $key.PSChildName '' $company $server
      $items += [PSCustomObject]@{
        Name = [string]$key.PSChildName
        Context = $root.Context
        Kind = 'Handler'
        RegPath = ($root.Reg + '\' + $key.PSChildName)
        Clsid = $clsid
        Command = $server
        Company = $company
        Enabled = (-not $blocked)
        IsWindows = [bool]$isWindows
      }
    } catch {}
  }
}

[Console]::Write((ConvertTo-Json -InputObject @($items) -Compress -Depth 5))
"#;
        match run_json::<ShellEntry>(SCRIPT) {
            Ok(mut items) => {
                items.sort_by(|a, b| {
                    a.context
                        .cmp(&b.context)
                        .then_with(|| a.name.cmp(&b.name))
                });
                let third_party = items.iter().filter(|x| !x.is_windows).count();
                self.shell_status = format!(
                    "{} itens detectados • {} classificados como não-Windows",
                    items.len(),
                    third_party
                );
                self.shell = items;
            }
            Err(error) => self.shell_status = error,
        }
    }

    pub fn shell_action_command(&self, enable: bool) -> Option<String> {
        let selected: Vec<&ShellEntry> = self.shell.iter().filter(|x| x.checked).collect();
        if selected.is_empty() {
            return None;
        }
        let mut commands = Vec::new();
        for item in selected {
            if item.kind == "Handler" && !item.clsid.trim().is_empty() {
                let blocked =
                    r#"HKCU\Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked"#;
                if enable {
                    commands.push(format!(
                        r#"reg delete "{}" /v "{}" /f"#,
                        blocked, item.clsid
                    ));
                } else {
                    commands.push(format!(
                        r#"reg add "{}" /v "{}" /t REG_SZ /d "Apocalipse Faxina" /f"#,
                        blocked, item.clsid
                    ));
                }
            } else if item.kind == "Static" {
                if enable {
                    commands.push(format!(
                        r#"reg delete "{}" /v LegacyDisable /f"#,
                        item.reg_path
                    ));
                } else {
                    commands.push(format!(
                        r#"reg add "{}" /v LegacyDisable /t REG_SZ /d "" /f"#,
                        item.reg_path
                    ));
                }
            }
        }
        (!commands.is_empty()).then(|| commands.join(" & "))
    }

    pub fn scan_registry_orphans(&mut self) {
        if self.registry_scanning {
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.registry_scan_rx = Some(rx);
        self.registry_scan_started = Some(Instant::now());
        self.registry_scanning = true;
        self.registry_status =
            "Varredura segura em andamento: verificando somente referências comprovadamente órfãs…"
                .into();
        self.registry_orphans.clear();

        crate::diagnostics::operation_start(
            "registro",
            "safe_scan",
            serde_json::json!({
                "mode": "safe",
                "policy": "somente referências com alvo comprovadamente inexistente"
            }),
        );

        thread::spawn(move || {
            let result = WindowsInventory::scan_registry_safe_impl();
            let _ = tx.send(result);
        });
    }

    pub fn poll_registry_scan(&mut self) {
        let result = self
            .registry_scan_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        let Some(result) = result else {
            return;
        };

        self.registry_scan_rx = None;
        self.registry_scanning = false;
        let elapsed = self
            .registry_scan_started
            .take()
            .map(|started| started.elapsed().as_millis())
            .unwrap_or(0);

        match result {
            Ok(mut items) => {
                for item in &mut items {
                    item.checked = true;
                    if item.category.trim().is_empty() {
                        item.category = "Outros seguros".into();
                    }
                }
                items.sort_by(|a, b| {
                    a.category
                        .cmp(&b.category)
                        .then_with(|| a.reg_path.cmp(&b.reg_path))
                        .then_with(|| a.value_name.cmp(&b.value_name))
                });

                let categories = items
                    .iter()
                    .map(|item| item.category.as_str())
                    .collect::<HashSet<_>>()
                    .len();

                self.registry_status = format!(
                    "{} entrada(s) segura(s) encontrada(s) em {} categoria(s). O modo Safe marca automaticamente todos os resultados.",
                    items.len(),
                    categories
                );

                crate::diagnostics::operation_end(
                    "registro",
                    "safe_scan",
                    true,
                    elapsed,
                    serde_json::json!({
                        "items": items.len(),
                        "categories": categories
                    }),
                );
                self.registry_orphans = items;
            }
            Err(error) => {
                self.registry_status = format!("Falha na varredura segura: {error}");
                crate::diagnostics::operation_end(
                    "registro",
                    "safe_scan",
                    false,
                    elapsed,
                    serde_json::json!({"error": error}),
                );
            }
        }
    }

    fn scan_registry_safe_impl() -> Result<Vec<RegistryOrphan>, String> {
        const SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$items = New-Object System.Collections.Generic.List[object]

function Add-SafeItem([string]$Category,[string]$Kind,[string]$RegPath,[string]$ValueName,[string]$Target,[string]$Reason,[string]$Evidence) {
    $items.Add([PSCustomObject]@{
        Category=$Category
        Kind=$Kind
        RegPath=$RegPath
        ValueName=$ValueName
        Target=$Target
        Reason=$Reason
        Evidence=$Evidence
    }) | Out-Null
}

function Expand-Text([string]$Text) {
    if ([string]::IsNullOrWhiteSpace($Text)) { return '' }
    return [Environment]::ExpandEnvironmentVariables($Text.Trim())
}

function Get-ExecutableTarget([string]$Text) {
    $e = Expand-Text $Text
    if ([string]::IsNullOrWhiteSpace($e)) { return $null }
    if ($e -match '^\s*"([^"]+\.(exe|com|cmd|bat))"') { return $matches[1] }
    if ($e -match '^\s*([^\s,]+\.(exe|com|cmd|bat))') { return $matches[1] }
    if ($e -match '(?i)^\s*rundll32(?:\.exe)?\s+"?([^",]+\.dll)') { return $matches[1] }
    return $null
}

function Get-PathTarget([string]$Text) {
    $e = Expand-Text $Text
    if ([string]::IsNullOrWhiteSpace($e)) { return $null }
    $e = $e.Trim('"')
    if ([IO.Path]::IsPathRooted($e)) { return $e }
    return $null
}

function Target-Exists([string]$Target) {
    if ([string]::IsNullOrWhiteSpace($Target)) { return $true }
    if ([IO.Path]::IsPathRooted($Target)) { return Test-Path -LiteralPath $Target }
    return $null -ne (Get-Command $Target -ErrorAction SilentlyContinue)
}

function Is-LocalMissingPath([string]$Target) {
    if ([string]::IsNullOrWhiteSpace($Target)) { return $false }
    $expanded = Expand-Text $Target
    if ($expanded.StartsWith('\\')) { return $false }
    if (-not [IO.Path]::IsPathRooted($expanded)) { return $false }
    return -not (Test-Path -LiteralPath $expanded)
}

function Is-WindowsPath([string]$Target) {
    if ([string]::IsNullOrWhiteSpace($Target)) { return $false }
    $expanded = (Expand-Text $Target).ToLowerInvariant()
    $win = ([string]$env:WINDIR).ToLowerInvariant()
    return $expanded.StartsWith($win + '\')
}

# Inicialização: Run / RunOnce
$runRoots = @(
  @{ PS='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'; Reg='HKCU\Software\Microsoft\Windows\CurrentVersion\Run' },
  @{ PS='HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce'; Reg='HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce' },
  @{ PS='HKLM:\Software\Microsoft\Windows\CurrentVersion\Run'; Reg='HKLM\Software\Microsoft\Windows\CurrentVersion\Run' },
  @{ PS='HKLM:\Software\Microsoft\Windows\CurrentVersion\RunOnce'; Reg='HKLM\Software\Microsoft\Windows\CurrentVersion\RunOnce' },
  @{ PS='HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run'; Reg='HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run' },
  @{ PS='HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce'; Reg='HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce' }
)
foreach ($root in $runRoots) {
    $key = Get-Item -LiteralPath $root.PS -ErrorAction SilentlyContinue
    if (-not $key) { continue }
    foreach ($name in $key.GetValueNames()) {
        $data = [string]$key.GetValue($name)
        $target = Get-ExecutableTarget $data
        if ($target -and -not (Target-Exists $target)) {
            Add-SafeItem 'Programas de inicialização' 'Value' $root.Reg ([string]$name) $target 'Inicialização aponta para arquivo inexistente' 'Run/RunOnce referencia um executável que não existe.'
        }
    }
}

# App Paths
$appRoots = @(
  @{ PS='HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths'; Reg='HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths' },
  @{ PS='HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths'; Reg='HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths' },
  @{ PS='HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths'; Reg='HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths' }
)
foreach ($root in $appRoots) {
    if (-not (Test-Path -LiteralPath $root.PS)) { continue }
    foreach ($key in Get-ChildItem -LiteralPath $root.PS -ErrorAction SilentlyContinue) {
        try {
            $obj = Get-Item -LiteralPath $key.PSPath -ErrorAction Stop
            $target = Expand-Text ([string]$obj.GetValue(''))
            if ($target -and (Is-LocalMissingPath $target)) {
                Add-SafeItem 'Atalhos de software / App Paths' 'Key' ($root.Reg + '\' + $key.PSChildName) '' $target 'App Paths aponta para executável inexistente' 'O caminho absoluto registrado pelo aplicativo não existe.'
            }
        } catch {}
    }
}

# RegisteredApplications / Capabilities
$registeredRoots = @(
  @{ PS='HKCU:\Software\RegisteredApplications'; Reg='HKCU\Software\RegisteredApplications'; Hive='Registry::HKEY_CURRENT_USER\' },
  @{ PS='HKLM:\Software\RegisteredApplications'; Reg='HKLM\Software\RegisteredApplications'; Hive='Registry::HKEY_LOCAL_MACHINE\' },
  @{ PS='HKLM:\Software\WOW6432Node\RegisteredApplications'; Reg='HKLM\Software\WOW6432Node\RegisteredApplications'; Hive='Registry::HKEY_LOCAL_MACHINE\' }
)
foreach ($root in $registeredRoots) {
    $key = Get-Item -LiteralPath $root.PS -ErrorAction SilentlyContinue
    if (-not $key) { continue }
    foreach ($name in $key.GetValueNames()) {
        $relative = [string]$key.GetValue($name)
        if ([string]::IsNullOrWhiteSpace($relative)) { continue }
        $providerPath = $root.Hive + $relative.TrimStart('\')
        if (-not (Test-Path -LiteralPath $providerPath)) {
            Add-SafeItem 'Definições de Aplicações' 'Value' $root.Reg ([string]$name) $relative 'RegisteredApplications aponta para Capabilities inexistente' 'O aplicativo está registrado, mas sua chave Capabilities não existe.'
        }
    }
}

# SharedDLLs
$sharedRoots = @(
  @{ PS='HKLM:\Software\Microsoft\Windows\CurrentVersion\SharedDLLs'; Reg='HKLM\Software\Microsoft\Windows\CurrentVersion\SharedDLLs' },
  @{ PS='HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\SharedDLLs'; Reg='HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\SharedDLLs' }
)
foreach ($root in $sharedRoots) {
    $key = Get-Item -LiteralPath $root.PS -ErrorAction SilentlyContinue
    if (-not $key) { continue }
    foreach ($name in $key.GetValueNames()) {
        $target = Expand-Text ([string]$name)
        if (Is-LocalMissingPath $target) {
            Add-SafeItem 'DLLs compartilhadas' 'Value' $root.Reg ([string]$name) $target 'SharedDLLs referencia arquivo inexistente' 'O valor usa o caminho da DLL como nome e esse arquivo não existe.'
        }
    }
}

# Fontes
$fontRoots = @(
  @{ PS='HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts'; Reg='HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts'; User=$false },
  @{ PS='HKCU:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts'; Reg='HKCU\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts'; User=$true }
)
foreach ($root in $fontRoots) {
    $key = Get-Item -LiteralPath $root.PS -ErrorAction SilentlyContinue
    if (-not $key) { continue }
    foreach ($name in $key.GetValueNames()) {
        $raw = Expand-Text ([string]$key.GetValue($name))
        if ([string]::IsNullOrWhiteSpace($raw)) { continue }
        if ([IO.Path]::IsPathRooted($raw)) {
            $target = $raw
        } elseif ($root.User) {
            $target = Join-Path $env:LOCALAPPDATA ('Microsoft\Windows\Fonts\' + $raw)
        } else {
            $target = Join-Path $env:WINDIR ('Fonts\' + $raw)
        }
        if (-not (Test-Path -LiteralPath $target)) {
            Add-SafeItem 'Fontes' 'Value' $root.Reg ([string]$name) $target 'Fonte registrada sem arquivo correspondente' 'O arquivo de fonte não existe no local esperado.'
        }
    }
}

# Applications\...\shell\open\command
$applicationRoots = @(
  @{ PS='HKCU:\Software\Classes\Applications'; Reg='HKCU\Software\Classes\Applications' },
  @{ PS='HKLM:\Software\Classes\Applications'; Reg='HKLM\Software\Classes\Applications' },
  @{ PS='HKLM:\Software\Classes\WOW6432Node\Applications'; Reg='HKLM\Software\Classes\WOW6432Node\Applications' }
)
foreach ($root in $applicationRoots) {
    if (-not (Test-Path -LiteralPath $root.PS)) { continue }
    foreach ($app in Get-ChildItem -LiteralPath $root.PS -ErrorAction SilentlyContinue) {
        $cmdKey = Get-Item -LiteralPath ($app.PSPath + '\shell\open\command') -ErrorAction SilentlyContinue
        if (-not $cmdKey) { continue }
        $command = [string]$cmdKey.GetValue('')
        $target = Get-ExecutableTarget $command
        if ($target -and (Is-LocalMissingPath $target) -and -not (Is-WindowsPath $target)) {
            Add-SafeItem 'Atalhos de aplicações' 'Key' ($root.Reg + '\' + $app.PSChildName) '' $target 'Aplicação registrada possui comando Open quebrado' 'O comando Open chama um executável local inexistente.'
        }
    }
}

# Associações de arquivo do usuário cujo ProgID sumiu
$userClasses = 'HKCU:\Software\Classes'
if (Test-Path -LiteralPath $userClasses) {
    foreach ($extension in Get-ChildItem -LiteralPath $userClasses -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -like '.*' }) {
        try {
            $obj = Get-Item -LiteralPath $extension.PSPath -ErrorAction Stop
            $progId = [string]$obj.GetValue('')
            if ([string]::IsNullOrWhiteSpace($progId)) { continue }
            $resolved = 'Registry::HKEY_CLASSES_ROOT\' + $progId
            if (-not (Test-Path -LiteralPath $resolved)) {
                Add-SafeItem 'Tipos de Arquivo' 'Value' ('HKCU\Software\Classes\' + $extension.PSChildName) '' $progId 'Associação de arquivo aponta para ProgID inexistente' 'A extensão do usuário referencia uma classe que não existe em HKCR.'
            }
        } catch {}
    }
}

# Programas desinstalados: duas provas, sem MSI/Microsoft
$uninstallRoots = @(
  @{ PS='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'; Reg='HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall' },
  @{ PS='HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall'; Reg='HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall' },
  @{ PS='HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'; Reg='HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall' }
)
foreach ($root in $uninstallRoots) {
    if (-not (Test-Path -LiteralPath $root.PS)) { continue }
    foreach ($sub in Get-ChildItem -LiteralPath $root.PS -ErrorAction SilentlyContinue) {
        try {
            $obj = Get-ItemProperty -LiteralPath $sub.PSPath -ErrorAction Stop
            $display = [string]$obj.DisplayName
            if ([string]::IsNullOrWhiteSpace($display)) { continue }
            if ([int]$obj.WindowsInstaller -eq 1 -or [int]$obj.SystemComponent -eq 1) { continue }
            if ([string]$obj.Publisher -match '(?i)Microsoft') { continue }

            $uninstallTarget = Get-ExecutableTarget ([string]$obj.UninstallString)
            $installLocation = Get-PathTarget ([string]$obj.InstallLocation)
            $displayIcon = Get-ExecutableTarget ([string]$obj.DisplayIcon)

            $uninstallMissing = $uninstallTarget -and (Is-LocalMissingPath $uninstallTarget)
            $locationMissing = $installLocation -and (Is-LocalMissingPath $installLocation)
            $iconMissing = $displayIcon -and (Is-LocalMissingPath $displayIcon)

            if ($uninstallMissing -and ($locationMissing -or $iconMissing)) {
                $evidence = if ($locationMissing) { 'UninstallString e InstallLocation não existem mais.' } else { 'UninstallString e DisplayIcon não existem mais.' }
                Add-SafeItem 'Programas desinstalados' 'Key' ($root.Reg + '\' + $sub.PSChildName) '' $uninstallTarget ('Resíduo de desinstalação confirmado: ' + $display) $evidence
            }
        } catch {}
    }
}

# StartupApproved órfão
$approvedPairs = @(
  @{ Approved='HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'; Reg='HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'; Run='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' },
  @{ Approved='HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32'; Reg='HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32'; Run='HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run' },
  @{ Approved='HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'; Reg='HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'; Run='HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' },
  @{ Approved='HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32'; Reg='HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32'; Run='HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run' }
)
foreach ($pair in $approvedPairs) {
    $approved = Get-Item -LiteralPath $pair.Approved -ErrorAction SilentlyContinue
    if (-not $approved) { continue }
    $run = Get-Item -LiteralPath $pair.Run -ErrorAction SilentlyContinue
    $runNames = if ($run) { @($run.GetValueNames()) } else { @() }
    foreach ($name in $approved.GetValueNames()) {
        if ($runNames -notcontains $name) {
            Add-SafeItem 'Programas de inicialização' 'Value' $pair.Reg ([string]$name) '' 'StartupApproved sem item Run correspondente' 'Sobrou apenas o estado habilitado/desabilitado; a entrada original não existe.'
        }
    }
}

# Cache MUI
$muiRoots = @(
  @{ PS='HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache'; Reg='HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache' },
  @{ PS='HKCU:\Software\Microsoft\Windows\ShellNoRoam\MUICache'; Reg='HKCU\Software\Microsoft\Windows\ShellNoRoam\MUICache' }
)
foreach ($root in $muiRoots) {
    $key = Get-Item -LiteralPath $root.PS -ErrorAction SilentlyContinue
    if (-not $key) { continue }
    foreach ($name in $key.GetValueNames()) {
        $candidate = [string]$name
        if ($candidate -match '^(.*\.(exe|dll|cpl|msc))(?:\.(FriendlyAppName|ApplicationCompany))?$') {
            $target = Expand-Text $matches[1]
            if ((Is-LocalMissingPath $target) -and -not (Is-WindowsPath $target)) {
                Add-SafeItem 'Cache MUI' 'Value' $root.Reg ([string]$name) $target 'Cache MUI aponta para binário inexistente' 'É somente cache de nome/empresa de um arquivo que não existe mais.'
            }
        }
    }
}

# Menu de contexto estático
$shellRoots = @(
  @{ PS='Registry::HKEY_CLASSES_ROOT\*\shell'; Reg='HKCR\*\shell' },
  @{ PS='Registry::HKEY_CLASSES_ROOT\Directory\shell'; Reg='HKCR\Directory\shell' },
  @{ PS='Registry::HKEY_CLASSES_ROOT\Directory\Background\shell'; Reg='HKCR\Directory\Background\shell' },
  @{ PS='Registry::HKEY_CLASSES_ROOT\Drive\shell'; Reg='HKCR\Drive\shell' },
  @{ PS='Registry::HKEY_CLASSES_ROOT\Folder\shell'; Reg='HKCR\Folder\shell' },
  @{ PS='Registry::HKEY_CLASSES_ROOT\DesktopBackground\Shell'; Reg='HKCR\DesktopBackground\Shell' }
)
foreach ($root in $shellRoots) {
    if (-not (Test-Path -LiteralPath $root.PS)) { continue }
    foreach ($verb in Get-ChildItem -LiteralPath $root.PS -ErrorAction SilentlyContinue) {
        $cmdKey = Get-Item -LiteralPath ($verb.PSPath + '\command') -ErrorAction SilentlyContinue
        if (-not $cmdKey) { continue }
        $command = [string]$cmdKey.GetValue('')
        $target = Get-ExecutableTarget $command
        if ($target -and (Is-LocalMissingPath $target) -and -not (Is-WindowsPath $target)) {
            Add-SafeItem 'Menu de contexto' 'Key' ($root.Reg + '\' + $verb.PSChildName) '' $target 'Menu de contexto aponta para executável inexistente' 'O verbo do shell chama um executável local que não existe.'
        }
    }
}

# ActiveX/COM: somente servidor local absoluto, fora do Windows, inexistente, sem TreatAs/LocalService
$clsidRoots = @(
  @{ PS='HKCU:\Software\Classes\CLSID'; Reg='HKCU\Software\Classes\CLSID' },
  @{ PS='HKLM:\Software\Classes\CLSID'; Reg='HKLM\Software\Classes\CLSID' },
  @{ PS='HKLM:\Software\Classes\WOW6432Node\CLSID'; Reg='HKLM\Software\Classes\WOW6432Node\CLSID' }
)
foreach ($root in $clsidRoots) {
    if (-not (Test-Path -LiteralPath $root.PS)) { continue }
    foreach ($clsid in Get-ChildItem -LiteralPath $root.PS -ErrorAction SilentlyContinue) {
        foreach ($serverName in @('InprocServer32','LocalServer32')) {
            $serverKey = Get-Item -LiteralPath ($clsid.PSPath + '\' + $serverName) -ErrorAction SilentlyContinue
            if (-not $serverKey) { continue }
            $raw = Expand-Text ([string]$serverKey.GetValue(''))
            if ([string]::IsNullOrWhiteSpace($raw)) { continue }

            if ($serverName -eq 'LocalServer32') {
                $target = Get-ExecutableTarget $raw
            } else {
                $candidate = $raw.Trim('"')
                $target = if ([IO.Path]::IsPathRooted($candidate)) { $candidate } else { $null }
            }

            if ($target -and (Is-LocalMissingPath $target) -and -not (Is-WindowsPath $target)) {
                $parentObj = Get-Item -LiteralPath $clsid.PSPath -ErrorAction SilentlyContinue
                $localService = if ($parentObj) { [string]$parentObj.GetValue('LocalService') } else { '' }
                $treatAs = Get-Item -LiteralPath ($clsid.PSPath + '\TreatAs') -ErrorAction SilentlyContinue
                if ([string]::IsNullOrWhiteSpace($localService) -and -not $treatAs) {
                    Add-SafeItem 'ActiveX/COM' 'Key' ($root.Reg + '\' + $clsid.PSChildName) '' $target ('Registro COM órfão: ' + $serverName) 'O servidor COM é caminho local absoluto, não é do Windows e o arquivo não existe.'
                    break
                }
            }
        }
    }
}

[Console]::Write((ConvertTo-Json -InputObject @($items) -Compress -Depth 6))
"#;

        run_json::<RegistryOrphan>(SCRIPT)
    }
