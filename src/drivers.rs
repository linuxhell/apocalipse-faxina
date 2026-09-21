use serde::Deserialize;
use std::{
    cmp::Ordering,
    collections::HashMap,
    process::{Command, Stdio},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug, Deserialize)]
pub struct DriverEntry {
    #[serde(rename = "PublishedName")]
    pub published_name: String,
    #[serde(rename = "OriginalInf", default)]
    pub original_inf: String,
    #[serde(rename = "ClassName", default)]
    pub class_name: String,
    #[serde(rename = "ProviderName", default)]
    pub provider_name: String,
    #[serde(rename = "Version", default)]
    pub version: String,
    #[serde(rename = "Date", default)]
    pub date: String,
    #[serde(rename = "StoreDate", default)]
    pub store_date: String,
    #[serde(rename = "Size", default)]
    pub size: u64,
    #[serde(rename = "Devices", default)]
    pub devices: String,
    #[serde(rename = "Active", default)]
    pub active: bool,
    #[serde(rename = "Inbox", default)]
    pub inbox: bool,
    #[serde(rename = "BootCritical", default)]
    pub boot_critical: bool,
    #[serde(skip)]
    pub checked: bool,
    #[serde(skip)]
    pub old_candidate: bool,
    #[serde(skip)]
    pub error: Option<String>,
}

impl DriverEntry {
    pub fn category(&self) -> &'static str {
        match self.class_name.to_ascii_lowercase().as_str() {
            "net" | "network" => "Adaptadores de rede",
            "display" => "Adaptadores de vídeo",
            "media" => "Controladores de som, vídeo e jogos",
            "hdc" | "scsiadapter" | "storage" => "Controladores de armazenamento",
            "usb" => "Controladores USB",
            "printer" | "printqueue" => "Impressoras",
            "bluetooth" => "Bluetooth",
            "system" => "Dispositivos de sistema",
            "softwarecomponent" | "softwaredevice" => "Dispositivos de software",
            "extension" => "Extensões",
            "securitydevices" => "Dispositivos de segurança",
            "mouse" => "Mouse e dispositivos apontadores",
            "keyboard" => "Teclados",
            "image" | "camera" => "Câmeras e imagem",
            "ports" => "Portas COM/LPT",
            "monitor" => "Monitores",
            _ => "Outros drivers",
        }
    }

    pub fn state_label(&self) -> &'static str {
        if self.boot_critical {
            "Crítico de inicialização"
        } else if self.inbox {
            "Driver do Windows"
        } else if self.active {
            "Em uso"
        } else if self.old_candidate {
            "Versão antiga candidata"
        } else {
            "Armazenado"
        }
    }
}

#[derive(Default)]
pub struct DriverState {
    pub items: Vec<DriverEntry>,
    pub status: String,
    pub output: String,
}

impl DriverState {
    pub fn scan(&mut self) {
        const SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$deviceMap = @{}
Get-CimInstance Win32_PnPSignedDriver -ErrorAction SilentlyContinue | ForEach-Object {
  $inf = [string]$_.InfName
  if([string]::IsNullOrWhiteSpace($inf)){ return }
  $key = $inf.ToLowerInvariant()
  if(-not $deviceMap.ContainsKey($key)){ $deviceMap[$key] = @() }
  if(-not [string]::IsNullOrWhiteSpace([string]$_.DeviceName)){
    $deviceMap[$key] += [string]$_.DeviceName
  }
}

$items = @(
  Get-WindowsDriver -Online -All -ErrorAction SilentlyContinue |
    Where-Object { [string]$_.Driver -match '^oem\d+\.inf$' } |
    ForEach-Object {
      $inf = [string]$_.Driver
      $original = [string]$_.OriginalFileName
      $dir = if($original){ Split-Path -Parent $original } else { '' }
      $size = 0L
      $storeDate = ''
      if($dir -and (Test-Path -LiteralPath $dir)){
        try {
          $size = [int64]((Get-ChildItem -LiteralPath $dir -File -Recurse -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum)
          $storeDate = (Get-Item -LiteralPath $dir -ErrorAction SilentlyContinue).LastWriteTime.ToString('yyyy-MM-dd')
        } catch {}
      }
      $date = ''
      try { if($_.Date){ $date = ([datetime]$_.Date).ToString('yyyy-MM-dd') } } catch { $date = [string]$_.Date }
      $key = $inf.ToLowerInvariant()
      $devices = if($deviceMap.ContainsKey($key)){ @($deviceMap[$key] | Select-Object -Unique) -join ' | ' } else { '' }
      [PSCustomObject]@{
        PublishedName = $inf
        OriginalInf = if($original){ [IO.Path]::GetFileName($original) } else { '' }
        ClassName = [string]$_.ClassName
        ProviderName = [string]$_.ProviderName
        Version = [string]$_.Version
        Date = $date
        StoreDate = $storeDate
        Size = [int64]$size
        Devices = [string]$devices
        Active = [bool]$deviceMap.ContainsKey($key)
        Inbox = [bool]$_.Inbox
        BootCritical = [bool]$_.BootCritical
      }
    }
)
[Console]::Write((ConvertTo-Json -InputObject $items -Compress -Depth 5))
"#;

        match run_json::<DriverEntry>(SCRIPT) {
            Ok(mut items) => {
                classify_old_candidates(&mut items);
                items.sort_by(|a, b| {
                    a.category()
                        .cmp(b.category())
                        .then_with(|| a.provider_name.cmp(&b.provider_name))
                        .then_with(|| a.published_name.cmp(&b.published_name))
                });
                let old = items.iter().filter(|x| x.old_candidate).count();
                let active = items.iter().filter(|x| x.active).count();
                self.status = format!(
                    "{} pacotes no Driver Store • {} em uso • {} versão(ões) antiga(s) candidata(s)",
                    items.len(),
                    active,
                    old
                );
                self.items = items;
                self.output.clear();
            }
            Err(error) => {
                self.status = "Falha ao analisar Driver Store.".into();
                self.output = error;
            }
        }
    }

    pub fn select_old(&mut self) {
        for item in &mut self.items {
            item.checked = item.old_candidate;
        }
        let count = self.items.iter().filter(|x| x.checked).count();
        self.status = format!(
            "{} driver(s) antigo(s) marcados. Drivers em uso, Inbox e críticos não são selecionados automaticamente.",
            count
        );
    }

    pub fn clear_selection(&mut self) {
        for item in &mut self.items {
            item.checked = false;
        }
    }

    pub fn has_selected(&self) -> bool {
        self.items.iter().any(|x| x.checked)
    }

    pub fn has_failed_selected(&self) -> bool {
        self.items
            .iter()
            .any(|x| x.checked && x.error.is_some() && !x.active)
    }

    pub fn remove_selected(&mut self, force: bool) {
        let selected: Vec<DriverEntry> = self.items.iter().filter(|x| x.checked).cloned().collect();
        if selected.is_empty() {
            self.status = "Nenhum driver selecionado.".into();
            return;
        }

        let mut failures: HashMap<String, String> = HashMap::new();
        let mut logs = Vec::new();
        let mut removed = 0usize;

        for driver in selected {
            if driver.active {
                failures.insert(
                    driver.published_name.clone(),
                    "Driver atualmente usado por um dispositivo; remoção automática bloqueada.".into(),
                );
                continue;
            }
            if driver.inbox || driver.boot_critical {
                failures.insert(
                    driver.published_name.clone(),
                    "Driver do Windows/crítico protegido contra remoção.".into(),
                );
                continue;
            }
            if force && driver.error.is_none() {
                continue;
            }

            let mut args = vec!["/delete-driver", driver.published_name.as_str()];
            if force {
                args.push("/force");
            }
            let (ok, text) = run_command("pnputil.exe", &args);
            logs.push(format!("{}:\n{}", driver.published_name, text.trim()));
            if ok {
                removed += 1;
            } else {
                failures.insert(
                    driver.published_name.clone(),
                    if text.trim().is_empty() {
                        "pnputil não conseguiu remover o pacote.".into()
                    } else {
                        text.trim().to_string()
                    },
                );
            }
        }

        self.scan();
        for item in &mut self.items {
            if let Some(error) = failures.get(&item.published_name) {
                item.error = Some(error.clone());
                item.checked = true;
            }
        }
        self.output = logs.join("\n\n");
        self.status = format!(
            "{} driver(s) removidos • {} falha(s) permanecem marcadas{}",
            removed,
            failures.len(),
            if force { " após tentativa forçada" } else { "" }
        );
    }
}

fn classify_old_candidates(items: &mut [DriverEntry]) {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let key = format!(
            "{}|{}|{}",
            item.provider_name.to_ascii_lowercase(),
            item.class_name.to_ascii_lowercase(),
            item.original_inf.to_ascii_lowercase()
        );
        if !item.original_inf.is_empty() {
            groups.entry(key).or_default().push(index);
        }
    }

    for indices in groups.values() {
        if indices.len() < 2 {
            continue;
        }
        let mut newest = indices[0];
        for &index in indices.iter().skip(1) {
            if compare_driver(&items[index], &items[newest]) == Ordering::Greater {
                newest = index;
            }
        }
        for &index in indices {
            if index == newest {
                continue;
            }
            let item = &mut items[index];
            if !item.active && !item.inbox && !item.boot_critical {
                item.old_candidate = true;
            }
        }
    }
}

fn compare_driver(a: &DriverEntry, b: &DriverEntry) -> Ordering {
    let av = version_parts(&a.version);
    let bv = version_parts(&b.version);
    av.cmp(&bv).then_with(|| a.date.cmp(&b.date))
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
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
        .map_err(|error| format!("Falha ao interpretar Driver Store: {error}\n{stdout}"))
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
