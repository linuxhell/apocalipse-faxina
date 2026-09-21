use serde_json::{json, Value};
use std::{
    backtrace::Backtrace,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{atomic::{AtomicU64, Ordering}, Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW:u32=0x08000000;
static LOGGER:OnceLock<DiagnosticLogger>=OnceLock::new();

pub struct DiagnosticLogger{
    root:PathBuf,session_dir:PathBuf,session_id:String,started:Instant,
    file:Mutex<File>,event_count:AtomicU64,last_event:Mutex<String>,
}

pub fn init(portable_root:&Path){
    if LOGGER.get().is_some(){return;}
    let session_id=format!("{}-{}",unix_seconds(),std::process::id());
    let root=portable_root.join("logs").join("diagnostics");
    let session_dir=root.join(&session_id);
    let _=fs::create_dir_all(&session_dir); rotate_sessions(&root,12);
    let Ok(file)=OpenOptions::new().create(true).append(true).open(session_dir.join("session.jsonl")) else{return;};
    let _=LOGGER.set(DiagnosticLogger{root,session_dir,session_id,started:Instant::now(),file:Mutex::new(file),event_count:AtomicU64::new(0),last_event:Mutex::new(String::new())});
    install_panic_hook();
    event("session_start","Apocalipse Faxina iniciado",json!({"version":env!("CARGO_PKG_VERSION"),"pid":std::process::id(),"arch":std::env::consts::ARCH,"os":std::env::consts::OS}));
}
pub fn event(kind:&str,message:&str,data:Value){
    let Some(l)=LOGGER.get()else{return;};let seq=l.event_count.fetch_add(1,Ordering::Relaxed)+1;
    let row=json!({"seq":seq,"unix_ms":unix_millis(),"elapsed_ms":l.started.elapsed().as_millis() as u64,"kind":kind,"message":message,"data":data});
    if let Ok(mut last)=l.last_event.lock(){*last=format!("{kind}: {message}");}
    if let Ok(mut f)=l.file.lock(){let _=writeln!(f,"{}",row);let _=f.flush();}
}
pub fn ui_click(section:&str,x:f32,y:f32,secondary:bool){event("ui_click","Clique na interface",json!({"section":section,"x":x,"y":y,"secondary":secondary}));}
pub fn frame_finished(section:&str,elapsed_ms:u128){if elapsed_ms>=350{event("ui_stall","Frame da interface demorou acima do limite",json!({"section":section,"elapsed_ms":elapsed_ms,"severity":if elapsed_ms>=2000{"critical"}else{"warning"}}));}}
pub fn operation_start(area:&str,operation:&str,data:Value){event("operation_start",&format!("{area}: {operation}"),data);}
pub fn operation_end(area:&str,operation:&str,ok:bool,elapsed_ms:u128,data:Value){event(if ok{"operation_success"}else{"operation_error"},&format!("{area}: {operation}"),json!({"ok":ok,"elapsed_ms":elapsed_ms,"details":data}));}
pub fn session_id()->String{LOGGER.get().map(|x|x.session_id.clone()).unwrap_or_else(||"inativo".into())}
pub fn session_dir()->Option<PathBuf>{LOGGER.get().map(|x|x.session_dir.clone())}
pub fn logs_root()->Option<PathBuf>{LOGGER.get().map(|x|x.root.clone())}
pub fn event_count()->u64{LOGGER.get().map(|x|x.event_count.load(Ordering::Relaxed)).unwrap_or(0)}
pub fn last_event()->String{LOGGER.get().and_then(|x|x.last_event.lock().ok().map(|v|v.clone())).unwrap_or_default()}

pub fn export_zip(portable_root:&Path)->Result<PathBuf,String>{
    let l=LOGGER.get().ok_or_else(||"Diagnóstico não inicializado.".to_string())?;
    event("diagnostic_export","Iniciando exportação",json!({}));
    let inv=l.session_dir.join("inventory");fs::create_dir_all(&inv).map_err(|e|e.to_string())?;
    let captures=[
      ("system.json","Get-ComputerInfo | Select WindowsProductName,WindowsVersion,OsBuildNumber,OsArchitecture,CsManufacturer,CsModel,CsTotalPhysicalMemory,BiosSMBIOSBIOSVersion | ConvertTo-Json -Depth 4"),
      ("processes.json","Get-Process | Sort ProcessName | Select ProcessName,Id,CPU,WorkingSet64,Path | ConvertTo-Json -Depth 4"),
      ("services.json","Get-CimInstance Win32_Service | Select Name,DisplayName,State,StartMode,StartName,PathName | ConvertTo-Json -Depth 4"),
      ("startup.json","Get-CimInstance Win32_StartupCommand | Select Name,Command,Location,User | ConvertTo-Json -Depth 4"),
      ("tasks.json","Get-ScheduledTask | Select TaskName,TaskPath,State,Author | ConvertTo-Json -Depth 4"),
      ("storage.json","$a=Get-Volume | Select DriveLetter,FileSystemLabel,FileSystem,HealthStatus,Size,SizeRemaining; $b=Get-Disk | Select Number,FriendlyName,BusType,HealthStatus,OperationalStatus,Size; $c=Get-PhysicalDisk | Select FriendlyName,MediaType,BusType,HealthStatus,Size; [PSCustomObject]@{Volumes=$a;Disks=$b;PhysicalDisks=$c}|ConvertTo-Json -Depth 5"),
      ("recent-errors.json","Get-WinEvent -FilterHashtable @{LogName='Application','System'; Level=1,2,3; StartTime=(Get-Date).AddDays(-2)} -ErrorAction SilentlyContinue | Select -First 250 TimeCreated,LogName,ProviderName,Id,LevelDisplayName,Message | ConvertTo-Json -Depth 4"),
    ];
    for(n,s)in captures{let _=fs::write(inv.join(n),run_powershell(s));}
    let _=fs::write(inv.join("driver-store.txt"),run_program("pnputil.exe",&["/enum-drivers"]));
    let manifest=json!({"product":"Apocalipse Faxina","version":env!("CARGO_PKG_VERSION"),"session_id":l.session_id,"event_count":event_count(),"generated_unix_ms":unix_millis(),"privacy":"Senhas, cookies, credenciais e tokens não são coletados intencionalmente. Caminhos e inventário técnico são incluídos."});
    let _=fs::write(l.session_dir.join("manifest.json"),serde_json::to_vec_pretty(&manifest).unwrap_or_default());
    let out=portable_root.join("diagnostics");fs::create_dir_all(&out).map_err(|e|e.to_string())?;
    let dst=out.join(format!("Apocalipse-Faxina-Diagnostico-{}.zip",l.session_id));let _=fs::remove_file(&dst);
    let src=ps_quote(&l.session_dir.to_string_lossy());let dest=ps_quote(&dst.to_string_lossy());
    let result=run_powershell(&format!("Compress-Archive -Path '{}\\*' -DestinationPath '{}' -CompressionLevel Optimal -Force",src,dest));
    if !dst.is_file(){return Err(format!("Falha ao gerar ZIP: {result}"));}
    event("diagnostic_export","Diagnóstico exportado",json!({"path":dst.to_string_lossy()}));Ok(dst)
}
fn install_panic_hook(){
    let prev=std::panic::take_hook();
    std::panic::set_hook(Box::new(move|info|{
        let payload=if let Some(v)=info.payload().downcast_ref::<&str>(){(*v).to_string()}else if let Some(v)=info.payload().downcast_ref::<String>(){v.clone()}else{"panic sem mensagem textual".into()};
        let location=info.location().map(|x|format!("{}:{}:{}",x.file(),x.line(),x.column())).unwrap_or_else(||"desconhecido".into());
        let backtrace=Backtrace::force_capture().to_string();
        event("panic","Panic capturado",json!({"payload":payload,"location":location,"backtrace":backtrace}));
        if let Some(l)=LOGGER.get(){let _=fs::write(l.session_dir.join("crash.txt"),format!("Apocalipse Faxina {}\nSessão: {}\nLocal: {}\nMensagem: {}\n\nBacktrace:\n{}\n",env!("CARGO_PKG_VERSION"),l.session_id,location,payload,backtrace));}
        prev(info);
    }));
}
fn run_powershell(s:&str)->String{run_program("powershell.exe",&["-NoProfile","-NonInteractive","-Command",s])}
fn run_program(p:&str,a:&[&str])->String{let mut c=Command::new(p);c.args(a).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());#[cfg(windows)]c.creation_flags(CREATE_NO_WINDOW);match c.output(){Ok(o)=>{let mut t=String::from_utf8_lossy(&o.stdout).to_string();let e=String::from_utf8_lossy(&o.stderr);if !e.trim().is_empty(){t.push('\n');t.push_str(&e);}t},Err(e)=>format!("Falha ao executar {p}: {e}")}}
fn ps_quote(v:&str)->String{v.replace('\'',"''")}
fn rotate_sessions(root:&Path,keep:usize){let Ok(e)=fs::read_dir(root)else{return;};let mut d:Vec<_>=e.flatten().map(|x|x.path()).filter(|p|p.is_dir()).collect();d.sort();if d.len()>keep{let n=d.len()-keep;for p in d.into_iter().take(n){let _=fs::remove_dir_all(p);}}}
fn unix_seconds()->u64{SystemTime::now().duration_since(UNIX_EPOCH).map(|x|x.as_secs()).unwrap_or(0)}
fn unix_millis()->u128{SystemTime::now().duration_since(UNIX_EPOCH).map(|x|x.as_millis()).unwrap_or(0)}
