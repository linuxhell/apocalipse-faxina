use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    path::Path,
    process::{Command, Stdio},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug, Deserialize)]
pub struct ServiceEntry {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "DisplayName", default)]
    pub display_name: String,
    #[serde(rename = "State", default)]
    pub state: String,
    #[serde(rename = "StartMode", default)]
    pub start_mode: String,
    #[serde(rename = "PathName", default)]
    pub path_name: String,
    #[serde(rename = "StartName", default)]
    pub start_name: String,
    #[serde(rename = "Company", default)]
    pub company: String,
    #[serde(rename = "IsMicrosoft", default)]
    pub is_microsoft: bool,
    #[serde(rename = "IsPerUser", default)]
    pub is_per_user: bool,
    #[serde(rename = "IsCurrentUser", default)]
    pub is_current_user: bool,
    #[serde(skip)]
    pub checked: bool,
    #[serde(skip)]
    pub error: Option<String>,
}

#[derive(Default)]
pub struct ServiceState {
    pub items: Vec<ServiceEntry>,
    pub status: String,
    pub output: String,
    pub install_name: String,
    pub install_display: String,
    pub install_exe: String,
    pub install_args: String,
    pub install_start: usize,
    pub install_account: usize,
}

impl ServiceState {
    pub fn scan(&mut self) {
        let previous_errors: HashMap<String, String> = self
            .items
            .iter()
            .filter_map(|item| item.error.as_ref().map(|e| (item.name.clone(), e.clone())))
            .collect();

        const SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$currentUser = [Security.Principal.WindowsIdentity]::GetCurrent().Name

function Get-Exe([string]$PathName) {
  if([string]::IsNullOrWhiteSpace($PathName)){ return '' }
  $expanded = [Environment]::ExpandEnvironmentVariables($PathName)
  if($expanded -match '^\s*"([^"]+\.exe)"'){ return $matches[1] }
  if($expanded -match '^\s*([^\s]+\.exe)'){ return $matches[1] }
  return ''
}

$items = @(
  Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | ForEach-Object {
    $exe = Get-Exe ([string]$_.PathName)
    $company = ''
    if($exe -and (Test-Path -LiteralPath $exe)){
      try { $company = [string](Get-Item -LiteralPath $exe).VersionInfo.CompanyName } catch {}
    }
    $reg = Get-Item -LiteralPath ("HKLM:\SYSTEM\CurrentControlSet\Services\" + [string]$_.Name) -ErrorAction SilentlyContinue
    $perUser = $false
    if($reg){
      try { $perUser = $null -ne $reg.GetValue('UserServiceFlags', $null) } catch {}
    }
    if([string]$_.Name -match '_[0-9A-Fa-f]{5,}$'){ $perUser = $true }
    $isMicrosoft = ($company -match '(?i)Microsoft') -or ($exe -match '(?i)\\Windows\\(System32|SysWOW64)\\')
    $isCurrent = $perUser -and (
      ([string]$_.StartName -eq $currentUser) -or
      ([string]$_.StartName -match '(?i)LocalSystem|LocalService|NetworkService')
    )
    [PSCustomObject]@{
      Name = [string]$_.Name
      DisplayName = [string]$_.DisplayName
      State = [string]$_.State
      StartMode = [string]$_.StartMode
      PathName = [string]$_.PathName
      StartName = [string]$_.StartName
      Company = [string]$company
      IsMicrosoft = [bool]$isMicrosoft
      IsPerUser = [bool]$perUser
      IsCurrentUser = [bool]$isCurrent
    }
  }
)
[Console]::Write((ConvertTo-Json -InputObject $items -Compress -Depth 5))
"#;

        match run_json::<ServiceEntry>(SCRIPT) {
            Ok(mut items) => {
                items.sort_by(|a, b| {
                    a.is_microsoft
                        .cmp(&b.is_microsoft)
                        .then_with(|| a.display_name.cmp(&b.display_name))
                        .then_with(|| a.name.cmp(&b.name))
                });
                for item in &mut items {
                    if let Some(error) = previous_errors.get(&item.name) {
                        item.error = Some(error.clone());
                    }
                }
                let third_party = items.iter().filter(|x| !x.is_microsoft).count();
                let per_user = items.iter().filter(|x| x.is_per_user).count();
                self.status = format!(
                    "{} serviços • {} não-Microsoft • {} serviço(s) por usuário",
                    items.len(),
                    third_party,
                    per_user
                );
                self.items = items;
            }
            Err(error) => {
                self.status = "Falha ao enumerar serviços.".into();
                self.output = error;
            }
        }
    }

    pub fn clear_selection(&mut self) {
        for item in &mut self.items {
            item.checked = false;
        }
    }

    pub fn apply_selected(&mut self, action: &str) {
        let selected: Vec<String> = self
            .items
            .iter()
            .filter(|x| x.checked)
            .map(|x| x.name.clone())
            .collect();
        if selected.is_empty() {
            self.status = "Nenhum serviço selecionado.".into();
            return;
        }

        let mut failures = HashMap::new();
        let mut logs = Vec::new();
        let mut success = 0usize;

        for name in selected {
            let args: Vec<&str> = match action {
                "start" => vec!["start", name.as_str()],
                "stop" => vec!["stop", name.as_str()],
                "auto" => vec!["config", name.as_str(), "start=", "auto"],
                "manual" => vec!["config", name.as_str(), "start=", "demand"],
                "disabled" => vec!["config", name.as_str(), "start=", "disabled"],
                _ => continue,
            };
            let (ok, text) = run_command("sc.exe", &args);
            logs.push(format!("{}:\n{}", name, text.trim()));
            if ok {
                success += 1;
            } else {
                failures.insert(
                    name,
                    if text.trim().is_empty() {
                        "O Windows recusou a operação.".into()
                    } else {
                        text.trim().to_string()
                    },
                );
            }
        }

        self.scan();
        for item in &mut self.items {
            if let Some(error) = failures.get(&item.name) {
                item.error = Some(error.clone());
                item.checked = true;
            }
        }
        self.output = logs.join("\n\n");
        self.status = format!(
            "{} serviço(s) alterados • {} falha(s) permanecem marcadas",
            success,
            failures.len()
        );
    }

    pub fn install(&mut self) {
        let name = self.install_name.trim().to_string();
        let exe = self.install_exe.trim().to_string();
        if name.is_empty() || exe.is_empty() {
            self.status = "Informe o nome do serviço e escolha o executável.".into();
            return;
        }
        if !Path::new(&exe).is_file() {
            self.status = "O executável informado não existe.".into();
            return;
        }

        let display = if self.install_display.trim().is_empty() {
            name.clone()
        } else {
            self.install_display.trim().to_string()
        };
        let bin_path = if self.install_args.trim().is_empty() {
            format!("\"{}\"", exe)
        } else {
            format!("\"{}\" {}", exe, self.install_args.trim())
        };
        let start = match self.install_start {
            0 => "auto",
            1 => "demand",
            _ => "disabled",
        };
        let account = match self.install_account {
            1 => r#"NT AUTHORITY\LocalService"#,
            2 => r#"NT AUTHORITY\NetworkService"#,
            _ => "LocalSystem",
        };

        let args = [
            "create",
            name.as_str(),
            "binPath=",
            bin_path.as_str(),
            "DisplayName=",
            display.as_str(),
            "start=",
            start,
            "obj=",
            account,
        ];
        let (ok, text) = run_command("sc.exe", &args);
        self.output = text;
        if ok {
            self.status = format!("Serviço {name} instalado com sucesso.");
            self.scan();
        } else {
            self.status = format!("Falha ao instalar o serviço {name}.");
        }
    }

    pub fn remove_selected(&mut self, backup_dir: &Path) {
        let selected: Vec<ServiceEntry> = self.items.iter().filter(|x| x.checked).cloned().collect();
        if selected.is_empty() {
            self.status = "Nenhum serviço selecionado.".into();
            return;
        }
        if let Err(error) = fs::create_dir_all(backup_dir) {
            self.status = format!("Não foi possível criar o backup: {error}");
            return;
        }

        let mut failures = HashMap::new();
        let mut manifest = String::from("BACKUP DE SERVIÇOS — APOCALIPSE FAXINA\n\n");
        let mut logs = Vec::new();
        let mut removed = 0usize;

        for service in selected {
            manifest.push_str(&format!(
                "Nome: {}\nExibição: {}\nEstado: {}\nInicialização: {}\nConta: {}\nCaminho: {}\nFabricante: {}\n\n",
                service.name,
                service.display_name,
                service.state,
                service.start_mode,
                service.start_name,
                service.path_name,
                service.company
            ));

            if service.is_microsoft {
                failures.insert(
                    service.name.clone(),
                    "Serviço Microsoft/Windows protegido contra remoção pelo Faxina.".into(),
                );
                continue;
            }

            let safe_name = sanitize_filename(&service.name);
            let reg_file = backup_dir.join(format!("{safe_name}.reg"));
            let reg_key = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{}", service.name);
            let _ = run_command(
                "reg.exe",
                &["export", reg_key.as_str(), reg_file.to_string_lossy().as_ref(), "/y"],
            );
            let _ = run_command("sc.exe", &["stop", service.name.as_str()]);
            let (ok, text) = run_command("sc.exe", &["delete", service.name.as_str()]);
            logs.push(format!("{}:\n{}", service.name, text.trim()));
            if ok {
                removed += 1;
            } else {
                failures.insert(
                    service.name.clone(),
                    if text.trim().is_empty() {
                        "O Windows recusou a remoção do serviço.".into()
                    } else {
                        text.trim().to_string()
                    },
                );
            }
        }

        let _ = fs::write(backup_dir.join("manifesto.txt"), manifest);
        self.scan();
        for item in &mut self.items {
            if let Some(error) = failures.get(&item.name) {
                item.error = Some(error.clone());
                item.checked = true;
            }
        }
        self.output = format!(
            "{}\n\nBackup/manifesto: {}",
            logs.join("\n\n"),
            backup_dir.display()
        );
        self.status = format!(
            "{} serviço(s) removidos • {} falha(s)/proteção(ões) permanecem marcadas",
            removed,
            failures.len()
        );
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect()
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
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&stdout)
        .map_err(|error| format!("Falha ao interpretar serviços: {error}\n{stdout}"))
}

fn run_command(program: &str, args: &[&str]) -> (bool, String) {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    match command.output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&stderr);
            }
            (output.status.success(), text)
        }
        Err(error) => (false, error.to_string()),
    }
}
