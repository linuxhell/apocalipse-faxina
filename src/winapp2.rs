use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleKind {
    Safe,
    Game,
    BrowserCache,
    BrowserTelemetry,
    BrowserPersonal,
    Thumbnail,
    WindowsSensitive,
    Warning,
}

impl RuleKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Safe => "Seguro",
            Self::Game => "Jogo",
            Self::BrowserCache => "Navegador / cache",
            Self::BrowserTelemetry => "Navegador / telemetria",
            Self::BrowserPersonal => "Navegador / dados pessoais",
            Self::Thumbnail => "Miniaturas",
            Self::WindowsSensitive => "Windows sensível",
            Self::Warning => "Aviso / inseguro",
        }
    }

    pub fn allowed_in_safe_preset(self) -> bool {
        matches!(self, Self::Safe)
    }
}

#[derive(Clone, Debug)]
pub struct WinappRule {
    pub name: String,
    pub section: String,
    pub warning: Option<String>,
    pub detects: Vec<String>,
    pub file_keys: Vec<String>,
    pub reg_keys: Vec<String>,
    pub exclude_keys: Vec<String>,
    pub checked: bool,
    pub kind: RuleKind,
}

impl WinappRule {
    fn empty(name: String) -> Self {
        Self {
            name,
            section: String::new(),
            warning: None,
            detects: Vec::new(),
            file_keys: Vec::new(),
            reg_keys: Vec::new(),
            exclude_keys: Vec::new(),
            checked: false,
            kind: RuleKind::Safe,
        }
    }
}

pub struct Winapp2State {
    pub path: PathBuf,
    pub rules: Vec<WinappRule>,
    pub status: String,
    pub filter: String,
}

impl Winapp2State {
    pub fn load(portable_root: &Path, config_dir: &Path) -> Self {
        let default_path = portable_root.join("data").join("winapp2.ini");
        let saved_path = fs::read_to_string(config_dir.join("winapp2-path.txt"))
            .ok()
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| !p.as_os_str().is_empty());

        let mut state = Self {
            path: saved_path.unwrap_or(default_path),
            rules: Vec::new(),
            status: "Winapp2.ini ainda não carregado".into(),
            filter: String::new(),
        };
        state.reload(config_dir);
        state
    }

    pub fn load_path(&mut self, path: PathBuf, config_dir: &Path) {
        self.path = path;
        let _ = fs::write(
            config_dir.join("winapp2-path.txt"),
            self.path.to_string_lossy().as_bytes(),
        );
        self.reload(config_dir);
    }

    pub fn reload(&mut self, config_dir: &Path) {
        self.rules.clear();
        match fs::read_to_string(&self.path) {
            Ok(content) => {
                let selected = load_selection(config_dir);
                self.rules = parse_winapp2(&content, &selected);
                self.status = format!(
                    "{} regras carregadas de {} • {} selecionadas",
                    self.rules.len(),
                    self.path.display(),
                    self.selected_count()
                );
            }
            Err(error) => {
                self.status = format!(
                    "Não foi possível carregar {}: {}",
                    self.path.display(),
                    error
                );
            }
        }
    }

    pub fn mark_all(&mut self) {
        for rule in &mut self.rules {
            rule.checked = true;
        }
        self.refresh_status();
    }

    pub fn clear(&mut self) {
        for rule in &mut self.rules {
            rule.checked = false;
        }
        self.refresh_status();
    }

    pub fn apply_safe_preset(&mut self) {
        for rule in &mut self.rules {
            rule.checked = rule.kind.allowed_in_safe_preset();
        }
        self.status = format!(
            "Preset seguro aplicado: {} selecionadas; jogos, caches/telemetria de navegadores, miniaturas e regras sensíveis ficaram desmarcados.",
            self.selected_count()
        );
    }

    pub fn save_selection(&mut self, config_dir: &Path) {
        let mut names: Vec<&str> = self
            .rules
            .iter()
            .filter(|r| r.checked)
            .map(|r| r.name.as_str())
            .collect();
        names.sort_unstable();
        match fs::write(config_dir.join("winapp2-selection.txt"), names.join("\n")) {
            Ok(_) => {
                self.status = format!(
                    "Marcações salvas: {} de {} regras.",
                    self.selected_count(),
                    self.rules.len()
                );
            }
            Err(error) => {
                self.status = format!("Falha ao salvar marcações: {error}");
            }
        }
    }

    pub fn export_current(&mut self, destination: &Path) {
        match fs::copy(&self.path, destination) {
            Ok(_) => self.status = format!("Cópia do Winapp2.ini salva em {}", destination.display()),
            Err(error) => self.status = format!("Falha ao exportar Winapp2.ini: {error}"),
        }
    }

    pub fn selected_count(&self) -> usize {
        self.rules.iter().filter(|r| r.checked).count()
    }

    pub fn visible_count(&self) -> usize {
        let filter = self.filter.trim().to_ascii_lowercase();
        if filter.is_empty() {
            self.rules.len()
        } else {
            self.rules
                .iter()
                .filter(|r| {
                    r.name.to_ascii_lowercase().contains(&filter)
                        || r.section.to_ascii_lowercase().contains(&filter)
                        || r.kind.label().to_ascii_lowercase().contains(&filter)
                })
                .count()
        }
    }

    pub fn refresh_status(&mut self) {
        self.status = format!(
            "{} regras • {} selecionadas",
            self.rules.len(),
            self.selected_count()
        );
    }
}

fn load_selection(config_dir: &Path) -> HashSet<String> {
    fs::read_to_string(config_dir.join("winapp2-selection.txt"))
        .map(|content| {
            content
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_winapp2(content: &str, selected: &HashSet<String>) -> Vec<WinappRule> {
    let mut rules = Vec::new();
    let mut current: Option<WinappRule> = None;

    let flush = |current: &mut Option<WinappRule>, rules: &mut Vec<WinappRule>| {
        if let Some(mut rule) = current.take() {
            rule.kind = classify_rule(&rule);
            rule.checked = selected.contains(&rule.name);
            rules.push(rule);
        }
    };

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') && line.len() > 2 {
            flush(&mut current, &mut rules);
            current = Some(WinappRule::empty(line[1..line.len() - 1].trim().to_string()));
            continue;
        }

        let Some(rule) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_string();

        if key == "section" {
            rule.section = value;
        } else if key == "warning" {
            rule.warning = Some(value);
        } else if key.starts_with("detect") {
            rule.detects.push(value);
        } else if key.starts_with("filekey") {
            rule.file_keys.push(value);
        } else if key.starts_with("regkey") {
            rule.reg_keys.push(value);
        } else if key.starts_with("excludekey") {
            rule.exclude_keys.push(value);
        }
    }

    flush(&mut current, &mut rules);
    rules
}

fn classify_rule(rule: &WinappRule) -> RuleKind {
    let name = rule.name.to_ascii_lowercase();
    let section = rule.section.to_ascii_lowercase();

    if section.trim() == "games" {
        return RuleKind::Game;
    }

    let browser = section.contains("web browser")
        || section.contains("internet suite")
        || section.contains("email client")
        || name.contains(" browser ");

    if browser && (name.contains(" cache") || name.contains("caches")) {
        return RuleKind::BrowserCache;
    }

    if browser && name.contains("telemetry") {
        return RuleKind::BrowserTelemetry;
    }

    if name.contains("thumbnail") || name.contains(".thumbnails") {
        return RuleKind::Thumbnail;
    }

    if is_windows_sensitive(&name) {
        return RuleKind::WindowsSensitive;
    }

    if rule.warning.as_ref().is_some_and(|w| !w.trim().is_empty()) {
        return RuleKind::Warning;
    }

    if browser && is_browser_personal(&name) {
        return RuleKind::BrowserPersonal;
    }

    RuleKind::Safe
}

fn is_windows_sensitive(name: &str) -> bool {
    const EXACT: &[&str] = &[
        "windows recent documents *",
        "windows volume mixer *",
        "windows settings *",
        "windows search *",
        "windows installer *",
        "microsoft directx shader cache *",
    ];

    EXACT.iter().any(|x| *x == name.trim()) || name.trim().starts_with("windows shell")
}

fn is_browser_personal(name: &str) -> bool {
    const TOKENS: &[&str] = &[
        "password",
        "cookie",
        "history",
        "session",
        "autofill",
        "bookmark",
        "pinned tab",
        "sync data",
        "synced tab",
        "web storage",
        "download history",
        "drm data",
        "security & threat",
        "privacy sandbox",
        "progressive web app",
        "extension cookie",
        "saved username",
    ];

    TOKENS.iter().any(|token| name.contains(token))
}
