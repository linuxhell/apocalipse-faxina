use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Default)]
pub struct DevelopmentState {
    pub output: String,
    pub status: String,
}

impl DevelopmentState {
    pub fn diagnose(&mut self) {
        let script = r#"
$ErrorActionPreference='SilentlyContinue'
$tools = 'py','python','pip','uv','poetry','conda'
'FERRAMENTAS ENCONTRADAS'
foreach($tool in $tools){
  $cmd = Get-Command $tool -ErrorAction SilentlyContinue
  if($cmd){ "$tool -> $($cmd.Source)" } else { "$tool -> (não encontrado)" }
}
''
'PIP'
if(Get-Command py -ErrorAction SilentlyContinue){ py -m pip cache info 2>&1 }
elseif(Get-Command python -ErrorAction SilentlyContinue){ python -m pip cache info 2>&1 }
else { 'Python/py não encontrado.' }
''
'UV'
if(Get-Command uv -ErrorAction SilentlyContinue){
  'Cache:'
  uv cache dir 2>&1
}else{'uv não encontrado.'}
''
'POETRY'
if(Get-Command poetry -ErrorAction SilentlyContinue){
  poetry cache list 2>&1
}else{'Poetry não encontrado.'}
''
'CONDA'
if(Get-Command conda -ErrorAction SilentlyContinue){
  conda info --base 2>&1
  conda clean --all --dry-run -y 2>&1
}else{'Conda não encontrado.'}
"#;
        self.output = run_capture("powershell.exe", &["-NoProfile", "-Command", script]);
        self.status = "Diagnóstico de caches de desenvolvimento concluído.".into();
    }

    pub fn clean_pip(&mut self) {
        let script = r#"
$ErrorActionPreference='Stop'
if(Get-Command py -ErrorAction SilentlyContinue){ py -m pip cache purge }
elseif(Get-Command python -ErrorAction SilentlyContinue){ python -m pip cache purge }
else{ throw 'Python/py não encontrado.' }
"#;
        self.output = run_capture("powershell.exe", &["-NoProfile", "-Command", script]);
        self.status = "Comando oficial de limpeza do cache do pip concluído.".into();
    }

    pub fn clean_uv(&mut self) {
        self.output = run_capture("uv.exe", &["cache", "clean"]);
        self.status = "Comando oficial de limpeza do cache do uv concluído.".into();
    }

    pub fn clean_poetry(&mut self) {
        let script = r#"
$ErrorActionPreference='Stop'
if(-not (Get-Command poetry -ErrorAction SilentlyContinue)){ throw 'Poetry não encontrado.' }
$names = @(poetry cache list 2>$null | ForEach-Object { $_.Trim() } | Where-Object { $_ })
if($names.Count -eq 0){ 'Nenhum cache do Poetry listado.'; exit 0 }
foreach($name in $names){
  "Limpando cache Poetry: $name"
  poetry cache clear $name --all --no-interaction 2>&1
}
"#;
        self.output = run_capture("powershell.exe", &["-NoProfile", "-Command", script]);
        self.status = "Limpeza dos caches listados pelo Poetry concluída.".into();
    }

    pub fn analyze_conda(&mut self) {
        self.output = run_capture("conda.exe", &["clean", "--all", "--dry-run", "-y"]);
        self.status = "Conda analisado em modo dry-run; nada foi apagado.".into();
    }

    pub fn clean_conda(&mut self) {
        self.output = run_capture("conda.exe", &["clean", "--all", "-y"]);
        self.status = "Limpeza oficial de caches/pacotes não usados do Conda concluída.".into();
    }
}

fn run_capture(program: &str, args: &[&str]) -> String {
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
            if text.trim().is_empty() {
                format!("Comando concluído com código {:?}.", output.status.code())
            } else {
                text
            }
        }
        Err(error) => format!("Ferramenta não encontrada ou falha ao executar: {error}"),
    }
}
