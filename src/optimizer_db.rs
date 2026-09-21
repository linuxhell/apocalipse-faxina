pub const DB_VERSION:&str="2026.09.21";
pub const DB_SOURCE_NOTE:&str="Base conservadora Windows 10/11. O preset de serviços só marca candidatos a Manual; nunca desativa serviços.";
struct StartupRule{tokens:&'static[&'static str],reason:&'static str}
const STARTUP_RULES:&[StartupRule]=&[
 StartupRule{tokens:&["steam"],reason:"Launcher de jogos; não é necessário no boot."},StartupRule{tokens:&["epicgameslauncher","epic games launcher"],reason:"Launcher de jogos; pode abrir sob demanda."},
 StartupRule{tokens:&["gog galaxy","galaxyclient"],reason:"Launcher de jogos; pode abrir sob demanda."},StartupRule{tokens:&["battle.net"],reason:"Launcher de jogos; pode abrir sob demanda."},
 StartupRule{tokens:&["riotclient","riot client"],reason:"Launcher de jogos; pode abrir sob demanda."},StartupRule{tokens:&["eadesktop","ea desktop"],reason:"Launcher de jogos; pode abrir sob demanda."},
 StartupRule{tokens:&["ubisoft","upc.exe"],reason:"Launcher de jogos; pode abrir sob demanda."},StartupRule{tokens:&["discord"],reason:"Comunicação opcional no logon."},
 StartupRule{tokens:&["spotify"],reason:"Mídia opcional no logon."},StartupRule{tokens:&["msteams","teams.exe","ms-teams"],reason:"Comunicação opcional no logon."},
 StartupRule{tokens:&["skype"],reason:"Comunicação opcional no logon."},StartupRule{tokens:&["telegram"],reason:"Comunicação opcional no logon."},
 StartupRule{tokens:&["whatsapp"],reason:"Comunicação opcional no logon."},StartupRule{tokens:&["slack"],reason:"Comunicação opcional no logon."},
 StartupRule{tokens:&["zoom"],reason:"Cliente de reuniões opcional."},StartupRule{tokens:&["webex"],reason:"Cliente de reuniões opcional."},
 StartupRule{tokens:&["ccxprocess","adobe creative cloud"],reason:"Auxiliar Adobe; não é requisito do boot."},StartupRule{tokens:&["adobegcinvoker","adobe updater"],reason:"Atualizador Adobe; não é requisito do boot."},
 StartupRule{tokens:&["jusched","java update scheduler"],reason:"Agendador Java; não é requisito do boot."},StartupRule{tokens:&["opera browser assistant"],reason:"Assistente de navegador opcional."},
 StartupRule{tokens:&["onedrive"],reason:"Sincronização opcional; só marque se aceitar iniciar manualmente."},StartupRule{tokens:&["googledrive","google drive"],reason:"Sincronização opcional; só marque se aceitar iniciar manualmente."},
 StartupRule{tokens:&["dropbox"],reason:"Sincronização opcional; só marque se aceitar iniciar manualmente."},
];
const PROTECTED:&[&str]=&["securityhealth","windows defender","msmpeng","antivirus","avira","bitdefender","kaspersky","eset","malwarebytes","vpn","wireguard","openvpn","nvidia","amd","realtek","logitech","razer","corsair","steelseries","synaptics","touchpad","audio"];
struct ServiceRule{name:&'static str,reason:&'static str,prefer_trigger:bool}
const SERVICE_RULES:&[ServiceRule]=&[
 ServiceRule{name:"MapsBroker",reason:"Mapas baixados; pode operar sob demanda.",prefer_trigger:false},ServiceRule{name:"Fax",reason:"Fax; normalmente só é necessário quando usado.",prefer_trigger:false},
 ServiceRule{name:"PhoneSvc",reason:"Telefonia/integração com telefone; uso eventual.",prefer_trigger:false},ServiceRule{name:"WalletService",reason:"Carteira digital; uso eventual.",prefer_trigger:false},
 ServiceRule{name:"RetailDemo",reason:"Modo de demonstração de varejo; não é necessário em uso comum.",prefer_trigger:false},ServiceRule{name:"WMPNetworkSvc",reason:"Compartilhamento do Windows Media Player; uso opcional.",prefer_trigger:false},
 ServiceRule{name:"XblAuthManager",reason:"Autenticação Xbox; pode iniciar sob demanda.",prefer_trigger:true},ServiceRule{name:"XblGameSave",reason:"Saves Xbox; pode operar sob demanda.",prefer_trigger:true},
 ServiceRule{name:"XboxNetApiSvc",reason:"Rede Xbox; pode iniciar sob demanda.",prefer_trigger:true},ServiceRule{name:"XboxGipSvc",reason:"Acessórios Xbox; pode iniciar sob demanda.",prefer_trigger:true},
 ServiceRule{name:"wisvc",reason:"Windows Insider; necessário apenas para recursos Insider.",prefer_trigger:false},ServiceRule{name:"SmsRouter",reason:"Roteamento SMS; opcional em PCs comuns.",prefer_trigger:false},
 ServiceRule{name:"SensorDataService",reason:"Dados de sensores; pode operar sob demanda.",prefer_trigger:true},ServiceRule{name:"SensrSvc",reason:"Monitoramento de sensores; pode operar sob demanda.",prefer_trigger:true},
 ServiceRule{name:"SensorService",reason:"Sensores; pode operar sob demanda.",prefer_trigger:true},ServiceRule{name:"icssvc",reason:"Hotspot móvel; pode iniciar quando usado.",prefer_trigger:true},
 ServiceRule{name:"SharedAccess",reason:"Compartilhamento de Internet; pode operar sob demanda.",prefer_trigger:true},ServiceRule{name:"lfsvc",reason:"Geolocalização; pode iniciar quando solicitada.",prefer_trigger:true},
 ServiceRule{name:"SEMgrSvc",reason:"Pagamentos/NFC; uso eventual.",prefer_trigger:true},ServiceRule{name:"DiagTrack",reason:"Experiências conectadas/telemetria; candidato a Manual, não Desativado.",prefer_trigger:false},
];
pub fn startup_reason(n:&str,c:&str,co:&str)->Option<&'static str>{let h=format!("{n} {c} {co}").to_ascii_lowercase();if PROTECTED.iter().any(|x|h.contains(x)){return None;}STARTUP_RULES.iter().find(|r|r.tokens.iter().any(|x|h.contains(x))).map(|r|r.reason)}
pub fn service_manual_reason(n:&str,m:bool,s:&str,t:bool)->Option<&'static str>{if !m||!s.eq_ignore_ascii_case("Auto"){return None;}SERVICE_RULES.iter().find(|r|r.name.eq_ignore_ascii_case(n)&&(!r.prefer_trigger||t)).map(|r|r.reason)}
pub fn service_rule_count()->usize{SERVICE_RULES.len()} pub fn startup_rule_count()->usize{STARTUP_RULES.len()}
