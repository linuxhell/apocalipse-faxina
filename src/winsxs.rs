use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Default)]
pub struct WinSxsState {
    pub explorer_size: String,
    pub actual_size: String,
    pub shared_size: String,
    pub backups_size: String,
    pub cache_size: String,
    pub reclaimable_packages: String,
    pub recommended: String,
    pub raw: String,
    pub status: String,
}

impl WinSxsState {
    pub fn analyze(&mut self) {
        self.clear_summary();
        let mut command = Command::new("dism.exe");
        command
            .args([
                "/Online",
                "/Cleanup-Image",
                "/AnalyzeComponentStore",
                "/English",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        match command.output() {
            Ok(output) => {
                let mut raw = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    raw.push('\n');
                    raw.push_str(&stderr);
                }
                self.raw = raw;
                self.parse();
                self.status = if output.status.success() {
                    "Análise oficial do Component Store concluída.".into()
                } else {
                    format!("DISM terminou com código {:?}.", output.status.code())
                };
            }
            Err(error) => {
                self.raw = format!("Falha ao executar DISM: {error}");
                self.status = "Não foi possível analisar o Component Store.".into();
            }
        }
    }

    fn clear_summary(&mut self) {
        self.explorer_size.clear();
        self.actual_size.clear();
        self.shared_size.clear();
        self.backups_size.clear();
        self.cache_size.clear();
        self.reclaimable_packages.clear();
        self.recommended.clear();
        self.raw.clear();
    }

    fn parse(&mut self) {
        for raw_line in self.raw.lines() {
            let line = raw_line.trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if key.contains("windows explorer reported size of component store") {
                self.explorer_size = value;
            } else if key.contains("actual size of component store") {
                self.actual_size = value;
            } else if key.contains("shared with windows") {
                self.shared_size = value;
            } else if key.contains("backups and disabled features") {
                self.backups_size = value;
            } else if key.contains("cache and temporary data") {
                self.cache_size = value;
            } else if key.contains("number of reclaimable packages") {
                self.reclaimable_packages = value;
            } else if key.contains("component store cleanup recommended") {
                self.recommended = value;
            }
        }
    }

    pub fn has_summary(&self) -> bool {
        !self.explorer_size.is_empty()
            || !self.actual_size.is_empty()
            || !self.reclaimable_packages.is_empty()
    }
}
