#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW,
    GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Apocalipse Faxina")
            .with_inner_size([1240.0, 780.0])
            .with_min_inner_size([980.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Apocalipse Faxina",
        options,
        Box::new(|cc| Ok(Box::new(FaxinaApp::new(cc)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Painel,
    Limpeza,
    Winapp2,
    WinSxS,
    Drivers,
    Registro,
    Inicializacao,
    Tarefas,
    Servicos,
    Shell,
    ReparoWindows,
    ReparoInternet,
    Exclusoes,
    Aparencia,
    Sobre,
}

impl Section {
    fn all() -> &'static [(Section, &'static str, &'static str)] {
        &[
            (Section::Painel, "⌂", "Painel"),
            (Section::Limpeza, "✦", "Limpeza"),
            (Section::Winapp2, "W", "Winapp2.ini"),
            (Section::WinSxS, "▦", "WinSxS"),
            (Section::Drivers, "D", "Drivers"),
            (Section::Registro, "R", "Registro"),
            (Section::Inicializacao, "↗", "Inicialização"),
            (Section::Tarefas, "✓", "Tarefas agendadas"),
            (Section::Servicos, "S", "Serviços"),
            (Section::Shell, "☰", "Menu de contexto"),
            (Section::ReparoWindows, "✚", "Reparo do Windows"),
            (Section::ReparoInternet, "⌁", "Reparo da Internet"),
            (Section::Exclusoes, "⊘", "Exclusões"),
            (Section::Aparencia, "◐", "Aparência"),
            (Section::Sobre, "ⓘ", "Sobre"),
        ]
    }
    fn title(self) -> &'static str {
        Self::all().iter().find(|x| x.0 == self).map(|x| x.2).unwrap_or("")
    }
}

#[derive(Clone)]
struct Theme {
    name: &'static str,
    dark: bool,
    bg: egui::Color32,
    panel: egui::Color32,
    accent: egui::Color32,
    text: egui::Color32,
    muted: egui::Color32,
}

fn themes() -> Vec<Theme> {
    use egui::Color32 as C;
    vec![
        Theme{name:"Apocalipse",dark:true,bg:C::from_rgb(11,18,21),panel:C::from_rgb(20,30,34),accent:C::from_rgb(42,196,153),text:C::from_rgb(238,247,244),muted:C::from_rgb(154,177,170)},
        Theme{name:"Ônix",dark:true,bg:C::from_rgb(9,9,11),panel:C::from_rgb(22,22,26),accent:C::from_rgb(151,110,255),text:C::WHITE,muted:C::from_gray(165)},
        Theme{name:"Noite Azul",dark:true,bg:C::from_rgb(9,16,30),panel:C::from_rgb(16,29,50),accent:C::from_rgb(66,153,255),text:C::from_rgb(238,246,255),muted:C::from_rgb(146,173,205)},
        Theme{name:"Carvão",dark:true,bg:C::from_rgb(22,22,22),panel:C::from_rgb(34,34,34),accent:C::from_rgb(255,180,64),text:C::from_rgb(248,248,248),muted:C::from_rgb(180,180,180)},
        Theme{name:"Vinho",dark:true,bg:C::from_rgb(28,10,17),panel:C::from_rgb(47,18,29),accent:C::from_rgb(244,78,120),text:C::from_rgb(255,240,245),muted:C::from_rgb(202,151,168)},
        Theme{name:"Floresta",dark:true,bg:C::from_rgb(8,21,16),panel:C::from_rgb(16,38,29),accent:C::from_rgb(83,205,128),text:C::from_rgb(237,255,244),muted:C::from_rgb(151,191,166)},
        Theme{name:"Roxo Profundo",dark:true,bg:C::from_rgb(18,10,29),panel:C::from_rgb(34,19,52),accent:C::from_rgb(189,109,255),text:C::from_rgb(249,241,255),muted:C::from_rgb(181,155,200)},
        Theme{name:"Cobre",dark:true,bg:C::from_rgb(26,17,13),panel:C::from_rgb(45,29,21),accent:C::from_rgb(224,137,76),text:C::from_rgb(255,245,235),muted:C::from_rgb(203,173,149)},
        Theme{name:"Ciano",dark:true,bg:C::from_rgb(6,20,25),panel:C::from_rgb(12,37,44),accent:C::from_rgb(47,207,224),text:C::from_rgb(236,253,255),muted:C::from_rgb(143,193,201)},
        Theme{name:"Escarlate",dark:true,bg:C::from_rgb(24,9,9),panel:C::from_rgb(44,18,18),accent:C::from_rgb(238,72,72),text:C::from_rgb(255,241,241),muted:C::from_rgb(203,154,154)},
        Theme{name:"Neve",dark:false,bg:C::from_rgb(244,247,249),panel:C::WHITE,accent:C::from_rgb(28,126,96),text:C::from_rgb(25,34,38),muted:C::from_rgb(88,104,111)},
        Theme{name:"Pérola Azul",dark:false,bg:C::from_rgb(239,247,255),panel:C::from_rgb(250,253,255),accent:C::from_rgb(35,110,205),text:C::from_rgb(22,38,57),muted:C::from_rgb(86,110,135)},
        Theme{name:"Marfim Dourado",dark:false,bg:C::from_rgb(252,248,235),panel:C::from_rgb(255,253,246),accent:C::from_rgb(171,115,23),text:C::from_rgb(55,43,21),muted:C::from_rgb(112,94,62)},
        Theme{name:"Menta Polar",dark:false,bg:C::from_rgb(236,250,246),panel:C::from_rgb(249,255,253),accent:C::from_rgb(25,143,107),text:C::from_rgb(24,50,42),muted:C::from_rgb(76,115,102)},
        Theme{name:"Rosa de Cristal",dark:false,bg:C::from_rgb(255,242,247),panel:C::from_rgb(255,252,253),accent:C::from_rgb(190,65,111),text:C::from_rgb(65,29,43),muted:C::from_rgb(124,83,99)},
        Theme{name:"Lavanda",dark:false,bg:C::from_rgb(247,242,255),panel:C::from_rgb(253,251,255),accent:C::from_rgb(116,76,190),text:C::from_rgb(47,35,68),muted:C::from_rgb(100,84,126)},
        Theme{name:"Areia",dark:false,bg:C::from_rgb(247,243,235),panel:C::from_rgb(255,252,246),accent:C::from_rgb(157,103,54),text:C::from_rgb(55,45,34),muted:C::from_rgb(111,94,74)},
        Theme{name:"Céu",dark:false,bg:C::from_rgb(239,249,255),panel:C::from_rgb(250,254,255),accent:C::from_rgb(41,137,191),text:C::from_rgb(25,48,61),muted:C::from_rgb(80,112,128)},
        Theme{name:"Folha",dark:false,bg:C::from_rgb(244,250,238),panel:C::from_rgb(252,255,249),accent:C::from_rgb(87,143,47),text:C::from_rgb(39,56,29),muted:C::from_rgb(91,113,75)},
        Theme{name:"Cinza Elegante",dark:false,bg:C::from_rgb(242,243,245),panel:C::from_rgb(252,252,253),accent:C::from_rgb(78,91,112),text:C::from_rgb(33,38,47),muted:C::from_rgb(95,103,117)},
    ]
}

#[derive(Clone)]
struct CleanItem {
    name: String,
    path: PathBuf,
    size: u64,
    checked: bool,
}

struct FaxinaApp {
    section: Section,
    theme_index: usize,
    transparency: u8,
    scan_items: Vec<CleanItem>,
    scan_status: String,
    total_found: u64,
    last_output: String,
    exclusions: Vec<String>,
    exclusion_input: String,
    winapp2: Vec<(String, bool)>,
    winapp2_status: String,
    driver_inf: String,
    task_name: String,
    service_name: String,
    hide_microsoft_services: bool,
    task_filter: usize,
    busy_label: String,
}

impl FaxinaApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (theme_index, transparency) = load_settings();
        let mut app = Self {
            section: Section::Painel,
            theme_index: theme_index.min(themes().len() - 1),
            transparency,
            scan_items: vec![],
            scan_status: "Ainda não analisado".into(),
            total_found: 0,
            last_output: String::new(),
            exclusions: load_exclusions(),
            exclusion_input: String::new(),
            winapp2: vec![],
            winapp2_status: "Winapp2.ini ainda não carregado".into(),
            driver_inf: String::new(),
            task_name: String::new(),
            service_name: String::new(),
            hide_microsoft_services: true,
            task_filter: 0,
            busy_label: String::new(),
        };
        app.apply_theme(&cc.egui_ctx);
        set_window_opacity(app.transparency);
        app.load_winapp2();
        app
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let t = &themes()[self.theme_index];
        let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = t.bg;
        v.window_fill = t.panel;
        v.extreme_bg_color = t.panel;
        v.override_text_color = Some(t.text);
        v.selection.bg_fill = t.accent;
        v.selection.stroke.color = t.text;
        v.hyperlink_color = t.accent;
        v.widgets.inactive.bg_fill = t.panel;
        v.widgets.hovered.bg_fill = blend(t.panel, t.accent, 0.18);
        v.widgets.active.bg_fill = blend(t.panel, t.accent, 0.28);
        v.widgets.noninteractive.fg_stroke.color = t.text;
        v.widgets.inactive.fg_stroke.color = t.text;
        v.widgets.hovered.fg_stroke.color = t.text;
        ctx.set_visuals(v);
    }

    fn load_winapp2(&mut self) {
        let path = portable_root().join("data").join("winapp2.ini");
        self.winapp2.clear();
        match fs::read_to_string(&path) {
            Ok(s) => {
                for line in s.lines() {
                    let l = line.trim();
                    if l.starts_with('[') && l.ends_with(']') && l.len() > 2 {
                        self.winapp2.push((l[1..l.len()-1].to_string(), false));
                    }
                }
                self.winapp2_status = format!("{} regras carregadas de {}", self.winapp2.len(), path.display());
            }
            Err(_) => {
                self.winapp2_status = format!("Coloque winapp2.ini em {}", path.display());
            }
        }
    }

    fn analyze(&mut self) {
        self.scan_status = "Analisando...".into();
        let candidates = safe_candidates();
        let exclusions = self.exclusions.clone();
        let mut items = vec![];
        for (name, path) in candidates {
            if !path.exists() || is_excluded(&path, &exclusions) { continue; }
            let size = dir_size(&path, &exclusions);
            if size > 0 {
                items.push(CleanItem { name: name.into(), path, size, checked: true });
            }
        }
        self.total_found = items.iter().map(|x| x.size).sum();
        self.scan_status = format!("Análise concluída: {} recuperáveis", fmt_bytes(self.total_found));
        self.scan_items = items;
    }

    fn clean_selected(&mut self) {
        let mut removed = 0u64;
        let exclusions = self.exclusions.clone();
        for item in self.scan_items.iter_mut().filter(|x| x.checked) {
            let before = item.size;
            clean_directory_contents(&item.path, &exclusions);
            let after = dir_size(&item.path, &exclusions);
            removed = removed.saturating_add(before.saturating_sub(after));
            item.size = after;
        }
        self.total_found = self.scan_items.iter().filter(|x| x.checked).map(|x| x.size).sum();
        self.scan_status = format!("Limpeza concluída. Espaço efetivamente liberado: {}", fmt_bytes(removed));
    }

    fn capture(&mut self, label: &str, program: &str, args: &[&str]) {
        self.busy_label = label.into();
        self.last_output = run_capture(program, args);
        self.busy_label.clear();
    }

    fn elevated_cmd(&mut self, label: &str, command: &str) {
        match spawn_elevated_cmd(command) {
            Ok(_) => self.last_output = format!("{label}\nComando iniciado com elevação administrativa.\nAcompanhe a janela de comando aberta pelo Windows."),
            Err(e) => self.last_output = format!("Falha ao iniciar: {e}"),
        }
    }

    fn header(&self, ui: &mut egui::Ui) {
        let t = &themes()[self.theme_index];
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("APOCALIPSE FAXINA").size(12.0).color(t.accent).strong());
                ui.heading(self.section.title());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("Rust • Windows x64 • Portátil").color(t.muted));
            });
        });
        ui.separator();
    }

    fn draw_trash(ui: &mut egui::Ui, size: f32, accent: egui::Color32) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
        let p = ui.painter();
        let c = rect.center();
        let w = size * 0.42;
        let h = size * 0.48;
        let body = egui::Rect::from_center_size(c + egui::vec2(0.0, size*0.08), egui::vec2(w, h));
        p.rect_filled(body, 8.0, accent);
        p.rect_stroke(body, 8.0, egui::Stroke::new(2.0, egui::Color32::WHITE), egui::StrokeKind::Inside);
        let top_y = body.top() - size*0.08;
        p.line_segment([egui::pos2(c.x-w*0.65, top_y), egui::pos2(c.x+w*0.65, top_y)], egui::Stroke::new(size*0.045, accent));
        p.line_segment([egui::pos2(c.x-w*0.18, top_y-size*0.08), egui::pos2(c.x+w*0.18, top_y-size*0.08)], egui::Stroke::new(size*0.045, accent));
        for f in [-0.23f32, 0.0, 0.23] {
            p.line_segment([egui::pos2(c.x+w*f, body.top()+10.0), egui::pos2(c.x+w*f, body.bottom()-10.0)], egui::Stroke::new(2.0, egui::Color32::WHITE));
        }
    }

    fn page_dashboard(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                Self::draw_trash(ui, 150.0, t.accent);
            });
            ui.add_space(22.0);
            ui.vertical(|ui| {
                ui.heading("Limpeza poderosa, organizada e com análise antes de apagar");
                ui.label(egui::RichText::new("O Apocalipse Faxina prioriza segurança, reversibilidade e transparência do que será removido.").color(t.muted));
                ui.add_space(10.0);
                if ui.button("🔎 Analisar agora").clicked() { self.analyze(); }
                ui.label(&self.scan_status);
                if self.total_found > 0 {
                    ui.label(egui::RichText::new(format!("Recuperável agora: {}", fmt_bytes(self.total_found))).size(22.0).strong().color(t.accent));
                }
            });
        });
        ui.add_space(20.0);
        egui::Grid::new("cards").num_columns(3).spacing([16.0, 14.0]).show(ui, |ui| {
            metric(ui, "Limpeza", if self.total_found > 0 { fmt_bytes(self.total_found) } else { "Não analisado".into() }, t.accent);
            metric(ui, "Winapp2.ini", format!("{} regras", self.winapp2.len()), t.accent);
            metric(ui, "Proteções", format!("{} exclusões", self.exclusions.len()), t.accent); ui.end_row();
            metric(ui, "Drivers", "normal → forçado".into(), t.accent);
            metric(ui, "Reparo", "SFC + DISM".into(), t.accent);
            metric(ui, "Aparência", "20 temas".into(), t.accent); ui.end_row();
        });
    }

    fn page_clean(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("🔎 Analisar").clicked() { self.analyze(); }
            if ui.add_enabled(!self.scan_items.is_empty(), egui::Button::new("🗑 Limpar selecionados")).clicked() {
                self.clean_selected();
            }
            ui.label(&self.scan_status);
        });
        ui.add_space(8.0);
        if self.scan_items.is_empty() {
            ui.label("Execute a análise para calcular o tamanho real dos caches seguros encontrados.");
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for item in &mut self.scan_items {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut item.checked, "");
                    ui.vertical(|ui| {
                        ui.strong(&item.name);
                        ui.small(item.path.display().to_string());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.strong(fmt_bytes(item.size));
                    });
                });
                ui.separator();
            }
        });
    }

    fn page_winapp2(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Recarregar winapp2.ini").clicked() { self.load_winapp2(); }
            if ui.button("Marcar tudo").clicked() { for x in &mut self.winapp2 { x.1 = true; } }
            if ui.button("Desmarcar tudo").clicked() { for x in &mut self.winapp2 { x.1 = false; } }
        });
        ui.label(&self.winapp2_status);
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (name, checked) in &mut self.winapp2 {
                ui.checkbox(checked, name.as_str());
            }
        });
    }

    fn page_winsxs(&mut self, ui: &mut egui::Ui) {
        ui.label("O WinSxS é tratado somente por DISM/CBS. Nunca apagamos arquivos diretamente dessa pasta.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Analisar Component Store").clicked() {
                self.capture("Analisando WinSxS", "dism.exe", &["/Online","/Cleanup-Image","/AnalyzeComponentStore"]);
            }
            if ui.button("StartComponentCleanup").clicked() {
                self.elevated_cmd("Limpeza WinSxS", "DISM /Online /Cleanup-Image /StartComponentCleanup");
            }
            if ui.button("ResetBase (irreversível)").clicked() {
                self.elevated_cmd("ResetBase", "echo ATENCAO: apos ResetBase atualizacoes instaladas nao poderao ser desinstaladas. && pause && DISM /Online /Cleanup-Image /StartComponentCleanup /ResetBase");
            }
        });
        output_box(ui, &self.last_output);
    }

    fn page_drivers(&mut self, ui: &mut egui::Ui) {
        ui.label("Fluxo padrão: identificar versões antigas → remover normalmente → usar modo forçado somente se a remoção normal falhar.");
        ui.horizontal(|ui| {
            if ui.button("Analisar Driver Store").clicked() {
                self.capture("Enumerando drivers", "pnputil.exe", &["/enum-drivers"]);
            }
            if ui.button("Ver dispositivos e drivers").clicked() {
                self.capture("Enumerando dispositivos", "pnputil.exe", &["/enum-devices","/connected","/drivers"]);
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("INF antigo:");
            ui.text_edit_singleline(&mut self.driver_inf);
            if ui.button("Remover normal").clicked() {
                let inf = self.driver_inf.trim().to_string();
                if inf.to_ascii_lowercase().starts_with("oem") && inf.to_ascii_lowercase().ends_with(".inf") {
                    self.elevated_cmd("Remoção normal", &format!("pnputil /delete-driver {}", inf));
                } else { self.last_output = "Informe um pacote no formato oemNN.inf".into(); }
            }
            if ui.button("Forçar se falhou").clicked() {
                let inf = self.driver_inf.trim().to_string();
                if inf.to_ascii_lowercase().starts_with("oem") && inf.to_ascii_lowercase().ends_with(".inf") {
                    self.elevated_cmd("Remoção forçada", &format!("echo MODO FORCADO - {} && pause && pnputil /delete-driver {} /force", inf, inf));
                } else { self.last_output = "Informe um pacote no formato oemNN.inf".into(); }
            }
        });
        output_box(ui, &self.last_output);
    }

    fn page_registry(&mut self, ui: &mut egui::Ui) {
        ui.label("Modo conservador: a primeira versão analisa pontos comuns sem apagar automaticamente chaves sensíveis.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Ver inicialização HKCU").clicked() {
                self.capture("Registro HKCU", "reg.exe", &["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run"]);
            }
            if ui.button("Ver inicialização HKLM").clicked() {
                self.capture("Registro HKLM", "reg.exe", &["query", r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run"]);
            }
            if ui.button("Abrir Editor do Registro").clicked() {
                let _ = Command::new("regedit.exe").spawn();
            }
        });
        ui.label("A limpeza automática estrutural ficará restrita a resíduos comprovados e terá backup/restauração.");
        output_box(ui, &self.last_output);
    }

    fn page_startup(&mut self, ui: &mut egui::Ui) {
        ui.label("Lista programas configurados para iniciar com o Windows.");
        if ui.button("Atualizar lista").clicked() {
            self.capture("Inicialização", "powershell.exe", &["-NoProfile","-Command","Get-CimInstance Win32_StartupCommand | Select-Object Name,Command,Location,User | Format-Table -AutoSize | Out-String -Width 240"]);
        }
        output_box(ui, &self.last_output);
    }

    fn page_tasks(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Exibir:");
            egui::ComboBox::from_id_salt("task_filter").selected_text(match self.task_filter {1=>"Criadas pelo usuário",2=>"Criadas por programas",3=>"Microsoft / Windows",_=>"Todas"}).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.task_filter,0,"Todas");
                ui.selectable_value(&mut self.task_filter,1,"Criadas pelo usuário");
                ui.selectable_value(&mut self.task_filter,2,"Criadas por programas");
                ui.selectable_value(&mut self.task_filter,3,"Microsoft / Windows");
            });
            if ui.button("Atualizar").clicked() {
                let cmd = match self.task_filter {
                    3 => "Get-ScheduledTask | Where-Object TaskPath -like '\\\\Microsoft\\\\*' | Select TaskPath,TaskName,State | Format-Table -AutoSize | Out-String -Width 240",
                    1 => "Get-ScheduledTask | Where-Object {$_.Author -and $_.TaskPath -notlike '\\\\Microsoft\\\\*'} | Select TaskPath,TaskName,Author,State | Format-Table -AutoSize | Out-String -Width 240",
                    2 => "Get-ScheduledTask | Where-Object {$_.TaskPath -notlike '\\\\Microsoft\\\\*'} | Select TaskPath,TaskName,Author,State | Format-Table -AutoSize | Out-String -Width 240",
                    _ => "Get-ScheduledTask | Select TaskPath,TaskName,Author,State | Format-Table -AutoSize | Out-String -Width 240",
                };
                self.capture("Tarefas", "powershell.exe", &["-NoProfile","-Command",cmd]);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Caminho/nome exato:");
            ui.text_edit_singleline(&mut self.task_name);
            if ui.button("Ativar").clicked() {
                let n = ps_quote(self.task_name.trim());
                self.elevated_cmd("Ativar tarefa", &format!("powershell -NoProfile -Command \"Enable-ScheduledTask -TaskName '{}'\"", n));
            }
            if ui.button("Desativar").clicked() {
                let n = ps_quote(self.task_name.trim());
                self.elevated_cmd("Desativar tarefa", &format!("powershell -NoProfile -Command \"Disable-ScheduledTask -TaskName '{}'\"", n));
            }
        });
        output_box(ui, &self.last_output);
    }

    fn page_services(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.hide_microsoft_services, "Ocultar serviços Microsoft (filtro por fabricante/caminho será refinado)");
        if ui.button("Atualizar serviços").clicked() {
            let cmd = if self.hide_microsoft_services {
                "Get-CimInstance Win32_Service | Where-Object {$_.PathName -and $_.PathName -notmatch 'Windows\\\\System32'} | Select Name,State,StartMode,DisplayName,PathName | Format-Table -AutoSize | Out-String -Width 280"
            } else {
                "Get-CimInstance Win32_Service | Select Name,State,StartMode,DisplayName,PathName | Format-Table -AutoSize | Out-String -Width 280"
            };
            self.capture("Serviços", "powershell.exe", &["-NoProfile","-Command",cmd]);
        }
        ui.horizontal(|ui| {
            ui.label("Nome do serviço:");
            ui.text_edit_singleline(&mut self.service_name);
            if ui.button("Iniciar").clicked() { self.elevated_cmd("Iniciar serviço", &format!("sc start \"{}\"", self.service_name.trim())); }
            if ui.button("Parar").clicked() { self.elevated_cmd("Parar serviço", &format!("sc stop \"{}\"", self.service_name.trim())); }
            if ui.button("Manual").clicked() { self.elevated_cmd("Serviço manual", &format!("sc config \"{}\" start= demand", self.service_name.trim())); }
            if ui.button("Desativar").clicked() { self.elevated_cmd("Desativar serviço", &format!("sc config \"{}\" start= disabled", self.service_name.trim())); }
        });
        output_box(ui, &self.last_output);
    }

    fn page_shell(&mut self, ui: &mut egui::Ui) {
        ui.label("Gerenciador do menu de contexto do Explorer. A versão inicial enumera as áreas principais antes de permitir alterações.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Itens de arquivos").clicked() { self.capture("Shell arquivos","reg.exe",&["query",r"HKCR\*\shell","/s"]); }
            if ui.button("Itens de pastas").clicked() { self.capture("Shell diretórios","reg.exe",&["query",r"HKCR\Directory\shell","/s"]); }
            if ui.button("Background da pasta").clicked() { self.capture("Shell background","reg.exe",&["query",r"HKCR\Directory\Background\shell","/s"]); }
        });
        output_box(ui, &self.last_output);
    }

    fn page_repair_windows(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("SFC /scannow").clicked() { self.elevated_cmd("SFC", "sfc /scannow"); }
            if ui.button("DISM CheckHealth").clicked() { self.elevated_cmd("DISM CheckHealth", "DISM /Online /Cleanup-Image /CheckHealth"); }
            if ui.button("DISM ScanHealth").clicked() { self.elevated_cmd("DISM ScanHealth", "DISM /Online /Cleanup-Image /ScanHealth"); }
            if ui.button("DISM RestoreHealth").clicked() { self.elevated_cmd("DISM RestoreHealth", "DISM /Online /Cleanup-Image /RestoreHealth"); }
            if ui.button("Reparação completa DISM → SFC").clicked() {
                self.elevated_cmd("Reparação completa", "DISM /Online /Cleanup-Image /RestoreHealth && sfc /scannow");
            }
        });
        output_box(ui, &self.last_output);
    }

    fn page_repair_internet(&mut self, ui: &mut egui::Ui) {
        ui.label("Os resets mais invasivos ficam separados para não apagar proxy/VPN/configuração manual sem necessidade.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Diagnóstico IP").clicked() { self.capture("IPConfig", "ipconfig.exe", &["/all"]); }
            if ui.button("Reparo rápido").clicked() { self.elevated_cmd("Reparo rápido", "ipconfig /flushdns && ipconfig /registerdns"); }
            if ui.button("Reparo completo").clicked() {
                self.elevated_cmd("Reparo completo", "ipconfig /flushdns && ipconfig /release && ipconfig /renew && ipconfig /registerdns && netsh winsock reset && netsh int ip reset");
            }
            if ui.button("Reset Winsock").clicked() { self.elevated_cmd("Winsock", "netsh winsock reset"); }
            if ui.button("Reset TCP/IP").clicked() { self.elevated_cmd("TCP/IP", "netsh int ip reset"); }
        });
        output_box(ui, &self.last_output);
    }

    fn page_exclusions(&mut self, ui: &mut egui::Ui) {
        ui.label("Tudo que estiver aqui tem prioridade sobre regras próprias e Winapp2.ini.");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.exclusion_input);
            if ui.button("Adicionar arquivo/diretório").clicked() {
                let v = self.exclusion_input.trim().to_string();
                if !v.is_empty() && !self.exclusions.contains(&v) {
                    self.exclusions.push(v);
                    self.exclusion_input.clear();
                    save_exclusions(&self.exclusions);
                }
            }
        });
        let mut remove = None;
        for (i, x) in self.exclusions.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label("🔒");
                ui.label(x);
                if ui.button("Remover").clicked() { remove = Some(i); }
            });
        }
        if let Some(i) = remove {
            self.exclusions.remove(i);
            save_exclusions(&self.exclusions);
        }
    }

    fn page_appearance(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let ts = themes();
        ui.label("20 temas com contraste de texto ajustado para cada paleta.");
        egui::Grid::new("themes_grid").num_columns(2).spacing([14.0,8.0]).show(ui, |ui| {
            for (i, theme) in ts.iter().enumerate() {
                if ui.selectable_label(self.theme_index == i, theme.name).clicked() {
                    self.theme_index = i;
                    self.apply_theme(ctx);
                    save_settings(self.theme_index, self.transparency);
                }
                ui.label(if theme.dark { "Escuro" } else { "Claro" });
                ui.end_row();
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Transparência da janela:");
            let old = self.transparency;
            ui.add(egui::Slider::new(&mut self.transparency, 0..=45).suffix("%"));
            if old != self.transparency {
                set_window_opacity(self.transparency);
                save_settings(self.theme_index, self.transparency);
            }
        });
        ui.small("0% = opaco. O limite de 45% evita perder legibilidade.");
    }

    fn page_about(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.vertical_centered(|ui| {
            Self::draw_trash(ui, 170.0, t.accent);
            ui.heading("Apocalipse Faxina");
            ui.label(egui::RichText::new("Sobre o criador do Apocalipse Faxina.").size(18.0).strong());
            ui.add_space(14.0);
            ui.label("Criador: Juliano - Brasil - Sátia Mortadela");
            ui.label(egui::RichText::new("Rust Core • Windows x64 • Portátil").color(t.muted));
            ui.add_space(10.0);
            ui.label("A seção Sobre seguirá a identidade da família Apocalipse; a transferência dos mesmos arquivos de mídia do Manager será feita sem alterar o texto acima.");
        });
    }
}

impl eframe::App for FaxinaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t = themes()[self.theme_index].clone();
        egui::SidePanel::left("nav").exact_width(235.0).frame(egui::Frame::new().fill(t.panel).inner_margin(egui::Margin::same(12))).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                Self::draw_trash(ui, 72.0, t.accent);
                ui.label(egui::RichText::new("APOCALIPSE").size(20.0).strong());
                ui.label(egui::RichText::new("FAXINA").size(12.0).color(t.accent).strong());
            });
            ui.add_space(12.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for &(section, icon, title) in Section::all() {
                    let selected = self.section == section;
                    if ui.selectable_label(selected, format!("{icon}  {title}")).clicked() {
                        self.section = section;
                    }
                }
            });
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.label(egui::RichText::new("v0.1.0 • Motor pronto").color(t.muted));
            });
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.bg).inner_margin(egui::Margin::same(22))).show(ctx, |ui| {
            self.header(ui);
            match self.section {
                Section::Painel => self.page_dashboard(ui),
                Section::Limpeza => self.page_clean(ui),
                Section::Winapp2 => self.page_winapp2(ui),
                Section::WinSxS => self.page_winsxs(ui),
                Section::Drivers => self.page_drivers(ui),
                Section::Registro => self.page_registry(ui),
                Section::Inicializacao => self.page_startup(ui),
                Section::Tarefas => self.page_tasks(ui),
                Section::Servicos => self.page_services(ui),
                Section::Shell => self.page_shell(ui),
                Section::ReparoWindows => self.page_repair_windows(ui),
                Section::ReparoInternet => self.page_repair_internet(ui),
                Section::Exclusoes => self.page_exclusions(ui),
                Section::Aparencia => self.page_appearance(ui, ctx),
                Section::Sobre => self.page_about(ui),
            }
        });
    }
}

fn metric(ui: &mut egui::Ui, title: &str, value: String, accent: egui::Color32) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(200.0);
        ui.label(title);
        ui.label(egui::RichText::new(value).size(18.0).strong().color(accent));
    });
}

fn output_box(ui: &mut egui::Ui, output: &str) {
    ui.add_space(8.0);
    egui::ScrollArea::vertical().max_height(430.0).show(ui, |ui| {
        ui.add(egui::TextEdit::multiline(&mut output.to_string()).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(18).interactive(false));
    });
}

fn blend(a: egui::Color32, b: egui::Color32, k: f32) -> egui::Color32 {
    let mix = |x:u8,y:u8| (x as f32*(1.0-k)+y as f32*k).round() as u8;
    egui::Color32::from_rgb(mix(a.r(),b.r()),mix(a.g(),b.g()),mix(a.b(),b.b()))
}

fn portable_root() -> PathBuf {
    env::current_exe().ok().and_then(|p| p.parent().map(|x| x.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

fn config_dir() -> PathBuf {
    let p = portable_root().join("config");
    let _ = fs::create_dir_all(&p);
    p
}

fn load_settings() -> (usize, u8) {
    let p = config_dir().join("appearance.txt");
    if let Ok(s) = fs::read_to_string(p) {
        let mut it = s.lines();
        let a = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let b = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        (a,b)
    } else { (0,0) }
}

fn save_settings(theme: usize, transparency: u8) {
    let _ = fs::write(config_dir().join("appearance.txt"), format!("{theme}\n{transparency}\n"));
}

fn load_exclusions() -> Vec<String> {
    fs::read_to_string(config_dir().join("exclusions.txt"))
        .map(|s| s.lines().map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

fn save_exclusions(v: &[String]) {
    let _ = fs::write(config_dir().join("exclusions.txt"), v.join("\n"));
}

fn safe_candidates() -> Vec<(&'static str, PathBuf)> {
    let mut v = vec![];
    if let Ok(p) = env::var("TEMP") { v.push(("Temporários do usuário", PathBuf::from(p))); }
    if let Ok(p) = env::var("LOCALAPPDATA") {
        let l = PathBuf::from(p);
        v.push(("CrashDumps", l.join("CrashDumps")));
        v.push(("DirectX Shader Cache", l.join("D3DSCache")));
        v.push(("NVIDIA DXCache", l.join("NVIDIA").join("DXCache")));
        v.push(("NVIDIA GLCache", l.join("NVIDIA").join("GLCache")));
        v.push(("pip cache", l.join("pip").join("Cache")));
    }
    if let Ok(p) = env::var("WINDIR") {
        v.push(("Windows Temp", PathBuf::from(p).join("Temp")));
    }
    v
}

fn is_excluded(path: &Path, exclusions: &[String]) -> bool {
    let s = path.to_string_lossy().to_ascii_lowercase();
    exclusions.iter().any(|e| {
        let e = e.trim().trim_end_matches(['\\','/']).to_ascii_lowercase();
        !e.is_empty() && s.starts_with(&e)
    })
}

fn dir_size(path: &Path, exclusions: &[String]) -> u64 {
    if is_excluded(path, exclusions) { return 0; }
    let meta = match fs::symlink_metadata(path) { Ok(m)=>m, Err(_)=>return 0 };
    if meta.file_type().is_symlink() { return 0; }
    if meta.is_file() { return meta.len(); }
    let mut total: u64 = 0;
    if let Ok(rd) = fs::read_dir(path) {
        for e in rd.flatten() { total = total.saturating_add(dir_size(&e.path(), exclusions)); }
    }
    total
}

fn clean_directory_contents(path: &Path, exclusions: &[String]) {
    if is_excluded(path, exclusions) { return; }
    let Ok(rd) = fs::read_dir(path) else { return; };
    for e in rd.flatten() {
        let p = e.path();
        if is_excluded(&p, exclusions) { continue; }
        let Ok(m) = fs::symlink_metadata(&p) else { continue; };
        if m.file_type().is_symlink() { continue; }
        if m.is_dir() { let _ = fs::remove_dir_all(&p); }
        else { let _ = fs::remove_file(&p); }
    }
}

fn fmt_bytes(n: u64) -> String {
    const K: f64 = 1024.0;
    let n = n as f64;
    if n >= K*K*K { format!("{:.2} GB", n/(K*K*K)) }
    else if n >= K*K { format!("{:.2} MB", n/(K*K)) }
    else if n >= K { format!("{:.2} KB", n/K) }
    else { format!("{} B", n as u64) }
}

fn ps_quote(s: &str) -> String { s.replace('\'', "''") }

fn run_capture(program: &str, args: &[&str]) -> String {
    let mut c = Command::new(program);
    c.args(args).stdin(Stdio::null()).stderr(Stdio::piped()).stdout(Stdio::piped());
    #[cfg(windows)]
    { c.creation_flags(CREATE_NO_WINDOW); }
    match c.output() {
        Ok(out) => {
            let mut s = String::from_utf8_lossy(&out.stdout).to_string();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() { s.push_str("\n"); s.push_str(&err); }
            if s.trim().is_empty() { format!("Comando concluído com código {:?}", out.status.code()) } else { s }
        }
        Err(e) => format!("Erro: {e}"),
    }
}

fn spawn_elevated_cmd(command: &str) -> std::io::Result<()> {
    let escaped = command.replace('\'', "''");
    let script = format!("Start-Process -FilePath 'cmd.exe' -ArgumentList '/k {}' -Verb RunAs", escaped);
    let mut c = Command::new("powershell.exe");
    c.args(["-NoProfile","-Command",&script]);
    #[cfg(windows)]
    { c.creation_flags(CREATE_NO_WINDOW); }
    c.spawn()?.wait()?;
    Ok(())
}

#[cfg(windows)]
fn set_window_opacity(transparency: u8) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() { return; }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED as i32);
        let alpha = ((100u16.saturating_sub(transparency as u16)) * 255 / 100) as u8;
        let _ = SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
    }
}

#[cfg(not(windows))]
fn set_window_opacity(_transparency: u8) {}
