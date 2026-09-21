use serde::Deserialize;
use std::{
    collections::HashSet,
    path::Path,
    process::{Command, Stdio},
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
}

impl WindowsInventory {
    pub fn scan_tasks(&mut self) {
        const SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$items = @(
  Get-ScheduledTask -ErrorAction SilentlyContinue |
    Where-Object { $_.TaskPath -notlike '\Microsoft\*' } |
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
      }
    }
)
[Console]::Write((ConvertTo-Json -InputObject $items -Compress -Depth 5))
"#;
        match run_json::<TaskEntry>(SCRIPT) {
            Ok(mut items) => {
                items.sort_by(|a, b| {
                    a.task_path
                        .cmp(&b.task_path)
                        .then_with(|| a.task_name.cmp(&b.task_name))
                });
                self.task_status = format!(
                    "{} tarefas não-Microsoft encontradas. Tarefas nativas do Windows ficam ocultas.",
                    items.len()
                );
                self.tasks = items;
            }
            Err(error) => self.task_status = error,
        }
    }

    pub fn task_action_command(&self, enable: bool) -> Option<String> {
        let selected: Vec<&TaskEntry> = self.tasks.iter().filter(|x| x.checked).collect();
        if selected.is_empty() {
            return None;
        }
        let verb = if enable {
            "Enable-ScheduledTask"
        } else {
            "Disable-ScheduledTask"
        };
        let statements = selected
            .into_iter()
            .map(|task| {
                format!(
                    "{} -TaskPath '{}' -TaskName '{}' -ErrorAction Continue",
                    verb,
                    ps_quote(&task.task_path),
                    ps_quote(&task.task_name)
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        Some(format!(
            "powershell -NoProfile -Command \"{}\"",
            statements.replace('"', "\\\"")
        ))
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
        const SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

function Get-Target([string]$Text) {
  if ([string]::IsNullOrWhiteSpace($Text)) { return $null }
  $e = [Environment]::ExpandEnvironmentVariables($Text)
  if ($e -match '^\s*"([^"]+\.(exe|com|cmd|bat))"') { return $matches[1] }
  if ($e -match '^\s*([^\s]+\.(exe|com|cmd|bat))') { return $matches[1] }
  return $null
}

function Target-Exists([string]$Target) {
  if ([string]::IsNullOrWhiteSpace($Target)) { return $true }
  if ([IO.Path]::IsPathRooted($Target)) { return Test-Path -LiteralPath $Target }
  return $null -ne (Get-Command $Target -ErrorAction SilentlyContinue)
}

$items = @()
$runRoots = @(
  @{ PS='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'; Reg='HKCU\Software\Microsoft\Windows\CurrentVersion\Run' },
  @{ PS='HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce'; Reg='HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce' },
  @{ PS='HKLM:\Software\Microsoft\Windows\CurrentVersion\Run'; Reg='HKLM\Software\Microsoft\Windows\CurrentVersion\Run' },
  @{ PS='HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run'; Reg='HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run' }
)
foreach ($root in $runRoots) {
  $key = Get-Item -LiteralPath $root.PS -ErrorAction SilentlyContinue
  if (-not $key) { continue }
  foreach ($name in $key.GetValueNames()) {
    $data = [string]$key.GetValue($name)
    $target = Get-Target $data
    if ($target -and -not (Target-Exists $target)) {
      $items += [PSCustomObject]@{
        Kind='Value'
        RegPath=$root.Reg
        ValueName=[string]$name
        Target=[string]$target
        Reason='Inicialização aponta para arquivo inexistente'
      }
    }
  }
}

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
      $target = [Environment]::ExpandEnvironmentVariables([string]$obj.GetValue(''))
      if ($target -and -not (Test-Path -LiteralPath $target)) {
        $items += [PSCustomObject]@{
          Kind='Key'
          RegPath=($root.Reg + '\' + $key.PSChildName)
          ValueName=''
          Target=[string]$target
          Reason='App Paths aponta para executável inexistente'
        }
      }
    } catch {}
  }
}

[Console]::Write((ConvertTo-Json -InputObject @($items) -Compress -Depth 5))
"#;
        match run_json::<RegistryOrphan>(SCRIPT) {
            Ok(mut items) => {
                items.sort_by(|a, b| a.reg_path.cmp(&b.reg_path));
                self.registry_status = format!(
                    "{} referências órfãs confirmadas em áreas conservadoras do Registro.",
                    items.len()
                );
                self.registry_orphans = items;
            }
            Err(error) => self.registry_status = error,
        }
    }

    pub fn registry_cleanup_command(&self, backup_dir: &Path) -> Option<(String, String)> {
        let selected: Vec<&RegistryOrphan> =
            self.registry_orphans.iter().filter(|x| x.checked).collect();
        if selected.is_empty() {
            return None;
        }

        let mut unique_keys = HashSet::new();
        let mut exports = Vec::new();
        let mut deletes = Vec::new();
        let mut manifest = String::from("BACKUP DE REGISTRO — APOCALIPSE FAXINA\n\n");

        for (index, item) in selected.iter().enumerate() {
            manifest.push_str(&format!(
                "{}\nTipo: {}\nChave: {}\nValor: {}\nAlvo: {}\nMotivo: {}\n\n",
                index + 1,
                item.kind,
                item.reg_path,
                item.value_name,
                item.target,
                item.reason
            ));

            if unique_keys.insert(item.reg_path.clone()) {
                let file = backup_dir.join(format!("chave-{:03}.reg", exports.len() + 1));
                exports.push(format!(
                    r#"reg export "{}" "{}" /y"#,
                    item.reg_path,
                    file.display()
                ));
            }

            if item.kind == "Key" {
                deletes.push(format!(r#"reg delete "{}" /f"#, item.reg_path));
            } else {
                deletes.push(format!(
                    r#"reg delete "{}" /v "{}" /f"#,
                    item.reg_path, item.value_name
                ));
            }
        }

        let command = format!("{} && {}", exports.join(" && "), deletes.join(" & "));
        Some((command, manifest))
    }
}

fn run_json<T>(script: &str) -> Result<Vec<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|error| format!("Falha ao executar PowerShell: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim_start_matches('\u{feff}')
        .trim()
        .to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(format!(
            "PowerShell terminou com erro: {}",
            if stderr.is_empty() { stdout } else { stderr }
        ));
    }
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&stdout)
        .map_err(|error| format!("Falha ao interpretar inventário do Windows: {error}\n{stdout}"))
}

fn ps_quote(value: &str) -> String {
    value.replace('\'', "''")
}
