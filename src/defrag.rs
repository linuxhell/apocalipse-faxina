use serde_json::{json,Value};
use std::{
    io::{BufRead,BufReader,Read},
    process::{Command,Stdio},
    sync::{atomic::{AtomicBool,Ordering},mpsc::{self,Receiver,Sender},Arc},
    thread,time::{Duration,Instant},
};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW:u32=0x08000000;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum OptimizationMode{Intelligent,Quick,CompleteHdd,ConsolidateFree,HddDefrag,Retrim}
impl OptimizationMode{
 pub const ALL:[Self;6]=[Self::Intelligent,Self::Quick,Self::CompleteHdd,Self::ConsolidateFree,Self::HddDefrag,Self::Retrim];
 pub fn label(self)->&'static str{match self{Self::Intelligent=>"Inteligente / recomendado",Self::Quick=>"Rápido",Self::CompleteHdd=>"Completo para HDD",Self::ConsolidateFree=>"Consolidar espaço livre",Self::HddDefrag=>"Desfragmentar HDD",Self::Retrim=>"ReTRIM SSD/NVMe"}}
 pub fn description(self)->&'static str{match self{Self::Intelligent=>"Windows escolhe a otimização adequada à mídia (/O).",Self::Quick=>"HDD: desfragmentação direta; SSD/NVMe: ReTRIM.",Self::CompleteHdd=>"HDD: desfragmenta e depois consolida espaço livre.",Self::ConsolidateFree=>"HDD: consolida espaço livre (/X).",Self::HddDefrag=>"HDD: desfragmentação tradicional (/D).",Self::Retrim=>"SSD/NVMe: ReTRIM (/L), sem desfragmentação agressiva."}}
}
#[derive(Clone,Debug,Default)]
pub struct VolumeInfo{pub drive:String,pub label:String,pub file_system:String,pub health:String,pub media_type:String,pub bus_type:String,pub size:u64,pub free:u64}
#[derive(Clone,Debug,Default)]
pub struct VolumeChoice{pub info:VolumeInfo,pub checked:bool}
#[derive(Clone,Debug,Default)]
pub struct Snapshot{pub info:VolumeInfo,pub fragmentation_percent:Option<f32>,pub fragmented_files:Option<u64>,pub sequential_read_mbps:Option<f64>,pub random_read_mbps:Option<f64>,pub analysis_output:String}
#[derive(Clone,Debug,Default)]
pub struct DriveReport{pub drive:String,pub before:Snapshot,pub after:Option<Snapshot>,pub status:String}
enum Event{Detected(Result<Vec<VolumeInfo>,String>),Progress(f32,String,String),Before(Snapshot),After(Snapshot),Output(String),Finished(String),Failed(String),Cancelled}

pub struct DefragState{
 pub selected_mode:usize,pub volumes:Vec<VolumeChoice>,pub reports:Vec<DriveReport>,pub running:bool,pub detecting:bool,
 pub progress:f32,pub phase:String,pub status:String,pub output:String,pub current_drive:String,
 rx:Option<Receiver<Event>>,cancel:Arc<AtomicBool>,
}
impl Default for DefragState{
 fn default()->Self{Self{selected_mode:0,volumes:vec![],reports:vec![],running:false,detecting:false,progress:0.0,phase:"Pronto".into(),status:"Detectando unidades…".into(),output:String::new(),current_drive:String::new(),rx:None,cancel:Arc::new(AtomicBool::new(false))}}
}
impl DefragState{
 pub fn selected_mode(&self)->OptimizationMode{OptimizationMode::ALL.get(self.selected_mode).copied().unwrap_or(OptimizationMode::Intelligent)}
 pub fn start_detect(&mut self){
  if self.running||self.detecting{return;}let(tx,rx)=mpsc::channel();self.rx=Some(rx);self.detecting=true;self.status="Detectando unidades…".into();
  thread::spawn(move||{let _=tx.send(Event::Detected(query_all_volumes()));});
 }
 pub fn select_all(&mut self,checked:bool){for v in &mut self.volumes{v.checked=checked;}}
 pub fn selected_count(&self)->usize{self.volumes.iter().filter(|v|v.checked).count()}
 pub fn analyze_selected(&mut self){
  if self.running{return;}let drives:Vec<String>=self.volumes.iter().filter(|v|v.checked).map(|v|v.info.drive.clone()).collect();
  if drives.is_empty(){self.status="Marque pelo menos uma unidade.".into();return;}self.begin();let(tx,rx)=mpsc::channel();self.rx=Some(rx);let cancel=self.cancel.clone();
  thread::spawn(move||run_multi(drives,None,tx,cancel));
 }
 pub fn optimize_selected(&mut self,mode:OptimizationMode){
  if self.running{return;}let drives:Vec<String>=self.volumes.iter().filter(|v|v.checked).map(|v|v.info.drive.clone()).collect();
  if drives.is_empty(){self.status="Marque pelo menos uma unidade.".into();return;}self.begin();let(tx,rx)=mpsc::channel();self.rx=Some(rx);let cancel=self.cancel.clone();
  thread::spawn(move||run_multi(drives,Some(mode),tx,cancel));
 }
 pub fn cancel(&mut self){if self.running{self.cancel.store(true,Ordering::Relaxed);self.phase="Solicitando parada…".into();}}
 pub fn poll(&mut self){
  let mut events=vec![];if let Some(rx)=&self.rx{while let Ok(e)=rx.try_recv(){events.push(e);}}
  for e in events{match e{
   Event::Detected(result)=>{self.detecting=false;self.rx=None;match result{Ok(infos)=>{let prev:Vec<String>=self.volumes.iter().filter(|v|v.checked).map(|v|v.info.drive.clone()).collect();self.volumes=infos.into_iter().map(|info|VolumeChoice{checked:prev.is_empty()||prev.contains(&info.drive),info}).collect();self.status=format!("{} unidade(s) detectada(s).",self.volumes.len());}Err(err)=>self.status=err}}
   Event::Progress(p,d,ph)=>{self.progress=p;self.current_drive=d;self.phase=ph;}
   Event::Before(s)=>{let drive=s.info.drive.clone();if let Some(r)=self.reports.iter_mut().find(|r|r.drive==drive){r.before=s;r.status="Análise antes concluída".into();}else{self.reports.push(DriveReport{drive,before:s,after:None,status:"Análise concluída".into()});}}
   Event::After(s)=>{let drive=s.info.drive.clone();if let Some(r)=self.reports.iter_mut().find(|r|r.drive==drive){r.after=Some(s);r.status="Otimização e medição concluídas".into();}}
   Event::Output(o)=>{if !self.output.is_empty(){self.output.push('\n');}self.output.push_str(&o);}
   Event::Finished(s)=>{self.running=false;self.rx=None;self.progress=100.0;self.phase="Concluído".into();self.status=s;}
   Event::Failed(s)=>{self.running=false;self.rx=None;self.phase="Falha".into();self.status=s.clone();crate::diagnostics::event("disk_error","Falha em discos",json!({"error":s}));}
   Event::Cancelled=>{self.running=false;self.rx=None;self.phase="Interrompido".into();self.status="Operação interrompida.".into();}
  }}
 }
 fn begin(&mut self){self.cancel.store(false,Ordering::Relaxed);self.running=true;self.progress=0.0;self.phase="Preparando".into();self.status="Operação em andamento.".into();self.output.clear();self.reports.clear();crate::diagnostics::operation_start("disks","multi_volume",json!({"selected":self.selected_count()}));}
}

fn run_multi(drives:Vec<String>,mode:Option<OptimizationMode>,tx:Sender<Event>,cancel:Arc<AtomicBool>){
 let started=Instant::now();let total=drives.len().max(1);
 for(index,drive)in drives.iter().enumerate(){
  if cancel.load(Ordering::Relaxed){let _=tx.send(Event::Cancelled);return;}
  let base=index as f32/total as f32*100.0;let span=100.0/total as f32;
  let _=tx.send(Event::Progress(base+2.0,drive.clone(),"Analisando antes".into()));
  let before=match capture_snapshot(drive,mode.is_some()){Ok(s)=>s,Err(e)=>{let _=tx.send(Event::Failed(format!("{drive}: {e}")));return;}};
  let _=tx.send(Event::Before(before.clone()));
  if let Some(mode)=mode{
   if let Err(e)=validate_mode(mode,&before.info){let _=tx.send(Event::Failed(e));return;}
   let commands=commands_for(mode,&before.info);let n=commands.len().max(1);
   for(ci,args)in commands.iter().enumerate(){
    let start=base+10.0+span*(ci as f32/n as f32)*0.65;let step=span*0.65/n as f32;
    let _=tx.send(Event::Progress(start,drive.clone(),format!("{} — etapa {}/{}",mode.label(),ci+1,n)));
    match run_defrag_stream(drive,args,&tx,&cancel,start,step){Ok(o)=>{let _=tx.send(Event::Output(format!("=== {} ===\n{}",drive,o)));}Err(e)=>{let _=tx.send(Event::Failed(e));return;}}
   }
   let _=tx.send(Event::Progress(base+span*0.82,drive.clone(),"Medindo depois".into()));
   let after=match capture_snapshot(drive,true){Ok(s)=>s,Err(e)=>{let _=tx.send(Event::Failed(format!("{drive}: otimizado, mas medição final falhou: {e}")));return;}};
   let _=tx.send(Event::After(after));
  }
  let _=tx.send(Event::Progress(base+span,drive.clone(),"Unidade concluída".into()));
 }
 crate::diagnostics::operation_end("disks",if mode.is_some(){"optimize_multi"}else{"analyze_multi"},true,started.elapsed().as_millis(),json!({"drives":drives}));
 let _=tx.send(Event::Finished(format!("{} unidade(s) concluída(s).",drives.len())));
}

fn validate_mode(mode:OptimizationMode,info:&VolumeInfo)->Result<(),String>{
 let ssd=is_ssd(info);let hdd=info.media_type.to_ascii_lowercase().contains("hdd")||info.media_type.to_ascii_lowercase().contains("hard disk");
 match mode{OptimizationMode::CompleteHdd|OptimizationMode::ConsolidateFree|OptimizationMode::HddDefrag if ssd=>Err(format!("{} bloqueado em {} porque a unidade foi identificada como SSD/NVMe.",mode.label(),info.drive)),OptimizationMode::Retrim if hdd=>Err(format!("ReTRIM bloqueado em {} porque foi identificado como HDD.",info.drive)),_=>Ok(())}
}
fn is_ssd(info:&VolumeInfo)->bool{let m=info.media_type.to_ascii_lowercase();let b=info.bus_type.to_ascii_lowercase();m.contains("ssd")||m.contains("solid")||b.contains("nvme")}
fn commands_for(mode:OptimizationMode,info:&VolumeInfo)->Vec<Vec<String>>{match mode{OptimizationMode::Intelligent=>vec![vec!["/O".into(),"/U".into(),"/V".into()]],OptimizationMode::Quick if is_ssd(info)=>vec![vec!["/L".into(),"/U".into()]],OptimizationMode::Quick=>vec![vec!["/D".into(),"/U".into()]],OptimizationMode::CompleteHdd=>vec![vec!["/D".into(),"/U".into(),"/V".into()],vec!["/X".into(),"/U".into(),"/V".into()]],OptimizationMode::ConsolidateFree=>vec![vec!["/X".into(),"/U".into(),"/V".into()]],OptimizationMode::HddDefrag=>vec![vec!["/D".into(),"/U".into(),"/V".into()]],OptimizationMode::Retrim=>vec![vec!["/L".into(),"/U".into(),"/V".into()]]}}

fn query_all_volumes()->Result<Vec<VolumeInfo>,String>{
 let script=r#"[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$items=@(Get-Volume | Where-Object {$_.DriveLetter} | ForEach-Object {
 $v=$_;$p=Get-Partition -DriveLetter $v.DriveLetter -ErrorAction SilentlyContinue;$d=$null;$pd=$null
 if($p){$d=Get-Disk -Number $p.DiskNumber -ErrorAction SilentlyContinue}
 if($d){$pd=Get-PhysicalDisk -ErrorAction SilentlyContinue | Where-Object {([string]$_.DeviceId -eq [string]$d.Number) -or ($_.FriendlyName -eq $d.FriendlyName)} | Select-Object -First 1}
 [PSCustomObject]@{Drive=([string]$v.DriveLetter+':');Label=[string]$v.FileSystemLabel;FileSystem=[string]$v.FileSystem;Health=[string]$v.HealthStatus;MediaType=if($pd){[string]$pd.MediaType}else{'Unspecified'};BusType=if($d){[string]$d.BusType}else{''};Size=[uint64]$v.Size;Free=[uint64]$v.SizeRemaining}
});[Console]::Write((ConvertTo-Json -InputObject $items -Compress -Depth 4))"#;
 let out=run_capture("powershell.exe",&["-NoProfile","-Command",script])?;parse_volume_list(&out)
}
fn parse_volume_list(out:&str)->Result<Vec<VolumeInfo>,String>{
 let v:Value=serde_json::from_str(out.trim()).map_err(|e|format!("Falha ao interpretar unidades: {e}\n{out}"))?;let arr=if let Some(a)=v.as_array(){a.clone()}else{vec![v]};
 Ok(arr.into_iter().filter_map(|x|{let drive=x["Drive"].as_str()?.to_string();Some(VolumeInfo{drive,label:x["Label"].as_str().unwrap_or("").into(),file_system:x["FileSystem"].as_str().unwrap_or("").into(),health:x["Health"].as_str().unwrap_or("").into(),media_type:x["MediaType"].as_str().unwrap_or("Unspecified").into(),bus_type:x["BusType"].as_str().unwrap_or("").into(),size:x["Size"].as_u64().unwrap_or(0),free:x["Free"].as_u64().unwrap_or(0)})}).collect())
}
fn query_volume_info(drive:&str)->Result<VolumeInfo,String>{query_all_volumes()?.into_iter().find(|v|v.drive.eq_ignore_ascii_case(drive)).ok_or_else(||format!("Unidade {drive} não encontrada."))}
fn capture_snapshot(drive:&str,bench:bool)->Result<Snapshot,String>{let info=query_volume_info(drive)?;let out=run_capture("defrag.exe",&[drive,"/A","/V"])?;let(frag,files)=(parse_fragmentation_percent(&out),parse_fragmented_files(&out));let(seq,ran)=if bench{(run_winsat_read(drive,false),run_winsat_read(drive,true))}else{(None,None)};Ok(Snapshot{info,fragmentation_percent:frag,fragmented_files:files,sequential_read_mbps:seq,random_read_mbps:ran,analysis_output:out})}
fn run_defrag_stream(drive:&str,args:&[String],tx:&Sender<Event>,cancel:&Arc<AtomicBool>,start:f32,span:f32)->Result<String,String>{
 let mut c=Command::new("defrag.exe");c.arg(drive).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());#[cfg(windows)]c.creation_flags(CREATE_NO_WINDOW);
 let mut child=c.spawn().map_err(|e|format!("Falha ao iniciar defrag.exe: {e}"))?;let stdout=child.stdout.take().ok_or_else(||"Sem stdout do defrag.".to_string())?;let mut text=String::new();
 for line in BufReader::new(stdout).lines(){let line=line.unwrap_or_default();text.push_str(&line);text.push('\n');if let Some(p)=extract_percent(&line){let _=tx.send(Event::Progress(start+span*(p/100.0),drive.into(),"Otimizando".into()));}if cancel.load(Ordering::Relaxed){let _=child.kill();let _=child.wait();return Err("Operação interrompida.".into());}}
 let status=child.wait().map_err(|e|e.to_string())?;if let Some(mut e)=child.stderr.take(){let _=e.read_to_string(&mut text);}if status.success(){Ok(text)}else{Err(format!("defrag.exe terminou com código {:?}.\n{text}",status.code()))}
}
fn run_winsat_read(drive:&str,random:bool)->Option<f64>{let letter=drive.chars().find(|c|c.is_ascii_alphabetic())?.to_ascii_uppercase().to_string();let mode=if random{"-ran"}else{"-seq"};extract_mbps(&run_capture("winsat.exe",&["disk",mode,"-read","-drive",&letter]).ok()?)}
fn run_capture(program:&str,args:&[&str])->Result<String,String>{let mut c=Command::new(program);c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());#[cfg(windows)]c.creation_flags(CREATE_NO_WINDOW);let o=c.output().map_err(|e|format!("Falha ao executar {program}: {e}"))?;let mut t=String::from_utf8_lossy(&o.stdout).to_string();let e=String::from_utf8_lossy(&o.stderr);if !e.trim().is_empty(){t.push('\n');t.push_str(&e);}if o.status.success(){Ok(t)}else{Err(format!("{program} terminou com código {:?}.\n{t}",o.status.code()))}}
fn parse_fragmentation_percent(o:&str)->Option<f32>{let mut f=None;for l in o.lines(){if l.to_ascii_lowercase().contains("fragment")&&l.contains('%'){if let Some(v)=extract_percent(l){f=Some(v);}}}f}
fn parse_fragmented_files(o:&str)->Option<u64>{for l in o.lines(){let x=l.to_ascii_lowercase();if x.contains("fragmented files")||x.contains("arquivos fragment"){return extract_first_integer(l);}}None}
fn extract_percent(l:&str)->Option<f32>{let p=l.find('%')?;l[..p].split(|c:char|!(c.is_ascii_digit()||c=='.'||c==',')).filter(|x|!x.is_empty()).last()?.replace(',','.').parse().ok()}
fn extract_first_integer(l:&str)->Option<u64>{l.split(|c:char|!c.is_ascii_digit()).find(|x|!x.is_empty())?.parse().ok()}
fn extract_mbps(o:&str)->Option<f64>{for l in o.lines(){if !l.to_ascii_lowercase().contains("mb/s"){continue;}let t:Vec<_>=l.split_whitespace().collect();for(i,v)in t.iter().enumerate(){if v.to_ascii_lowercase().contains("mb/s")&&i>0{if let Ok(n)=t[i-1].replace(',','.').parse(){return Some(n);}}}}None}
pub fn format_bytes(n:u64)->String{let v=n as f64;let k=1024.0;if v>=k*k*k*k{format!("{:.2} TB",v/(k*k*k*k))}else if v>=k*k*k{format!("{:.2} GB",v/(k*k*k))}else if v>=k*k{format!("{:.2} MB",v/(k*k))}else{format!("{:.2} KB",v/k)}}
pub fn performance_delta(a:Option<f64>,b:Option<f64>)->String{match(a,b){(Some(x),Some(y))if x>0.0=>format!("{x:.1} → {y:.1} MB/s ({:+.1}%)",(y-x)/x*100.0),(Some(x),Some(y))=>format!("{x:.1} → {y:.1} MB/s"),_=>"indisponível".into()}}
pub fn sleep_repaint_hint()->Duration{Duration::from_millis(120)}
