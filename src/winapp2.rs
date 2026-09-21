use glob::{glob_with, MatchOptions, Pattern};
use std::{
    collections::HashSet,
    env,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

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

#[derive(Clone, Debug)]
pub struct WinappAnalysis {
    pub rule_name: String,
    pub file_count: usize,
    pub size: u64,
    pub files: Vec<PathBuf>,
    pub checked: bool,
}

pub struct Winapp2State {
    pub path: PathBuf,
    pub rules: Vec<WinappRule>,
    pub status: String,
    pub filter: String,
    pub analysis: Vec<WinappAnalysis>,
    pub analysis_status: String,
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
            analysis: Vec::new(),
            analysis_status: String::new(),
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
        self.analysis.clear();
        self.analysis_status.clear();
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


    pub fn analyze_selected(&mut self, global_exclusions: &[String]) {
        self.analysis.clear();
        let mut total_files = 0usize;
        let mut total_size = 0u64;
        let mut skipped_registry = 0usize;
        let mut truncated = false;
        let mut globally_seen = HashSet::new();

        for rule in self.rules.iter().filter(|rule| rule.checked) {
            skipped_registry = skipped_registry.saturating_add(rule.reg_keys.len());
            let mut files = Vec::new();
            let mut size = 0u64;

            for file_key in &rule.file_keys {
                if total_files >= 250_000 {
                    truncated = true;
                    break;
                }
                for path in resolve_file_key(file_key, rule, global_exclusions) {
                    if total_files >= 250_000 {
                        truncated = true;
                        break;
                    }
                    let normalized = normalize_path(&path.to_string_lossy());
                    if !globally_seen.insert(normalized) {
                        continue;
                    }
                    let Ok(meta) = fs::symlink_metadata(&path) else {
                        continue;
                    };
                    if !meta.is_file() || meta.file_type().is_symlink() {
                        continue;
                    }
                    size = size.saturating_add(meta.len());
                    total_size = total_size.saturating_add(meta.len());
                    total_files += 1;
                    files.push(path);
                }
            }

            if !files.is_empty() {
                self.analysis.push(WinappAnalysis {
                    rule_name: rule.name.clone(),
                    file_count: files.len(),
                    size,
                    files,
                    checked: true,
                });
            }

            if truncated {
                break;
            }
        }

        self.analysis.sort_by(|a, b| {
            b.size
                .cmp(&a.size)
                .then_with(|| a.rule_name.cmp(&b.rule_name))
        });

        self.analysis_status = format!(
            "{} regras com arquivos • {} arquivos • {}{} • {} ações de Registro ignoradas pelo modo seguro",
            self.analysis.len(),
            total_files,
            fmt_bytes(total_size),
            if truncated { " (limite de análise atingido)" } else { "" },
            skipped_registry
        );
    }

    pub fn clean_analyzed(&mut self, global_exclusions: &[String]) -> u64 {
        let mut removed = 0u64;
        let selected_rules: HashSet<String> = self
            .analysis
            .iter()
            .filter(|group| group.checked)
            .map(|group| group.rule_name.clone())
            .collect();

        let rules_by_name: std::collections::HashMap<&str, &WinappRule> = self
            .rules
            .iter()
            .map(|rule| (rule.name.as_str(), rule))
            .collect();

        for group in &mut self.analysis {
            if !selected_rules.contains(&group.rule_name) {
                continue;
            }
            let Some(rule) = rules_by_name.get(group.rule_name.as_str()).copied() else {
                continue;
            };
            for path in &group.files {
                if is_excluded(path, global_exclusions) || is_rule_excluded(path, rule) {
                    continue;
                }
                let before = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                if fs::remove_file(path).is_ok() {
                    removed = removed.saturating_add(before);
                }
            }
            group.files.retain(|path| path.exists());
            group.file_count = group.files.len();
            group.size = group
                .files
                .iter()
                .filter_map(|path| fs::metadata(path).ok().map(|m| m.len()))
                .sum();
        }

        self.analysis.retain(|group| !group.files.is_empty());
        self.analysis_status = format!(
            "Limpeza Winapp2 concluída • {} liberados • {} grupos ainda possuem arquivos. RegKey não foi executado.",
            fmt_bytes(removed),
            self.analysis.len()
        );
        removed
    }

    pub fn analyzed_size(&self) -> u64 {
        self.analysis
            .iter()
            .filter(|group| group.checked)
            .map(|group| group.size)
            .sum()
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


fn resolve_file_key(
    file_key: &str,
    rule: &WinappRule,
    global_exclusions: &[String],
) -> Vec<PathBuf> {
    let parts: Vec<&str> = file_key.split('|').map(str::trim).collect();
    if parts.len() < 2 {
        return Vec::new();
    }

    let base_pattern = expand_path(parts[0]);
    if base_pattern.trim().is_empty() {
        return Vec::new();
    }
    let file_patterns: Vec<Pattern> = parts[1]
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter_map(|p| Pattern::new(p).ok())
        .collect();
    if file_patterns.is_empty() {
        return Vec::new();
    }

    let flags = parts.iter().skip(2).map(|x| x.to_ascii_uppercase()).collect::<Vec<_>>();
    let recurse = flags.iter().any(|x| x == "RECURSE" || x == "REMOVESELF");
    let match_options = MatchOptions {
        case_sensitive: false,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };

    let glob_pattern = base_pattern.replace('\\', "/");
    let mut bases = Vec::new();
    if glob_pattern.contains('*') || glob_pattern.contains('?') || glob_pattern.contains('[') {
        if let Ok(paths) = glob_with(&glob_pattern, match_options) {
            for path in paths.flatten() {
                bases.push(path);
            }
        }
    } else {
        bases.push(PathBuf::from(base_pattern));
    }

    let mut result = Vec::new();
    let mut seen = HashSet::new();

    for base in bases {
        if is_excluded(&base, global_exclusions) {
            continue;
        }

        let Ok(meta) = fs::symlink_metadata(&base) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }

        if meta.is_file() {
            let name = base
                .file_name()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_default();
            if patterns_match(&file_patterns, &name, match_options)
                && !is_rule_excluded(&base, rule)
            {
                let key = normalize_path(&base.to_string_lossy());
                if seen.insert(key) {
                    result.push(base);
                }
            }
            continue;
        }

        if recurse {
            for entry in WalkDir::new(&base).follow_links(false).into_iter().flatten() {
                if entry.depth() == 0 || !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if is_excluded(path, global_exclusions) || is_rule_excluded(path, rule) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy();
                if patterns_match(&file_patterns, &name, match_options) {
                    let key = normalize_path(&path.to_string_lossy());
                    if seen.insert(key) {
                        result.push(path.to_path_buf());
                    }
                }
            }
        } else if let Ok(entries) = fs::read_dir(&base) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if !meta.is_file() || is_excluded(&path, global_exclusions) || is_rule_excluded(&path, rule) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if patterns_match(&file_patterns, &name, match_options) {
                    let key = normalize_path(&path.to_string_lossy());
                    if seen.insert(key) {
                        result.push(path);
                    }
                }
            }
        }
    }

    result
}

fn patterns_match(patterns: &[Pattern], name: &str, options: MatchOptions) -> bool {
    patterns
        .iter()
        .any(|pattern| pattern.matches_with(name, options))
}

fn is_rule_excluded(path: &Path, rule: &WinappRule) -> bool {
    let normalized = normalize_path(&path.to_string_lossy());
    let file_name = path
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_default();
    let options = MatchOptions {
        case_sensitive: false,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };

    for exclude in &rule.exclude_keys {
        let parts: Vec<&str> = exclude.split('|').map(str::trim).collect();
        if parts.len() < 2 {
            continue;
        }
        let kind = parts[0].to_ascii_uppercase();
        if kind != "FILE" && kind != "PATH" {
            continue;
        }
        let base = normalize_path(&expand_path(parts[1]));
        if base.is_empty() {
            continue;
        }
        let below = normalized == base
            || normalized
                .strip_prefix(&base)
                .is_some_and(|rest| rest.starts_with('\\'));
        if !below {
            continue;
        }
        if kind == "PATH" || parts.len() < 3 {
            return true;
        }
        for pattern in parts[2].split(';').map(str::trim).filter(|p| !p.is_empty()) {
            if Pattern::new(pattern)
                .ok()
                .is_some_and(|p| p.matches_with(&file_name, options))
            {
                return true;
            }
        }
    }
    false
}

fn expand_path(input: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '%' {
            if let Some(end) = chars[index + 1..].iter().position(|c| *c == '%') {
                let end = index + 1 + end;
                let key: String = chars[index + 1..end].iter().collect();
                let value = if key.eq_ignore_ascii_case("LocalLowAppData") {
                    env::var("USERPROFILE")
                        .ok()
                        .map(|p| format!(r"{}\AppData\LocalLow", p))
                } else {
                    env::var(&key).ok()
                };
                if let Some(value) = value {
                    result.push_str(&value);
                } else {
                    result.push('%');
                    result.push_str(&key);
                    result.push('%');
                }
                index = end + 1;
                continue;
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

fn normalize_path(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn is_excluded(path: &Path, exclusions: &[String]) -> bool {
    let path = normalize_path(&path.to_string_lossy());
    exclusions.iter().any(|entry| {
        let entry = normalize_path(entry);
        !entry.is_empty()
            && (path == entry
                || path
                    .strip_prefix(&entry)
                    .is_some_and(|rest| rest.starts_with('\\')))
    })
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
