use same_file::Handle;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug)]
pub struct DuplicateFile {
    pub path: PathBuf,
    pub size: u64,
    pub checked: bool,
    pub is_video: bool,
    pub thumbnail_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct DuplicateGroup {
    pub hash: String,
    pub files: Vec<DuplicateFile>,
}

pub struct DuplicateState {
    pub root: String,
    pub groups: Vec<DuplicateGroup>,
    pub status: String,
}

impl Default for DuplicateState {
    fn default() -> Self {
        Self {
            root: std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".into()),
            groups: Vec::new(),
            status: "Escolha uma pasta/unidade. A confirmação usa tamanho + SHA-256; nomes iguais não bastam.".into(),
        }
    }
}

impl DuplicateState {
    pub fn analyze(&mut self, exclusions: &[String], portable_root: &Path, cache_dir: &Path) {
        self.groups.clear();
        let root = PathBuf::from(self.root.trim());
        if !root.exists() {
            self.status = "O caminho informado não existe.".into();
            return;
        }

        let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        collect_files(&root, exclusions, 0, &mut by_size);

        let mut by_hash: HashMap<(u64, String), Vec<PathBuf>> = HashMap::new();
        for (size, paths) in by_size.into_iter().filter(|(_, paths)| paths.len() > 1) {
            let mut physical_handles: Vec<Handle> = Vec::new();
            for path in paths {
                if let Ok(handle) = Handle::from_path(&path) {
                    if physical_handles.iter().any(|seen| seen == &handle) {
                        continue;
                    }
                    physical_handles.push(handle);
                }
                if let Ok(hash) = sha256_file(&path) {
                    by_hash.entry((size, hash)).or_default().push(path);
                }
            }
        }

        let ffmpeg = find_ffmpeg(portable_root);
        let thumbnail_dir = cache_dir.join("duplicate-thumbnails");
        let _ = fs::create_dir_all(&thumbnail_dir);

        let mut groups = Vec::new();
        for ((size, hash), mut paths) in by_hash {
            if paths.len() < 2 {
                continue;
            }
            paths.sort();
            let mut files = Vec::new();
            for path in paths {
                let is_video = is_video(&path);
                let thumbnail_path = if is_video {
                    ffmpeg
                        .as_ref()
                        .and_then(|exe| generate_thumbnail(exe, &path, &thumbnail_dir))
                } else {
                    None
                };
                files.push(DuplicateFile {
                    path,
                    size,
                    checked: false,
                    is_video,
                    thumbnail_path,
                });
            }
            groups.push(DuplicateGroup { hash, files });
        }

        groups.sort_by(|a, b| {
            let a_size = a.files.first().map(|x| x.size).unwrap_or(0);
            let b_size = b.files.first().map(|x| x.size).unwrap_or(0);
            b_size.cmp(&a_size)
        });
        self.groups = groups;

        let recoverable = self.recoverable_size();
        self.status = if ffmpeg.is_some() {
            format!(
                "{} grupos de duplicados reais • recuperável se mantida uma cópia por grupo: {} • thumbnails de vídeo habilitadas",
                self.groups.len(),
                fmt_bytes(recoverable)
            )
        } else {
            format!(
                "{} grupos de duplicados reais • recuperável: {} • thumbnails de vídeo aguardam ffmpeg.exe em data/ ou no PATH",
                self.groups.len(),
                fmt_bytes(recoverable)
            )
        };
    }

    pub fn recoverable_size(&self) -> u64 {
        self.groups
            .iter()
            .map(|group| {
                let size = group.files.first().map(|x| x.size).unwrap_or(0);
                size.saturating_mul(group.files.len().saturating_sub(1) as u64)
            })
            .sum()
    }

    pub fn selected_size(&self) -> u64 {
        self.groups
            .iter()
            .flat_map(|group| &group.files)
            .filter(|file| file.checked)
            .map(|file| file.size)
            .sum()
    }

    pub fn clear_selection(&mut self) {
        for file in self.groups.iter_mut().flat_map(|group| &mut group.files) {
            file.checked = false;
        }
    }

    pub fn delete_selected(&mut self, exclusions: &[String]) -> (u64, usize) {
        let mut removed = 0u64;
        let mut protected_groups = 0usize;

        for group in &mut self.groups {
            let selected = group.files.iter().filter(|f| f.checked).count();
            if selected == 0 {
                continue;
            }
            if selected >= group.files.len() {
                protected_groups += 1;
                continue;
            }

            for file in &group.files {
                if !file.checked || is_excluded(&file.path, exclusions) {
                    continue;
                }
                if fs::remove_file(&file.path).is_ok() {
                    removed = removed.saturating_add(file.size);
                }
            }
            group.files.retain(|file| file.path.exists());
        }

        self.groups.retain(|group| group.files.len() >= 2);
        self.status = if protected_groups > 0 {
            format!(
                "Exclusão concluída: {} liberados. {} grupo(s) não foram apagados porque todas as cópias estavam marcadas.",
                fmt_bytes(removed),
                protected_groups
            )
        } else {
            format!("Exclusão concluída: {} liberados.", fmt_bytes(removed))
        };
        (removed, protected_groups)
    }
}

fn collect_files(
    dir: &Path,
    exclusions: &[String],
    depth: usize,
    by_size: &mut HashMap<u64, Vec<PathBuf>>,
) {
    if depth > 64 || is_excluded(dir, exclusions) || should_skip_directory(dir) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_excluded(&path, exclusions) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect_files(&path, exclusions, depth + 1, by_size);
        } else if meta.is_file() && meta.len() > 0 {
            by_size.entry(meta.len()).or_default().push(path);
        }
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn find_ffmpeg(portable_root: &Path) -> Option<PathBuf> {
    for candidate in [
        portable_root.join("data").join("ffmpeg.exe"),
        portable_root.join("ffmpeg.exe"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let mut command = Command::new("where.exe");
    command.arg("ffmpeg.exe").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

fn generate_thumbnail(ffmpeg: &Path, video: &Path, thumbnail_dir: &Path) -> Option<PathBuf> {
    let mut hasher = Sha256::new();
    hasher.update(video.to_string_lossy().as_bytes());
    let key = format!("{:x}", hasher.finalize());
    let output = thumbnail_dir.join(format!("{key}.jpg"));
    if output.is_file() {
        return Some(output);
    }

    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", "00:00:01"])
        .arg("-i")
        .arg(video)
        .args(["-frames:v", "1", "-vf", "scale=240:-1"])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let status = command.status().ok()?;
    (status.success() && output.is_file()).then_some(output)
}

fn is_video(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|x| x.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    [
        "mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "mpg", "mpeg", "ts", "mts",
    ]
    .contains(&ext.as_str())
}

fn should_skip_directory(path: &Path) -> bool {
    let p = normalize_path(&path.to_string_lossy());
    let protected = [
        "\\windows",
        "\\program files",
        "\\program files (x86)",
        "\\programdata\\package cache",
        "\\system volume information",
        "\\$recycle.bin",
        "\\recovery",
    ];
    protected.iter().any(|part| p.ends_with(part) || p.contains(&format!("{part}\\")))
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
