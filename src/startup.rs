use serde::Deserialize;
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug, Deserialize)]
pub struct StartupEntry {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Command", default)]
    pub command: String,
    #[serde(rename = "Location", default)]
    pub location: String,
    #[serde(rename = "Company", default)]
    pub company: String,
    #[serde(rename = "ApprovalPath")]
    pub approval_path: String,
    #[serde(rename = "ApprovalName")]
    pub approval_name: String,
    #[serde(rename = "Enabled", default)]
    pub enabled: bool,
    #[serde(skip)]
    pub checked: bool,
    #[serde(skip)]
    pub optimization_reason: Option<String>,
}

#[derive(Default)]
pub struct StartupState {
    pub entries: Vec<StartupEntry>,
    pub status: String,
}

impl StartupState {
    pub fn scan(&mut self) {
        const SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

function Get-ApprovalState([string]$ApprovalPsPath, [string]$Name) {
  $key = Get-Item -LiteralPath $ApprovalPsPath -ErrorAction SilentlyContinue
  if (-not $key) { return $true }
  $bytes = $key.GetValue($Name, $null)
  if ($null -eq $bytes -or $bytes.Length -lt 1) { return $true }
  return ([int]$bytes[0] -ne 3)
}

function Get-CompanyFromCommand([string]$Command) {
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

$items = @()

$runRoots = @(
  @{
    SourcePs='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    Location='Registro HKCU\Run'
    ApprovalPs='HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
    ApprovalReg='HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
  },
  @{
    SourcePs='HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce'
    Location='Registro HKCU\RunOnce'
    ApprovalPs='HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
    ApprovalReg='HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
  },
  @{
    SourcePs='HKLM:\Software\Microsoft\Windows\CurrentVersion\Run'
    Location='Registro HKLM\Run'
    ApprovalPs='HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
    ApprovalReg='HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
  },
  @{
    SourcePs='HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run'
    Location='Registro HKLM 32-bit\Run'
    ApprovalPs='HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32'
    ApprovalReg='HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32'
  }
)

foreach ($root in $runRoots) {
  $key = Get-Item -LiteralPath $root.SourcePs -ErrorAction SilentlyContinue
  if (-not $key) { continue }
  foreach ($name in $key.GetValueNames()) {
    if ([string]::IsNullOrWhiteSpace([string]$name)) { continue }
    $command = [string]$key.GetValue($name)
    $items += [PSCustomObject]@{
      Name = [string]$name
      Command = $command
      Location = [string]$root.Location
      Company = Get-CompanyFromCommand $command
      ApprovalPath = [string]$root.ApprovalReg
      ApprovalName = [string]$name
      Enabled = [bool](Get-ApprovalState $root.ApprovalPs $name)
    }
  }
}

$shell = New-Object -ComObject WScript.Shell
$folderRoots = @(
  @{
    Folder=[Environment]::GetFolderPath('Startup')
    Location='Pasta Inicializar do usuário'
    ApprovalPs='HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder'
    ApprovalReg='HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder'
  },
  @{
    Folder=[Environment]::GetFolderPath('CommonStartup')
    Location='Pasta Inicializar de todos os usuários'
    ApprovalPs='HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder'
    ApprovalReg='HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder'
  }
)

foreach ($root in $folderRoots) {
  if ([string]::IsNullOrWhiteSpace($root.Folder) -or -not (Test-Path -LiteralPath $root.Folder)) { continue }
  foreach ($file in Get-ChildItem -LiteralPath $root.Folder -File -ErrorAction SilentlyContinue) {
    $command = [string]$file.FullName
    if ($file.Extension -ieq '.lnk') {
      try {
        $shortcut = $shell.CreateShortcut($file.FullName)
        $target = [string]$shortcut.TargetPath
        $args = [string]$shortcut.Arguments
        if (-not [string]::IsNullOrWhiteSpace($target)) {
          $command = ('"' + $target + '" ' + $args).Trim()
        }
      } catch {}
    }
    $items += [PSCustomObject]@{
      Name = [string]$file.Name
      Command = $command
      Location = [string]$root.Location
      Company = Get-CompanyFromCommand $command
      ApprovalPath = [string]$root.ApprovalReg
      ApprovalName = [string]$file.Name
      Enabled = [bool](Get-ApprovalState $root.ApprovalPs $file.Name)
    }
  }
}

[Console]::Write((ConvertTo-Json -InputObject @($items) -Compress -Depth 5))
"#;

        match run_json::<StartupEntry>(SCRIPT) {
            Ok(mut entries) => {
                entries.sort_by(|a, b| {
                    a.location
                        .cmp(&b.location)
                        .then_with(|| a.name.cmp(&b.name))
                });
                let active = entries.iter().filter(|x| x.enabled).count();
                self.status = format!(
                    "{} itens de inicialização detectados • {} ativos • {} desativados",
                    entries.len(),
                    active,
                    entries.len().saturating_sub(active)
                );
                self.entries = entries;
            }
            Err(error) => self.status = error,
        }
    }

    pub fn mark_known_for_optimization(&mut self) -> usize {
        let mut count=0usize;
        for entry in &mut self.entries {
            entry.checked=false;
            entry.optimization_reason=crate::optimizer_db::startup_reason(&entry.name,&entry.command,&entry.company).map(str::to_string);
            if entry.enabled && entry.optimization_reason.is_some(){entry.checked=true;count+=1;}
        }
        self.status=format!("{} programa(s) conhecidos marcados como opcionais no logon. Revise antes de desativar.",count);
        crate::diagnostics::event("optimization_selection","Programas conhecidos marcados",serde_json::json!({"count":count,"db_version":crate::optimizer_db::DB_VERSION}));
        count
    }

    pub fn set_all_checked(&mut self, checked: bool) {
        for entry in &mut self.entries {
            entry.checked = checked;
        }
    }

    pub fn selected_action_command(&self, enable: bool) -> Option<String> {
        let selected: Vec<&StartupEntry> = self.entries.iter().filter(|x| x.checked).collect();
        if selected.is_empty() {
            return None;
        }

        // StartupApproved é o mesmo mecanismo consultado pelo Windows para manter
        // o item cadastrado, mas marcar seu estado como habilitado/desabilitado.
        let bytes = if enable {
            "020000000000000000000000"
        } else {
            "030000000000000000000000"
        };

        Some(
            selected
                .into_iter()
                .map(|entry| {
                    format!(
                        r#"reg add "{}" /v "{}" /t REG_BINARY /d {} /f"#,
                        entry.approval_path,
                        reg_quote(&entry.approval_name),
                        bytes
                    )
                })
                .collect::<Vec<_>>()
                .join(" & "),
        )
    }

    pub fn apply_local_state(&mut self, enable: bool) {
        for entry in self.entries.iter_mut().filter(|x| x.checked) {
            entry.enabled = enable;
        }
        let active = self.entries.iter().filter(|x| x.enabled).count();
        self.status = format!(
            "{} itens • {} ativos • {} desativados. Atualize a lista para confirmar o estado no Windows.",
            self.entries.len(),
            active,
            self.entries.len().saturating_sub(active)
        );
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
        .map_err(|error| format!("Falha ao consultar inicialização: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim_start_matches('\u{feff}')
        .trim()
        .to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(format!(
            "Falha ao consultar inicialização: {}",
            if stderr.is_empty() { stdout } else { stderr }
        ));
    }
    if stdout.is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(&stdout)
        .map_err(|error| format!("Falha ao interpretar inicialização: {error}\n{stdout}"))
}

fn reg_quote(value: &str) -> String {
    value.replace('"', "\\\"")
}
