use serde_json::json;
use std::{
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Instant,
};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW:u32=0x08000000;

#[derive(Clone,Debug,Default)]
struct Analysis{
    explorer_size:String,actual_size:String,shared_size:String,backups_size:String,cache_size:String,
    reclaimable_packages:String,recommended:String,raw:String,actual_bytes:Option<u64>,estimated_bytes:Option<u64>,
}
struct CleanupResult{before:Analysis,after:Analysis,free_before:u64,free_after:u64,raw:String,reset_base:bool}
enum Event{Progress(u8,String),Analysis(Result<Analysis,String>),Cleanup(Result<CleanupResult,String>)}

#[derive(Default)]
pub struct WinSxsState{
    pub explorer_size:String,pub actual_size:String,pub shared_size:String,pub backups_size:String,pub cache_size:String,
    pub reclaimable_packages:String,pub recommended:String,pub raw:String,pub status:String,
    pub running:bool,pub progress:u8,pub operation:String,pub estimated_gain_bytes:Option<u64>,
    pub last_gain_bytes:Option<u64>,pub last_store_gain_bytes:Option<u64>,
    rx:Option<Receiver<Event>>,
}

impl WinSxsState{
    pub fn analyze(&mut self){
        if self.running{return;} self.begin("Analisando Component Store");
        let(tx,rx)=mpsc::channel();self.rx=Some(rx);
        thread::spawn(move||{let _=tx.send(Event::Progress(15,"Executando DISM /AnalyzeComponentStore".into()));let result=analyze_store();let _=tx.send(Event::Progress(100,"Análise concluída".into()));let _=tx.send(Event::Analysis(result));});
    }
    pub fn cleanup(&mut self,reset_base:bool){
        if self.running{return;} self.begin(if reset_base{"ResetBase"}else{"Limpando Component Store"});
        let(tx,rx)=mpsc::channel();self.rx=Some(rx);
        thread::spawn(move||run_cleanup(reset_base,tx));
    }
    pub fn poll(&mut self){
        let mut events=Vec::new();if let Some(rx)=&self.rx{while let Ok(e)=rx.try_recv(){events.push(e);}}
        for event in events{
            match event{
                Event::Progress(p,s)=>{self.progress=p;self.operation=s;}
                Event::Analysis(result)=>{self.running=false;self.rx=None;match result{Ok(a)=>{self.apply_analysis(a);self.status="Análise oficial do Component Store concluída.".into();}Err(e)=>{self.status=format!("Falha na análise: {e}");self.raw=e.clone();crate::diagnostics::event("winsxs_error","Falha na análise",json!({"error":e}));}}}
                Event::Cleanup(result)=>{self.running=false;self.rx=None;match result{Ok(r)=>{
                    let gain=r.free_after.saturating_sub(r.free_before);let store_gain=r.before.actual_bytes.zip(r.after.actual_bytes).map(|(b,a)|b.saturating_sub(a));
                    self.last_gain_bytes=Some(gain);self.last_store_gain_bytes=store_gain;self.raw=r.raw.clone();self.apply_analysis(r.after);
                    self.status=format!("{} concluído • ganho real no volume do Windows: {}",if r.reset_base{"ResetBase"}else{"Limpeza Component Store"},format_bytes(gain));
                    crate::diagnostics::event("winsxs_cleanup","Manutenção WinSxS concluída",json!({"reset_base":r.reset_base,"free_gain_bytes":gain,"store_gain_bytes":store_gain}));
                }Err(e)=>{self.status=format!("Falha na manutenção WinSxS: {e}");self.raw=e.clone();crate::diagnostics::event("winsxs_error","Falha na manutenção",json!({"error":e}));}}}
            }
        }
    }
    pub fn has_summary(&self)->bool{!self.explorer_size.is_empty()||!self.actual_size.is_empty()||!self.reclaimable_packages.is_empty()}
    fn begin(&mut self,name:&str){self.running=true;self.progress=0;self.operation=name.into();self.last_gain_bytes=None;self.last_store_gain_bytes=None;crate::diagnostics::operation_start("winsxs",name,json!({}));}
    fn apply_analysis(&mut self,a:Analysis){
        self.explorer_size=a.explorer_size;self.actual_size=a.actual_size;self.shared_size=a.shared_size;self.backups_size=a.backups_size;
        self.cache_size=a.cache_size;self.reclaimable_packages=a.reclaimable_packages;self.recommended=a.recommended;self.estimated_gain_bytes=a.estimated_bytes;self.raw=a.raw;
    }
}

fn run_cleanup(reset_base:bool,tx:Sender<Event>){
    let started=Instant::now();let _=tx.send(Event::Progress(5,"Medindo antes da limpeza".into()));
    let before=match analyze_store(){Ok(v)=>v,Err(e)=>{let _=tx.send(Event::Cleanup(Err(e)));return;}};
    let free_before=query_system_free().unwrap_or(0);
    let _=tx.send(Event::Progress(25,if reset_base{"Executando DISM ResetBase".into()}else{"Executando DISM StartComponentCleanup".into()}));
    let args=if reset_base{vec!["/Online","/Cleanup-Image","/StartComponentCleanup","/ResetBase","/English"]}else{vec!["/Online","/Cleanup-Image","/StartComponentCleanup","/English"]};
    let raw=match run_command("dism.exe",&args){Ok(v)=>v,Err(e)=>{let _=tx.send(Event::Cleanup(Err(e)));return;}};
    let _=tx.send(Event::Progress(85,"Medindo depois da limpeza".into()));
    let after=match analyze_store(){Ok(v)=>v,Err(e)=>{let _=tx.send(Event::Cleanup(Err(format!("DISM terminou, mas a análise final falhou: {e}\n\n{raw}"))));return;}};
    let free_after=query_system_free().unwrap_or(free_before);
    crate::diagnostics::operation_end("winsxs",if reset_base{"resetbase"}else{"component_cleanup"},true,started.elapsed().as_millis(),json!({"free_gain_bytes":free_after.saturating_sub(free_before)}));
    let _=tx.send(Event::Progress(100,"Concluído".into()));
    let _=tx.send(Event::Cleanup(Ok(CleanupResult{before,after,free_before,free_after,raw,reset_base})));
}

fn analyze_store()->Result<Analysis,String>{
    let raw=run_command("dism.exe",&["/Online","/Cleanup-Image","/AnalyzeComponentStore","/English"])?;
    let mut a=Analysis{raw:raw.clone(),..Default::default()};
    for line in raw.lines(){
        let Some((key,value))=line.trim().split_once(':')else{continue;};let k=key.trim().to_ascii_lowercase();let v=value.trim().to_string();
        if k.contains("windows explorer reported size of component store"){a.explorer_size=v;}
        else if k.contains("actual size of component store"){a.actual_bytes=parse_size(&v);a.actual_size=v;}
        else if k.contains("shared with windows"){a.shared_size=v;}
        else if k.contains("backups and disabled features"){a.backups_size=v;}
        else if k.contains("cache and temporary data"){a.cache_size=v;}
        else if k.contains("number of reclaimable packages"){a.reclaimable_packages=v;}
        else if k.contains("component store cleanup recommended"){a.recommended=v;}
    }
    a.estimated_bytes=Some(parse_size(&a.backups_size).unwrap_or(0).saturating_add(parse_size(&a.cache_size).unwrap_or(0)));
    Ok(a)
}
fn query_system_free()->Result<u64,String>{
    let script="$d=$env:SystemDrive.TrimEnd(':'); (Get-PSDrive -Name $d -ErrorAction Stop).Free";
    let out=run_command("powershell.exe",&["-NoProfile","-Command",script])?;
    out.trim().parse::<u64>().map_err(|e|format!("Falha ao medir espaço livre: {e}"))
}
fn parse_size(v:&str)->Option<u64>{
    let mut it=v.split_whitespace();let n=it.next()?.replace(',',"." ).parse::<f64>().ok()?;let unit=it.next().unwrap_or("bytes").to_ascii_lowercase();
    let mul=if unit.starts_with("gb"){1024f64.powi(3)}else if unit.starts_with("mb"){1024f64.powi(2)}else if unit.starts_with("kb"){1024.0}else{1.0};
    Some((n*mul)as u64)
}
fn run_command(program:&str,args:&[&str])->Result<String,String>{
    let mut c=Command::new(program);c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());#[cfg(windows)]c.creation_flags(CREATE_NO_WINDOW);
    let o=c.output().map_err(|e|format!("Falha ao executar {program}: {e}"))?;let mut t=String::from_utf8_lossy(&o.stdout).to_string();let e=String::from_utf8_lossy(&o.stderr);if !e.trim().is_empty(){t.push('\n');t.push_str(&e);}
    if o.status.success(){Ok(t)}else{Err(format!("{program} terminou com código {:?}.\n{t}",o.status.code()))}
}
pub fn format_bytes(n:u64)->String{let v=n as f64;let k=1024.0;if v>=k*k*k{format!("{:.2} GB",v/(k*k*k))}else if v>=k*k{format!("{:.2} MB",v/(k*k))}else if v>=k{format!("{:.2} KB",v/k)}else{format!("{n} B")}}
