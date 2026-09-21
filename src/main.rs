#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod defrag;
mod development;
mod drivers;
mod duplicates;
mod portable_browsers;
mod residue_scan;
mod services;
mod startup;
mod winapp2;
mod windows_inventory;
mod winsxs;

use eframe::egui;
use std::{
    collections::HashMap,
    env,
    fs,
    io::BufReader,
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
    Residuos,
    Duplicados,
    Winapp2,
    Navegadores,
    Desenvolvimento,
    WinSxS,
    Discos,
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
            (Section::Residuos, "R", "Resíduos profundos"),
            (Section::Duplicados, "≡", "Arquivos duplicados"),
            (Section::Winapp2, "W", "Winapp2.ini"),
            (Section::Navegadores, "B", "Navegadores portáteis"),
            (Section::Desenvolvimento, "P", "Python / Desenvolvimento"),
            (Section::WinSxS, "▦", "WinSxS"),
            (Section::Discos, "◉", "Discos / SSD"),
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
    error: Option<String>,
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
    winapp2: winapp2::Winapp2State,
    browsers: portable_browsers::BrowserState,
    residues: residue_scan::ResidueState,
    duplicates: duplicates::DuplicateState,
    duplicate_thumbnails: HashMap<String, egui::TextureHandle>,
    development: development::DevelopmentState,
    winsxs: winsxs::WinSxsState,
    drive_target: String,
    defrag: defrag::DefragState,
    drivers: drivers::DriverState,
    inventory: windows_inventory::WindowsInventory,
    startup: startup::StartupState,
    services: services::ServiceState,
    hide_microsoft_services: bool,
    service_user_only: bool,
    task_view_windows: bool,
    show_windows_shell: bool,
    busy_label: String,
    about_background: Option<egui::TextureHandle>,
    about_creator: Option<egui::TextureHandle>,
    about_audio: Option<AboutAudio>,
    about_volume: f32,
}

impl FaxinaApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (theme_index, transparency) = load_settings();
        let root = portable_root();
        let cfg = config_dir();
        let exclusions = load_exclusions();
        let winapp2 = winapp2::Winapp2State::load(&root, &cfg);
        let browsers = portable_browsers::BrowserState::load(&cfg);
        let about_background = load_texture(&cc.egui_ctx, &root.join("assets").join("about-background.jpg"), "about-background");
        let about_creator = load_texture(&cc.egui_ctx, &root.join("assets").join("about-creator.jpg"), "about-creator");
        let mut app = Self {
            section: Section::Painel,
            theme_index: theme_index.min(themes().len() - 1),
            transparency,
            scan_items: vec![],
            scan_status: "Ainda não analisado".into(),
            total_found: 0,
            last_output: String::new(),
            exclusions,
            exclusion_input: String::new(),
            winapp2,
            browsers,
            residues: residue_scan::ResidueState::default(),
            duplicates: duplicates::DuplicateState::default(),
            duplicate_thumbnails: HashMap::new(),
            development: development::DevelopmentState::default(),
            winsxs: winsxs::WinSxsState::default(),
            drive_target: "C:".into(),
            defrag: defrag::DefragState::default(),
            drivers: drivers::DriverState::default(),
            inventory: windows_inventory::WindowsInventory::default(),
            startup: startup::StartupState::default(),
            services: services::ServiceState::default(),
            hide_microsoft_services: true,
            service_user_only: false,
            task_view_windows: false,
            show_windows_shell: false,
            busy_label: String::new(),
            about_background,
            about_creator,
            about_audio: None,
            about_volume: 0.70,
        };
        app.apply_theme(&cc.egui_ctx);
        set_window_opacity(app.transparency);
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



    fn analyze(&mut self) {
        self.scan_status = "Analisando...".into();
        let candidates = safe_candidates();
        let exclusions = self.exclusions.clone();
        let mut items = vec![];
        for (name, path) in candidates {
            if !path.exists() || is_excluded(&path, &exclusions) { continue; }
            let size = dir_size(&path, &exclusions);
            if size > 0 {
                items.push(CleanItem { name: name.into(), path, size, checked: true, error: None });
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
            item.error = None;
            let before = item.size;
            let failures = clean_directory_contents_report(&item.path, &exclusions);
            let after = dir_size(&item.path, &exclusions);
            removed = removed.saturating_add(before.saturating_sub(after));
            item.size = after;

            if after > 0 {
                item.error = Some(if failures.is_empty() {
                    "Arquivos permaneceram ou foram recriados durante a limpeza.".into()
                } else {
                    summarize_failures(&failures)
                });
            }
        }

        self.scan_items
            .retain(|item| !item.checked || item.size > 0 || item.error.is_some());
        self.total_found = self.scan_items.iter().map(|x| x.size).sum();
        let failed = self
            .scan_items
            .iter()
            .filter(|x| x.checked && x.error.is_some())
            .count();
        self.scan_status = format!(
            "Limpeza concluída • liberado {} • {} item(ns) permaneceram e continuam na lista",
            fmt_bytes(removed),
            failed
        );
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

    fn elevated_with_network_backup(&mut self, label: &str, command: &str) {
        match backup_network_config() {
            Ok(path) => {
                self.elevated_cmd(label, command);
                self.last_output.push_str(&format!(
                    "\n\nBackup preventivo da configuração de rede salvo em:\n{}",
                    path.display()
                ));
            }
            Err(e) => {
                self.last_output = format!(
                    "A ação não foi iniciada porque o backup preventivo da rede falhou.\nErro: {e}"
                );
            }
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
            metric(ui, "Winapp2.ini", format!("{} regras", self.winapp2.rules.len()), t.accent);
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
                        if let Some(error) = &item.error {
                            ui.small(
                                egui::RichText::new(format!("Não removido: {error}"))
                                    .color(themes()[self.theme_index].accent),
                            );
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.strong(fmt_bytes(item.size));
                    });
                });
                ui.separator();
            }
        });
    }


    fn page_residues(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Scanner profundo por extensão, idade e localização. Arquivos de backup (.old/.bak/.backup/.bk) e logs ficam em Revisar e nunca são marcados automaticamente.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Pasta/unidade:");
            ui.text_edit_singleline(&mut self.residues.root);
            if ui.button("Escolher pasta").clicked() {
                if let Some(path) = pick_folder_dialog() {
                    self.residues.root = path.to_string_lossy().to_string();
                }
            }
            ui.label("Idade mínima:");
            ui.add(egui::Slider::new(&mut self.residues.min_age_days, 0..=365).suffix(" dias"));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Analisar resíduos").clicked() {
                self.residues.analyze(&self.exclusions);
            }
            if ui.button("Marcar todos").clicked() {
                self.residues.mark_all();
            }
            if ui.button("Desmarcar tudo").clicked() {
                self.residues.clear();
            }
            if ui
                .add_enabled(
                    self.residues.items.iter().any(|x| x.checked),
                    egui::Button::new("Apagar selecionados"),
                )
                .clicked()
            {
                self.residues.delete_selected(&self.exclusions);
            }
        });
        ui.label(&self.residues.status);
        ui.label(
            egui::RichText::new(format!(
                "Selecionado: {}",
                fmt_bytes(self.residues.selected_size())
            ))
            .strong()
            .color(t.accent),
        );
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for item in &mut self.residues.items {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut item.checked, "");
                    ui.vertical(|ui| {
                        ui.strong(
                            item.path
                                .file_name()
                                .map(|x| x.to_string_lossy().to_string())
                                .unwrap_or_else(|| item.path.display().to_string()),
                        );
                        ui.small(item.path.display().to_string());
                        ui.small(
                            egui::RichText::new(format!(
                                "{} • {} dias • {}",
                                item.risk.label(),
                                item.age_days,
                                item.reason
                            ))
                            .color(if item.risk == residue_scan::ResidueRisk::Safe {
                                t.accent
                            } else {
                                t.muted
                            }),
                        );
                        if let Some(error) = &item.error {
                            ui.small(
                                egui::RichText::new(format!("Não removido: {error}"))
                                    .color(t.accent),
                            );
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.strong(fmt_bytes(item.size));
                    });
                });
                ui.separator();
            }
        });
    }

    fn refresh_duplicate_thumbnails(&mut self, ctx: &egui::Context) {
        self.duplicate_thumbnails.clear();
        for file in self
            .duplicates
            .groups
            .iter()
            .flat_map(|group| group.files.iter())
        {
            let Some(path) = &file.thumbnail_path else {
                continue;
            };
            let key = file.path.to_string_lossy().to_string();
            if let Some(texture) = load_texture(ctx, path, &format!("duplicate-{key}")) {
                self.duplicate_thumbnails.insert(key, texture);
            }
        }
    }

    fn page_duplicates(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = themes()[self.theme_index].clone();
        ui.label("Duplicados reais: primeiro agrupa por tamanho, depois confirma byte a byte por SHA-256. Hardlinks para o mesmo arquivo físico são ignorados.");
        ui.small("Nenhuma cópia é marcada para exclusão automaticamente. O Faxina também impede apagar todas as cópias de um mesmo grupo.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Pasta/unidade:");
            ui.text_edit_singleline(&mut self.duplicates.root);
            if ui.button("Escolher pasta").clicked() {
                if let Some(path) = pick_folder_dialog() {
                    self.duplicates.root = path.to_string_lossy().to_string();
                }
            }
        });

        let mut analyzed = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Procurar duplicados").clicked() {
                self.duplicates
                    .analyze(&self.exclusions, &portable_root(), &config_dir());
                analyzed = true;
            }
            if ui.button("Desmarcar tudo").clicked() {
                self.duplicates.clear_selection();
            }
            if ui
                .add_enabled(
                    self.duplicates
                        .groups
                        .iter()
                        .flat_map(|g| &g.files)
                        .any(|f| f.checked),
                    egui::Button::new("Apagar cópias marcadas"),
                )
                .clicked()
            {
                self.duplicates.delete_selected(&self.exclusions);
            }
        });
        if analyzed {
            self.refresh_duplicate_thumbnails(ctx);
        }

        ui.label(&self.duplicates.status);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Recuperável mantendo uma cópia: {}",
                    fmt_bytes(self.duplicates.recoverable_size())
                ))
                .strong()
                .color(t.accent),
            );
            ui.label(format!(
                "Marcado para apagar: {}",
                fmt_bytes(self.duplicates.selected_size())
            ));
        });
        ui.separator();

        let thumbnails = &self.duplicate_thumbnails;
        let mut open_file: Option<PathBuf> = None;
        let mut open_folder: Option<PathBuf> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (group_index, group) in self.duplicates.groups.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(format!("Grupo {}", group_index + 1));
                        ui.label(format!(
                            "{} arquivos • {} cada",
                            group.files.len(),
                            fmt_bytes(group.files.first().map(|x| x.size).unwrap_or(0))
                        ));
                        ui.label(
                            egui::RichText::new(format!("SHA-256 {}…", &group.hash[..12.min(group.hash.len())]))
                                .color(t.muted),
                        );
                    });
                    for file in &mut group.files {
                        ui.horizontal(|ui| {
                            if file.is_video {
                                let key = file.path.to_string_lossy().to_string();
                                if let Some(texture) = thumbnails.get(&key) {
                                    ui.add(egui::Image::new((
                                        texture.id(),
                                        egui::vec2(128.0, 72.0),
                                    )));
                                } else {
                                    ui.allocate_ui(egui::vec2(128.0, 72.0), |ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.small("Vídeo\nsem thumbnail");
                                        });
                                    });
                                }
                            }
                            ui.checkbox(&mut file.checked, "");
                            ui.vertical(|ui| {
                                ui.strong(
                                    file.path
                                        .file_name()
                                        .map(|x| x.to_string_lossy().to_string())
                                        .unwrap_or_default(),
                                );
                                ui.small(file.path.display().to_string());
                                ui.small(if file.is_video {
                                    "Vídeo • conteúdo confirmado por hash"
                                } else {
                                    "Conteúdo confirmado por hash"
                                });
                            });
                            if ui.button("Abrir").clicked() {
                                open_file = Some(file.path.clone());
                            }
                            if ui.button("Pasta").clicked() {
                                open_folder = Some(file.path.clone());
                            }
                        });
                        ui.separator();
                    }
                });
                ui.add_space(8.0);
            }
        });

        if let Some(path) = open_file {
            open_default(&path);
        }
        if let Some(path) = open_folder {
            open_in_folder(&path);
        }
    }

    fn page_winapp2(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Perfis e regras Winapp2 com seleção persistente. As Exclusões globais sempre prevalecem sobre qualquer regra.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Carregar outro Winapp2.ini").clicked() {
                if let Some(path) = pick_file_dialog("Arquivos INI (*.ini)|*.ini|Todos os arquivos (*.*)|*.*") {
                    self.winapp2.load_path(path, &config_dir());
                }
            }
            if ui.button("Recarregar").clicked() {
                self.winapp2.reload(&config_dir());
            }
            if ui.button("Verificar atualização").clicked() {
                self.winapp2.check_update(&config_dir());
            }
            if ui.button("Salvar marcações").clicked() {
                self.winapp2.save_selection(&config_dir());
            }
            if ui.button("Exportar cópia do INI").clicked() {
                if let Some(path) = save_file_dialog("winapp2.ini", "Arquivos INI (*.ini)|*.ini|Todos os arquivos (*.*)|*.*") {
                    self.winapp2.export_current(&path);
                }
            }
        });

        ui.horizontal_wrapped(|ui| {
            if ui.button("Marcar todos exceto jogos, cache, telemetria e inseguros").clicked() {
                self.winapp2.apply_safe_preset();
                self.winapp2.save_selection(&config_dir());
            }
            if ui.button("Marcar tudo").clicked() {
                self.winapp2.mark_all();
                self.winapp2.save_selection(&config_dir());
            }
            if ui.button("Desmarcar tudo").clicked() {
                self.winapp2.clear();
                self.winapp2.save_selection(&config_dir());
            }
        });

        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.winapp2.selected_count() > 0,
                    egui::Button::new("Analisar arquivos das regras marcadas"),
                )
                .clicked()
            {
                self.winapp2.analyze_selected(&self.exclusions);
            }
            if ui
                .add_enabled(
                    !self.winapp2.analysis.is_empty()
                        && self.winapp2.analysis.iter().any(|group| group.checked),
                    egui::Button::new("Limpar arquivos analisados"),
                )
                .clicked()
            {
                self.winapp2.clean_analyzed(&self.exclusions);
            }
        });

        if !self.winapp2.analysis_status.is_empty() {
            ui.label(
                egui::RichText::new(&self.winapp2.analysis_status)
                    .color(t.accent)
                    .strong(),
            );
        }
        if !self.winapp2.analysis.is_empty() {
            egui::CollapsingHeader::new(format!(
                "Resultado da análise Winapp2 • selecionado para limpeza: {}",
                fmt_bytes(self.winapp2.analyzed_size())
            ))
            .default_open(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for group in &mut self.winapp2.analysis {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut group.checked, "");
                            ui.vertical(|ui| {
                                ui.strong(&group.rule_name);
                                ui.label(format!("{} arquivos", group.file_count));
                                if !group.failures.is_empty() {
                                    ui.small(
                                        egui::RichText::new(format!(
                                            "{} falha(s): {}",
                                            group.failures.len(),
                                            group.failures.first().cloned().unwrap_or_default()
                                        ))
                                        .color(t.accent),
                                    );
                                }
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.strong(fmt_bytes(group.size));
                                },
                            );
                        });
                    }
                });
            });
        }

        ui.horizontal(|ui| {
            ui.label("Buscar:");
            ui.text_edit_singleline(&mut self.winapp2.filter);
            ui.label(
                egui::RichText::new(format!(
                    "{} visíveis • {} selecionadas / {}",
                    self.winapp2.visible_count(),
                    self.winapp2.selected_count(),
                    self.winapp2.rules.len()
                ))
                .color(t.muted),
            );
        });

        ui.label(&self.winapp2.status);
        ui.small(format!("Arquivo ativo: {}", self.winapp2.path.display()));
        ui.separator();

        let filter = self.winapp2.filter.trim().to_ascii_lowercase();
        let mut changed = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for rule in &mut self.winapp2.rules {
                if !filter.is_empty()
                    && !rule.name.to_ascii_lowercase().contains(&filter)
                    && !rule.section.to_ascii_lowercase().contains(&filter)
                    && !rule.kind.label().to_ascii_lowercase().contains(&filter)
                {
                    continue;
                }

                ui.horizontal(|ui| {
                    if ui.checkbox(&mut rule.checked, "").changed() {
                        changed = true;
                    }
                    ui.vertical(|ui| {
                        ui.strong(&rule.name);
                        let section = if rule.section.is_empty() { "Sem categoria" } else { &rule.section };
                        ui.small(
                            egui::RichText::new(format!("{} • {}", section, rule.kind.label()))
                                .color(t.muted),
                        );
                        if let Some(warning) = &rule.warning {
                            ui.small(egui::RichText::new(warning).color(t.accent));
                        }
                    });
                });
                ui.separator();
            }
        });
        if changed {
            self.winapp2.refresh_status();
            self.winapp2.save_selection(&config_dir());
        }
    }


    fn page_browsers(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Aponte para o executável do navegador portátil. O Faxina procura perfis e identifica apenas cache e telemetria dentro da árvore portátil.");
        ui.small("Favoritos, senhas, extensões, histórico e sessões não entram nesta limpeza automática.");

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Chrome:");
            ui.text_edit_singleline(&mut self.browsers.chrome);
            if ui.button("Escolher .exe").clicked() {
                if let Some(path) = pick_file_dialog("Executáveis (*.exe)|*.exe|Todos os arquivos (*.*)|*.*") {
                    self.browsers.chrome = path.to_string_lossy().to_string();
                    self.browsers.save(&config_dir());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Edge:");
            ui.text_edit_singleline(&mut self.browsers.edge);
            if ui.button("Escolher .exe").clicked() {
                if let Some(path) = pick_file_dialog("Executáveis (*.exe)|*.exe|Todos os arquivos (*.*)|*.*") {
                    self.browsers.edge = path.to_string_lossy().to_string();
                    self.browsers.save(&config_dir());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Firefox:");
            ui.text_edit_singleline(&mut self.browsers.firefox);
            if ui.button("Escolher .exe").clicked() {
                if let Some(path) = pick_file_dialog("Executáveis (*.exe)|*.exe|Todos os arquivos (*.*)|*.*") {
                    self.browsers.firefox = path.to_string_lossy().to_string();
                    self.browsers.save(&config_dir());
                }
            }
        });

        ui.horizontal_wrapped(|ui| {
            if ui.button("Salvar caminhos").clicked() {
                self.browsers.save(&config_dir());
                self.browsers.status = "Caminhos salvos.".into();
            }
            if ui.button("Analisar cache e telemetria").clicked() {
                self.browsers.save(&config_dir());
                self.browsers.analyze(&self.exclusions);
            }
            if ui
                .add_enabled(
                    !self.browsers.items.is_empty(),
                    egui::Button::new("Limpar selecionados"),
                )
                .clicked()
            {
                self.browsers.clean_selected(&self.exclusions);
            }
        });

        ui.label(&self.browsers.status);
        if !self.browsers.items.is_empty() {
            ui.label(
                egui::RichText::new(format!(
                    "Total detectado: {}",
                    fmt_bytes(self.browsers.total_size())
                ))
                .size(18.0)
                .strong()
                .color(t.accent),
            );
        }

        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for item in &mut self.browsers.items {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut item.checked, "");
                    ui.vertical(|ui| {
                        ui.strong(format!("{} • {}", item.browser.label(), item.kind.label()));
                        ui.small(item.path.display().to_string());
                        if let Some(error) = &item.error {
                            ui.small(
                                egui::RichText::new(format!("Não removido: {error}"))
                                    .color(t.accent),
                            );
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.strong(fmt_bytes(item.size));
                    });
                });
                ui.separator();
            }
        });
    }

    fn page_disks(&mut self, ui: &mut egui::Ui) {
        self.defrag.poll();
        if self.defrag.running {
            ui.ctx().request_repaint_after(defrag::sleep_repaint_hint());
        }

        let t = themes()[self.theme_index].clone();
        ui.label("Otimização por tipo de mídia com análise antes/depois, progresso e mapa proporcional de ocupação/fragmentação.");
        ui.small("O Faxina usa o mecanismo nativo do Windows. O mapa preserva proporções reais de espaço usado/livre e fragmentação, mas não representa a posição física exata de cada cluster.");

        ui.horizontal_wrapped(|ui| {
            if ui.button("Detectar unidades").clicked() {
                self.capture(
                    "Detectando unidades",
                    "powershell.exe",
                    &[
                        "-NoProfile",
                        "-Command",
                        "Get-Volume | Where-Object {$_.DriveLetter} | Select DriveLetter,FileSystemLabel,FileSystem,HealthStatus,SizeRemaining,Size | Format-Table -AutoSize | Out-String -Width 240; Get-PhysicalDisk | Select FriendlyName,MediaType,BusType,HealthStatus,Size | Format-Table -AutoSize | Out-String -Width 240"
                    ],
                );
            }
            ui.label("Unidade:");
            ui.add_enabled(
                !self.defrag.running,
                egui::TextEdit::singleline(&mut self.drive_target).desired_width(70.0),
            );
        });

        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_label("Modo")
                .selected_text(self.defrag.selected_mode().label())
                .show_ui(ui, |ui| {
                    for (index, mode) in defrag::OptimizationMode::ALL.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.defrag.selected_mode,
                            index,
                            mode.label(),
                        );
                    }
                });
            ui.label(
                egui::RichText::new(self.defrag.selected_mode().description())
                    .color(t.muted),
            );
        });

        let target = normalize_drive_target(&self.drive_target);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    target.is_some() && !self.defrag.running,
                    egui::Button::new("Analisar unidade"),
                )
                .clicked()
            {
                if let Some(drive) = target.clone() {
                    self.defrag.analyze(drive);
                }
            }

            if ui
                .add_enabled(
                    target.is_some() && !self.defrag.running,
                    egui::Button::new("Otimizar + medir antes/depois"),
                )
                .clicked()
            {
                if let Some(drive) = target.clone() {
                    let mode = self.defrag.selected_mode();
                    self.defrag.optimize(drive, mode);
                }
            }

            if ui
                .add_enabled(self.defrag.running, egui::Button::new("Parar"))
                .clicked()
            {
                self.defrag.cancel();
            }
        });

        if self.defrag.running || self.defrag.progress > 0.0 {
            ui.add(
                egui::ProgressBar::new((self.defrag.progress / 100.0).clamp(0.0, 1.0))
                    .show_percentage()
                    .text(format!(
                        "{} • {:.0}%",
                        self.defrag.phase, self.defrag.progress
                    )),
            );
        }
        ui.label(&self.defrag.status);

        if let Some(snapshot) = self.defrag.current_snapshot() {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!(
                    "{} {}",
                    snapshot.info.drive,
                    if snapshot.info.label.is_empty() {
                        ""
                    } else {
                        &snapshot.info.label
                    }
                ));
                ui.label(format!(
                    "{} • {} • {} • {}",
                    snapshot.info.file_system,
                    snapshot.info.media_type,
                    snapshot.info.bus_type,
                    snapshot.info.health
                ));
                ui.label(format!(
                    "Livre: {} / {}",
                    defrag::format_bytes(snapshot.info.free),
                    defrag::format_bytes(snapshot.info.size)
                ));
            });

            ui.add_space(8.0);
            ui.strong("Mapa proporcional");
            draw_fragmentation_map(ui, snapshot, t.accent);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("■ Fragmentado").color(egui::Color32::from_rgb(220, 55, 55)));
                ui.label(egui::RichText::new("■ Alocado").color(t.accent));
                ui.label("Espaço livre = sem bloco");
            });
        }

        if let Some(before) = &self.defrag.before {
            ui.separator();
            ui.strong("Resultado real antes/depois");
            let after = self.defrag.after.as_ref();

            egui::Grid::new("disk_before_after")
                .num_columns(3)
                .spacing([18.0, 6.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Métrica");
                    ui.strong("Antes");
                    ui.strong("Depois");
                    ui.end_row();

                    ui.label("Fragmentação");
                    ui.label(format_optional_percent(before.fragmentation_percent));
                    ui.label(after.map(|x| format_optional_percent(x.fragmentation_percent)).unwrap_or_else(|| "—".into()));
                    ui.end_row();

                    ui.label("Arquivos fragmentados");
                    ui.label(format_optional_u64(before.fragmented_files));
                    ui.label(after.map(|x| format_optional_u64(x.fragmented_files)).unwrap_or_else(|| "—".into()));
                    ui.end_row();

                    ui.label("Espaço livre");
                    ui.label(defrag::format_bytes(before.info.free));
                    ui.label(after.map(|x| defrag::format_bytes(x.info.free)).unwrap_or_else(|| "—".into()));
                    ui.end_row();

                    ui.label("Leitura sequencial WinSAT");
                    ui.label(before.sequential_read_mbps.map(|x| format!("{x:.1} MB/s")).unwrap_or_else(|| "indisponível".into()));
                    ui.label(after.and_then(|x| x.sequential_read_mbps).map(|x| format!("{x:.1} MB/s")).unwrap_or_else(|| "—".into()));
                    ui.end_row();

                    ui.label("Leitura aleatória WinSAT");
                    ui.label(before.random_read_mbps.map(|x| format!("{x:.1} MB/s")).unwrap_or_else(|| "indisponível".into()));
                    ui.label(after.and_then(|x| x.random_read_mbps).map(|x| format!("{x:.1} MB/s")).unwrap_or_else(|| "—".into()));
                    ui.end_row();
                });

            if let Some(after) = after {
                ui.small(format!(
                    "Variação medida: sequencial {} • aleatória {}",
                    defrag::performance_delta(
                        before.sequential_read_mbps,
                        after.sequential_read_mbps
                    ),
                    defrag::performance_delta(
                        before.random_read_mbps,
                        after.random_read_mbps
                    )
                ));
                ui.small("WinSAT mede leitura no momento do teste; diferenças pequenas podem refletir cache, temperatura, carga do sistema e características do SSD/HDD.");
            }
        }

        if !self.defrag.output.trim().is_empty() {
            egui::CollapsingHeader::new("Saída detalhada do Windows")
                .show(ui, |ui| output_box(ui, &self.defrag.output));
        }

        if !self.last_output.trim().is_empty() {
            egui::CollapsingHeader::new("Inventário de unidades")
                .show(ui, |ui| output_box(ui, &self.last_output));
        }
    }


    fn page_development(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Python e ferramentas de desenvolvimento/IA — limpeza de caches regeneráveis sem apagar projetos, ambientes virtuais ou modelos.");
        ui.small("As ações abaixo preferem os comandos oficiais das próprias ferramentas. __pycache__/.pyc e outros resíduos também entram no scanner profundo quando encontrados.");

        ui.horizontal_wrapped(|ui| {
            if ui.button("Diagnosticar Python / caches").clicked() {
                self.development.diagnose();
            }
            if ui.button("Limpar cache do pip").clicked() {
                self.development.clean_pip();
            }
            if ui.button("Limpar cache do uv").clicked() {
                self.development.clean_uv();
            }
            if ui.button("Limpar caches do Poetry").clicked() {
                self.development.clean_poetry();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Analisar limpeza Conda").clicked() {
                self.development.analyze_conda();
            }
            if ui.button("Limpar caches Conda").clicked() {
                self.development.clean_conda();
            }
        });

        if !self.development.status.is_empty() {
            ui.label(egui::RichText::new(&self.development.status).color(t.accent));
        }
        output_box(ui, &self.development.output);
    }

    fn page_winsxs(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("O WinSxS é tratado somente por DISM/CBS. Nunca apagamos arquivos diretamente da pasta do Component Store.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Analisar tamanho e recuperável").clicked() {
                self.winsxs.analyze();
            }
            if ui.button("Limpar Component Store").clicked() {
                self.elevated_cmd(
                    "Limpeza WinSxS",
                    "DISM /Online /Cleanup-Image /StartComponentCleanup",
                );
            }
            if ui.button("ResetBase (irreversível)").clicked() {
                self.elevated_cmd(
                    "ResetBase",
                    "echo ATENCAO: apos ResetBase atualizacoes instaladas nao poderao ser desinstaladas. && pause && DISM /Online /Cleanup-Image /StartComponentCleanup /ResetBase",
                );
            }
        });

        if !self.winsxs.status.is_empty() {
            ui.label(egui::RichText::new(&self.winsxs.status).color(t.accent));
        }

        if self.winsxs.has_summary() {
            egui::Grid::new("winsxs_summary")
                .num_columns(2)
                .spacing([18.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Tamanho reportado pelo Explorer");
                    ui.strong(&self.winsxs.explorer_size);
                    ui.end_row();
                    ui.label("Tamanho real do Component Store");
                    ui.strong(&self.winsxs.actual_size);
                    ui.end_row();
                    ui.label("Compartilhado com o Windows");
                    ui.label(&self.winsxs.shared_size);
                    ui.end_row();
                    ui.label("Backups / recursos desativados");
                    ui.label(&self.winsxs.backups_size);
                    ui.end_row();
                    ui.label("Cache / dados temporários");
                    ui.label(&self.winsxs.cache_size);
                    ui.end_row();
                    ui.label("Pacotes recuperáveis");
                    ui.strong(&self.winsxs.reclaimable_packages);
                    ui.end_row();
                    ui.label("Windows recomenda limpeza");
                    ui.strong(&self.winsxs.recommended);
                    ui.end_row();
                });
            ui.add_space(8.0);
        }

        ui.small("Backups/cache e pacotes recuperáveis indicam o potencial de limpeza, mas não são promessa de bytes exatos por causa de hardlinks e dependências do Component Store. Analise novamente depois da limpeza para comparar.");
        output_box(ui, &self.winsxs.raw);
    }

    fn page_drivers(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Driver Store organizado por categoria. O Faxina só marca automaticamente como antigo um pacote redundante que não está em uso, não é Inbox e não é crítico de inicialização.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Analisar Driver Store").clicked() {
                self.drivers.scan();
            }
            if ui.button("Selecionar drivers antigos").clicked() {
                self.drivers.select_old();
            }
            if ui.button("Desmarcar todos").clicked() {
                self.drivers.clear_selection();
            }
            if ui
                .add_enabled(
                    self.drivers.has_selected(),
                    egui::Button::new("Remover selecionados"),
                )
                .clicked()
            {
                self.drivers.remove_selected(false);
            }
            if ui
                .add_enabled(
                    self.drivers.has_failed_selected(),
                    egui::Button::new("Forçar falhas selecionadas"),
                )
                .clicked()
            {
                self.drivers.remove_selected(true);
            }
        });

        ui.label(&self.drivers.status);
        ui.small("A remoção normal é tentada primeiro. O modo forçado só fica disponível para pacotes que falharam e continua bloqueando drivers atualmente em uso.");
        ui.separator();

        let mut current_category = String::new();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for driver in &mut self.drivers.items {
                let category = driver.category().to_string();
                if category != current_category {
                    if !current_category.is_empty() {
                        ui.add_space(8.0);
                    }
                    current_category = category.clone();
                    ui.heading(
                        egui::RichText::new(category)
                            .size(15.0)
                            .color(t.accent),
                    );
                    ui.separator();
                }

                ui.horizontal(|ui| {
                    ui.checkbox(&mut driver.checked, "");
                    ui.vertical(|ui| {
                        ui.strong(format!(
                            "{} • {}",
                            driver.published_name,
                            if driver.provider_name.is_empty() {
                                "Fornecedor não informado"
                            } else {
                                &driver.provider_name
                            }
                        ));
                        ui.small(format!(
                            "Classe: {} • Versão: {} • Data: {} • Driver Store: {}",
                            driver.class_name,
                            driver.version,
                            driver.date,
                            driver.store_date
                        ));
                        if !driver.devices.is_empty() {
                            ui.small(format!("Dispositivo(s): {}", driver.devices));
                        }
                        ui.small(
                            egui::RichText::new(format!(
                                "{} • {}",
                                driver.state_label(),
                                fmt_bytes(driver.size)
                            ))
                            .color(if driver.old_candidate { t.accent } else { t.muted }),
                        );
                        if let Some(error) = &driver.error {
                            ui.small(
                                egui::RichText::new(format!("Falha: {error}"))
                                    .color(t.accent),
                            );
                        }
                    });
                });
                ui.separator();
            }
        });

        if !self.drivers.output.trim().is_empty() {
            egui::CollapsingHeader::new("Detalhes da última operação")
                .show(ui, |ui| output_box(ui, &self.drivers.output));
        }
    }

    fn page_registry(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Scanner conservador: somente referências com alvo inexistente em Run/RunOnce e App Paths. Nenhuma limpeza genérica de 'milhares de erros'.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Analisar órfãos confirmados").clicked() {
                self.inventory.scan_registry_orphans();
            }
            if ui.button("Marcar todos encontrados").clicked() {
                for item in &mut self.inventory.registry_orphans {
                    item.checked = true;
                }
            }
            if ui.button("Desmarcar tudo").clicked() {
                for item in &mut self.inventory.registry_orphans {
                    item.checked = false;
                }
            }
            if ui.button("Abrir Editor do Registro").clicked() {
                match open_registry_editor() {
                    Ok(_) => self.last_output = "Editor do Registro aberto.".into(),
                    Err(error) => self.last_output = format!("Falha ao abrir o Editor do Registro: {error}"),
                }
            }
        });

        if ui
            .add_enabled(
                self.inventory.registry_orphans.iter().any(|x| x.checked),
                egui::Button::new("Backup + remover selecionados"),
            )
            .clicked()
        {
            let backup = portable_root()
                .join("backups")
                .join("registry")
                .join(timestamp_slug());
            match fs::create_dir_all(&backup) {
                Ok(_) => {
                    if let Some((command, manifest)) =
                        self.inventory.registry_cleanup_command(&backup)
                    {
                        let _ = fs::write(backup.join("manifesto.txt"), manifest);
                        self.elevated_cmd("Limpeza conservadora do Registro", &command);
                        self.last_output.push_str(&format!(
                            "\n\nBackup .reg e manifesto salvos em:\n{}",
                            backup.display()
                        ));
                    }
                }
                Err(error) => {
                    self.last_output =
                        format!("A limpeza não foi iniciada: falha ao criar backup: {error}");
                }
            }
        }

        ui.label(&self.inventory.registry_status);
        ui.small("Cada chave selecionada é exportada para .reg antes de qualquer remoção. Services, COM e HKLM\\SYSTEM não entram neste scanner.");
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for item in &mut self.inventory.registry_orphans {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut item.checked, "");
                    ui.vertical(|ui| {
                        ui.strong(&item.reason);
                        ui.small(&item.reg_path);
                        if !item.value_name.is_empty() {
                            ui.small(format!("Valor: {}", item.value_name));
                        }
                        ui.small(
                            egui::RichText::new(format!("Alvo inexistente: {}", item.target))
                                .color(t.muted),
                        );
                    });
                });
                ui.separator();
            }
        });
    }

    fn page_startup(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Gerenciador reversível de programas que iniciam com o Windows. Desativar não apaga a entrada original.");
        ui.small("Analisa Run/RunOnce de usuário e máquina, 32/64-bit e as pastas Inicializar. O estado é controlado por StartupApproved.");

        ui.horizontal_wrapped(|ui| {
            if ui.button("Atualizar lista").clicked() {
                self.startup.scan();
            }
            if ui.button("Marcar todos").clicked() {
                self.startup.set_all_checked(true);
            }
            if ui.button("Desmarcar todos").clicked() {
                self.startup.set_all_checked(false);
            }
            if ui
                .add_enabled(
                    self.startup.entries.iter().any(|x| x.checked),
                    egui::Button::new("Desativar selecionados"),
                )
                .clicked()
            {
                if let Some(command) = self.startup.selected_action_command(false) {
                    self.elevated_cmd("Desativar inicialização", &command);
                    self.startup.apply_local_state(false);
                }
            }
            if ui
                .add_enabled(
                    self.startup.entries.iter().any(|x| x.checked),
                    egui::Button::new("Ativar selecionados"),
                )
                .clicked()
            {
                if let Some(command) = self.startup.selected_action_command(true) {
                    self.elevated_cmd("Ativar inicialização", &command);
                    self.startup.apply_local_state(true);
                }
            }
        });

        ui.label(&self.startup.status);
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in &mut self.startup.entries {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut entry.checked, "");
                    ui.vertical(|ui| {
                        ui.strong(&entry.name);
                        ui.small(format!(
                            "{} • {}",
                            entry.location,
                            if entry.enabled { "Ativo" } else { "Desativado" }
                        ));
                        if !entry.company.is_empty() {
                            ui.small(
                                egui::RichText::new(format!("Fabricante: {}", entry.company))
                                    .color(t.muted),
                            );
                        }
                        if !entry.command.is_empty() {
                            ui.small(egui::RichText::new(&entry.command).color(t.muted));
                        }
                    });
                });
                ui.separator();
            }
        });
    }

    fn page_tasks(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Alterne entre tarefas do usuário/programas e tarefas nativas do Windows.");

        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(
                    !self.task_view_windows,
                    "Tarefas do usuário/programas",
                )
                .clicked()
            {
                self.task_view_windows = false;
                for task in &mut self.inventory.tasks {
                    task.checked = false;
                }
                if self.inventory.tasks.is_empty() {
                    self.inventory.scan_tasks();
                }
            }
            if ui
                .selectable_label(self.task_view_windows, "Tarefas do Windows")
                .clicked()
            {
                self.task_view_windows = true;
                for task in &mut self.inventory.tasks {
                    task.checked = false;
                }
                if self.inventory.tasks.is_empty() {
                    self.inventory.scan_tasks();
                }
            }
            if ui.button("Atualizar lista").clicked() {
                self.inventory.scan_tasks();
            }
        });

        let visible_count = self
            .inventory
            .tasks
            .iter()
            .filter(|task| task.is_windows == self.task_view_windows)
            .count();

        ui.horizontal_wrapped(|ui| {
            if !self.task_view_windows && ui.button("Marcar todas").clicked() {
                for task in &mut self.inventory.tasks {
                    task.checked = !task.is_windows;
                }
            }
            if ui.button("Desmarcar todas").clicked() {
                for task in &mut self.inventory.tasks {
                    task.checked = false;
                }
            }

            let selected_visible = self
                .inventory
                .tasks
                .iter()
                .any(|task| task.checked && task.is_windows == self.task_view_windows);

            if ui
                .add_enabled(selected_visible, egui::Button::new("Desativar selecionadas"))
                .clicked()
            {
                if let Some(script) =
                    self.inventory.task_action_script(false, self.task_view_windows)
                {
                    self.last_output = run_capture(
                        "powershell.exe",
                        &["-NoProfile", "-Command", &script],
                    );
                    self.inventory.scan_tasks();
                }
            }
            if ui
                .add_enabled(selected_visible, egui::Button::new("Ativar selecionadas"))
                .clicked()
            {
                if let Some(script) =
                    self.inventory.task_action_script(true, self.task_view_windows)
                {
                    self.last_output = run_capture(
                        "powershell.exe",
                        &["-NoProfile", "-Command", &script],
                    );
                    self.inventory.scan_tasks();
                }
            }
        });

        ui.label(format!(
            "{} • {} tarefa(s) nesta aba",
            self.inventory.task_status, visible_count
        ));
        if self.task_view_windows {
            ui.small("Proteção: não existe 'Marcar todas' na aba Windows. As tarefas nativas devem ser selecionadas individualmente.");
        }
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for task in &mut self.inventory.tasks {
                if task.is_windows != self.task_view_windows {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut task.checked, "");
                    ui.vertical(|ui| {
                        ui.strong(&task.task_name);
                        ui.small(format!(
                            "{} • {} • {}{}",
                            task.task_path,
                            if task.author.is_empty() { "autor não informado" } else { &task.author },
                            task.state,
                            if task.is_windows { " • Windows/Microsoft" } else { "" }
                        ));
                        if !task.actions.is_empty() {
                            ui.small(egui::RichText::new(&task.actions).color(t.muted));
                        }
                    });
                });
                ui.separator();
            }
        });
    }

    fn page_services(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();

        ui.horizontal_wrapped(|ui| {
            ui.checkbox(
                &mut self.hide_microsoft_services,
                "Esconder serviços da Microsoft",
            );
            ui.checkbox(
                &mut self.service_user_only,
                "Somente serviços por usuário atual",
            );
            if ui.button("Atualizar serviços").clicked() {
                self.services.scan();
            }
            if ui.button("Desmarcar todos").clicked() {
                self.services.clear_selection();
            }
        });

        ui.horizontal_wrapped(|ui| {
            let has_visible_selected = self.services.items.iter().any(|service| {
                service.checked
                    && (!self.hide_microsoft_services || !service.is_microsoft)
                    && (!self.service_user_only || service.is_current_user)
            });
            if ui
                .add_enabled(has_visible_selected, egui::Button::new("Iniciar"))
                .clicked()
            {
                self.services.apply_selected("start");
            }
            if ui
                .add_enabled(has_visible_selected, egui::Button::new("Parar"))
                .clicked()
            {
                self.services.apply_selected("stop");
            }
            if ui
                .add_enabled(has_visible_selected, egui::Button::new("Automático"))
                .clicked()
            {
                self.services.apply_selected("auto");
            }
            if ui
                .add_enabled(has_visible_selected, egui::Button::new("Manual"))
                .clicked()
            {
                self.services.apply_selected("manual");
            }
            if ui
                .add_enabled(has_visible_selected, egui::Button::new("Desativar"))
                .clicked()
            {
                self.services.apply_selected("disabled");
            }
        });

        ui.label(&self.services.status);
        ui.separator();

        egui::CollapsingHeader::new("Instalar um serviço")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Nome:");
                    ui.text_edit_singleline(&mut self.services.install_name);
                    ui.label("Nome de exibição:");
                    ui.text_edit_singleline(&mut self.services.install_display);
                });
                ui.horizontal(|ui| {
                    ui.label("Executável:");
                    ui.text_edit_singleline(&mut self.services.install_exe);
                    if ui.button("Escolher .exe").clicked() {
                        if let Some(path) = pick_file_dialog(
                            "Executáveis (*.exe)|*.exe|Todos os arquivos (*.*)|*.*",
                        ) {
                            self.services.install_exe = path.to_string_lossy().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Argumentos:");
                    ui.text_edit_singleline(&mut self.services.install_args);
                    egui::ComboBox::from_label("Inicialização")
                        .selected_text(match self.services.install_start {
                            0 => "Automático",
                            1 => "Manual",
                            _ => "Desativado",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.services.install_start, 0, "Automático");
                            ui.selectable_value(&mut self.services.install_start, 1, "Manual");
                            ui.selectable_value(&mut self.services.install_start, 2, "Desativado");
                        });
                    egui::ComboBox::from_label("Conta")
                        .selected_text(match self.services.install_account {
                            1 => "LocalService",
                            2 => "NetworkService",
                            _ => "LocalSystem",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.services.install_account, 0, "LocalSystem");
                            ui.selectable_value(&mut self.services.install_account, 1, "LocalService");
                            ui.selectable_value(&mut self.services.install_account, 2, "NetworkService");
                        });
                    if ui.button("Instalar serviço").clicked() {
                        self.services.install();
                    }
                });
            });

        let removable_selected = self.services.items.iter().any(|service| {
            service.checked
                && !service.is_microsoft
                && (!self.service_user_only || service.is_current_user)
        });
        if ui
            .add_enabled(
                removable_selected,
                egui::Button::new("Backup + remover serviços selecionados"),
            )
            .clicked()
        {
            let backup = portable_root()
                .join("backups")
                .join("services")
                .join(timestamp_slug());
            self.services.remove_selected(&backup);
        }
        ui.small("A remoção exporta a chave do serviço e grava um manifesto antes do sc delete. Serviços Microsoft/Windows são protegidos contra remoção.");

        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for service in &mut self.services.items {
                if self.hide_microsoft_services && service.is_microsoft {
                    continue;
                }
                if self.service_user_only && !service.is_current_user {
                    continue;
                }

                ui.horizontal(|ui| {
                    ui.checkbox(&mut service.checked, "");
                    ui.vertical(|ui| {
                        ui.strong(format!(
                            "{} ({})",
                            service.display_name,
                            service.name
                        ));
                        ui.small(format!(
                            "{} • Inicialização: {} • Conta: {}",
                            service.state,
                            service.start_mode,
                            service.start_name
                        ));
                        if !service.company.is_empty() {
                            ui.small(format!("Fabricante: {}", service.company));
                        }
                        if !service.path_name.is_empty() {
                            ui.small(
                                egui::RichText::new(&service.path_name)
                                    .color(t.muted),
                            );
                        }
                        let mut tags = Vec::new();
                        if service.is_microsoft {
                            tags.push("Microsoft/Windows");
                        } else {
                            tags.push("Terceiro");
                        }
                        if service.is_per_user {
                            tags.push("Serviço por usuário");
                        }
                        if service.is_current_user {
                            tags.push("Usuário atual");
                        }
                        ui.small(
                            egui::RichText::new(tags.join(" • "))
                                .color(t.accent),
                        );
                        if let Some(error) = &service.error {
                            ui.small(
                                egui::RichText::new(format!("Falha: {error}"))
                                    .color(t.accent),
                            );
                        }
                    });
                });
                ui.separator();
            }
        });

        if !self.services.output.trim().is_empty() {
            egui::CollapsingHeader::new("Detalhes da última operação")
                .show(ui, |ui| output_box(ui, &self.services.output));
        }
    }

    fn page_shell(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Gerencia entradas estáticas e extensões do menu de contexto. Itens identificados como Windows/Microsoft ficam ocultos por padrão.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Atualizar itens").clicked() {
                self.inventory.scan_shell();
            }
            ui.checkbox(&mut self.show_windows_shell, "Mostrar itens do Windows");
            if ui.button("Marcar visíveis").clicked() {
                for item in &mut self.inventory.shell {
                    if self.show_windows_shell || !item.is_windows {
                        item.checked = true;
                    }
                }
            }
            if ui.button("Desmarcar todos").clicked() {
                for item in &mut self.inventory.shell {
                    item.checked = false;
                }
            }
        });

        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.inventory.shell.iter().any(|x| x.checked),
                    egui::Button::new("Desativar selecionados"),
                )
                .clicked()
            {
                if let Some(command) = self.inventory.shell_action_command(false) {
                    self.elevated_cmd("Desativar menu de contexto", &command);
                }
            }
            if ui
                .add_enabled(
                    self.inventory.shell.iter().any(|x| x.checked),
                    egui::Button::new("Ativar selecionados"),
                )
                .clicked()
            {
                if let Some(command) = self.inventory.shell_action_command(true) {
                    self.elevated_cmd("Ativar menu de contexto", &command);
                }
            }
        });
        ui.label(&self.inventory.shell_status);
        ui.small("Entradas shell usam LegacyDisable; handlers COM usam a lista reversível Shell Extensions\\Blocked.");
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for item in &mut self.inventory.shell {
                if item.is_windows && !self.show_windows_shell {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut item.checked, "");
                    ui.vertical(|ui| {
                        ui.strong(&item.name);
                        ui.small(format!(
                            "{} • {} • {}",
                            item.context,
                            item.kind,
                            if item.enabled { "Ativo" } else { "Desativado" }
                        ));
                        if !item.company.is_empty() {
                            ui.small(
                                egui::RichText::new(format!("Fabricante: {}", item.company))
                                    .color(t.muted),
                            );
                        }
                        if !item.command.is_empty() {
                            ui.small(egui::RichText::new(&item.command).color(t.muted));
                        }
                    });
                });
                ui.separator();
            }
        });
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
        ui.label("Diagnostique primeiro. Resets mais invasivos ficam separados para não alterar proxy, VPN ou configuração manual sem necessidade.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Diagnóstico completo").clicked() {
                self.busy_label = "Diagnóstico de rede".into();
                self.last_output = network_diagnostics();
                self.busy_label.clear();
            }
            if ui.button("Salvar configuração atual").clicked() {
                self.last_output = match backup_network_config() {
                    Ok(path) => format!("Backup da configuração de rede salvo em:\n{}", path.display()),
                    Err(e) => format!("Falha ao salvar backup de rede: {e}"),
                };
            }
        });

        ui.separator();
        ui.strong("Reparo normal");
        ui.label("O reparo completo NÃO redefine proxy WinHTTP e não reinicia o Windows automaticamente.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Reparo rápido").clicked() {
                self.elevated_cmd("Reparo rápido", "ipconfig /flushdns && ipconfig /registerdns");
            }
            if ui.button("Reparo completo").clicked() {
                self.elevated_with_network_backup(
                    "Reparo completo",
                    "ipconfig /flushdns && ipconfig /release && ipconfig /renew && ipconfig /registerdns && netsh winsock reset && netsh int ip reset"
                );
            }
        });

        ui.separator();
        ui.strong("Reparo avançado — ações separadas");
        ui.label("Use somente a correção necessária. Ações que redefinem a pilha de rede fazem backup preventivo antes de executar.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Limpar DNS").clicked() {
                self.elevated_cmd("Limpar DNS", "ipconfig /flushdns");
            }
            if ui.button("Registrar DNS").clicked() {
                self.elevated_cmd("Registrar DNS", "ipconfig /registerdns");
            }
            if ui.button("Renovar IP").clicked() {
                self.elevated_cmd("Renovar IP", "ipconfig /release && ipconfig /renew");
            }
            if ui.button("Limpar ARP").clicked() {
                self.elevated_cmd("Limpar ARP", "arp -d *");
            }
            if ui.button("Reset Winsock").clicked() {
                self.elevated_with_network_backup("Reset Winsock", "netsh winsock reset");
            }
            if ui.button("Reset TCP/IP").clicked() {
                self.elevated_with_network_backup("Reset TCP/IP", "netsh int ip reset");
            }
            if ui.button("Restaurar proxy WinHTTP").clicked() {
                self.elevated_with_network_backup("Restaurar proxy WinHTTP", "netsh winhttp reset proxy");
            }
            if ui.button("Reiniciar adaptadores físicos").clicked() {
                self.elevated_cmd(
                    "Reiniciar adaptadores",
                    "powershell -NoProfile -Command \"Get-NetAdapter | Where-Object {$_.HardwareInterface -and $_.Status -ne 'Disabled'} | Restart-NetAdapter -Confirm:$false\""
                );
            }
        });

        ui.small("Winsock/TCP-IP podem exigir reinicialização para concluir. O Apocalipse Faxina apenas informa; nunca reinicia sem confirmação do usuário.");
        output_box(ui, &self.last_output);
    }

    fn page_exclusions(&mut self, ui: &mut egui::Ui) {
        let t = themes()[self.theme_index].clone();
        ui.label("Exclusões globais — prioridade máxima sobre Limpeza, Winapp2.ini, navegadores e qualquer outro motor.");
        ui.small("A ordem é: exclusão global → proteção do sistema → regra → seleção do usuário → limpeza.");

        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.exclusion_input);
            if ui.button("Adicionar caminho").clicked() {
                let value = self.exclusion_input.trim().to_string();
                if !value.is_empty() && !contains_path(&self.exclusions, &value) {
                    self.exclusions.push(value);
                    self.exclusion_input.clear();
                    save_exclusions(&self.exclusions);
                }
            }
            if ui.button("Escolher arquivo").clicked() {
                if let Some(path) = pick_file_dialog("Todos os arquivos (*.*)|*.*") {
                    let value = path.to_string_lossy().to_string();
                    if !contains_path(&self.exclusions, &value) {
                        self.exclusions.push(value);
                        save_exclusions(&self.exclusions);
                    }
                }
            }
            if ui.button("Escolher pasta").clicked() {
                if let Some(path) = pick_folder_dialog() {
                    let value = path.to_string_lossy().to_string();
                    if !contains_path(&self.exclusions, &value) {
                        self.exclusions.push(value);
                        save_exclusions(&self.exclusions);
                    }
                }
            }
        });

        ui.separator();
        let mut remove = None;
        for (index, value) in self.exclusions.iter().enumerate() {
            let built_in = is_default_exclusion(value);
            ui.horizontal(|ui| {
                ui.label("🔒");
                ui.vertical(|ui| {
                    ui.label(value);
                    if built_in {
                        ui.small(
                            egui::RichText::new("Proteção padrão do Windows / impressão")
                                .color(t.accent),
                        );
                    } else {
                        ui.small(egui::RichText::new("Exclusão adicionada pelo usuário").color(t.muted));
                    }
                });
                if !built_in && ui.button("Remover").clicked() {
                    remove = Some(index);
                }
            });
            ui.separator();
        }

        if let Some(index) = remove {
            self.exclusions.remove(index);
            self.exclusions = merge_default_exclusions(std::mem::take(&mut self.exclusions));
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

        ui.label(
            egui::RichText::new("Sobre o criador do Apocalipse Faxina.")
                .size(18.0)
                .strong(),
        );
        ui.add_space(8.0);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                let paused_or_stopped = self
                    .about_audio
                    .as_ref()
                    .map(|audio| audio.sink.is_paused() || audio.sink.empty())
                    .unwrap_or(true);

                if ui.button(if paused_or_stopped { "Tocar" } else { "Pausar" }).clicked() {
                    if let Some(audio) = &self.about_audio {
                        if audio.sink.empty() {
                            self.restart_about_audio();
                        } else if audio.sink.is_paused() {
                            audio.sink.play();
                        } else {
                            audio.sink.pause();
                        }
                    } else {
                        self.restart_about_audio();
                    }
                }

                if ui.button("Parar").clicked() {
                    self.stop_about_audio();
                }

                ui.label("Volume");
                let old_volume = self.about_volume;
                ui.add(egui::Slider::new(&mut self.about_volume, 0.0..=1.0).show_value(false));
                if (old_volume - self.about_volume).abs() > f32::EPSILON {
                    if let Some(audio) = &self.about_audio {
                        audio.sink.set_volume(self.about_volume);
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(photo) = &self.about_creator {
                        ui.add(egui::Image::new((photo.id(), egui::vec2(78.0, 78.0))));
                    }
                    ui.allocate_ui_with_layout(
                        egui::vec2(320.0, 78.0),
                        egui::Layout::top_down(egui::Align::RIGHT),
                        |ui| {
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new("Criador: Juliano - Brasil - Sátia Mortadela")
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new("Rust Core • Windows x64 • Portátil")
                                    .color(t.muted),
                            );
                        },
                    );
                });
            });
        });

        ui.add_space(6.0);
        if let Some(background) = &self.about_background {
            // O arquivo original contém um player de vídeo no topo direito.
            // A seção mostra apenas a parte limpa da arte, cortando a faixa superior.
            let available = ui.available_width().max(120.0);
            let [w, h] = background.size();
            let source_ratio = if w == 0 { 0.55 } else { h as f32 / w as f32 };
            let visible_ratio = source_ratio * 0.81;
            let height = (available * visible_ratio).min(ui.available_height().max(220.0));
            let uv = egui::Rect::from_min_max(
                egui::pos2(0.0, 0.19),
                egui::pos2(1.0, 1.0),
            );
            ui.add(
                egui::Image::new((background.id(), egui::vec2(available, height)))
                    .uv(uv),
            );
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Mídia da seção Sobre não encontrada na pasta assets.")
                        .color(t.muted),
                );
            });
        }

        if !self.last_output.is_empty() && self.last_output.contains("áudio") {
            ui.small(&self.last_output);
        }
    }

    fn stop_about_audio(&mut self) {
        if let Some(audio) = self.about_audio.take() {
            audio.sink.stop();
        }
    }

    fn restart_about_audio(&mut self) {
        self.stop_about_audio();
        let path = portable_root().join("assets").join("about-theme.mp4");
        match start_about_audio(&path, self.about_volume) {
            Ok(audio) => self.about_audio = Some(audio),
            Err(error) => self.last_output = error,
        }
    }
}

impl eframe::App for FaxinaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);
        let t = themes()[self.theme_index].clone();
        let section_before_nav = self.section;
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
                ui.label(egui::RichText::new(format!("v{} • Motor pronto", env!("CARGO_PKG_VERSION"))).color(t.muted));
            });
        });

        if self.section != section_before_nav {
            if section_before_nav == Section::Sobre {
                self.stop_about_audio();
            }
            if self.section == Section::Sobre {
                self.restart_about_audio();
            }
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.bg).inner_margin(egui::Margin::same(22))).show(ctx, |ui| {
            self.header(ui);
            match self.section {
                Section::Painel => self.page_dashboard(ui),
                Section::Limpeza => self.page_clean(ui),
                Section::Residuos => self.page_residues(ui),
                Section::Duplicados => self.page_duplicates(ui, ctx),
                Section::Winapp2 => self.page_winapp2(ui),
                Section::Navegadores => self.page_browsers(ui),
                Section::Desenvolvimento => self.page_development(ui),
                Section::WinSxS => self.page_winsxs(ui),
                Section::Discos => self.page_disks(ui),
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


struct AboutAudio {
    _stream: rodio::OutputStream,
    sink: rodio::Sink,
}

fn start_about_audio(path: &Path, volume: f32) -> Result<AboutAudio, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Falha ao abrir áudio da seção Sobre: {error}"))?;
    let (stream, handle) = rodio::OutputStream::try_default()
        .map_err(|error| format!("Falha ao iniciar saída de áudio: {error}"))?;
    let sink = rodio::Sink::try_new(&handle)
        .map_err(|error| format!("Falha ao criar player de áudio: {error}"))?;
    let source = rodio::Decoder::new(BufReader::new(file))
        .map_err(|error| format!("Falha ao decodificar áudio MP4: {error}"))?;
    sink.set_volume(volume.clamp(0.0, 1.0));
    sink.append(source);
    Ok(AboutAudio {
        _stream: stream,
        sink,
    })
}

fn load_texture(ctx: &egui::Context, path: &Path, name: &str) -> Option<egui::TextureHandle> {
    let image = image::open(path).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let pixels = image.into_raw();
    let color = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

fn normalize_drive_target(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches(['\\', '/']);
    let bytes = value.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        Some(format!("{}:", (bytes[0] as char).to_ascii_uppercase()))
    } else if bytes.len() == 1 && bytes[0].is_ascii_alphabetic() {
        Some(format!("{}:", (bytes[0] as char).to_ascii_uppercase()))
    } else {
        None
    }
}

#[cfg(windows)]
fn run_picker_script(script: &str) -> Option<PathBuf> {
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-STA", "-Command", script]);
    command.stdin(Stdio::null()).stderr(Stdio::null()).stdout(Stdio::piped());
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let hex = String::from_utf8_lossy(&output.stdout)
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    if hex.is_empty() || hex.len() % 2 != 0 {
        return None;
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let raw = hex.as_bytes();
    for index in (0..raw.len()).step_by(2) {
        let pair = std::str::from_utf8(&raw[index..index + 2]).ok()?;
        bytes.push(u8::from_str_radix(pair, 16).ok()?);
    }
    let value = String::from_utf8(bytes).ok()?;
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

#[cfg(not(windows))]
fn run_picker_script(_script: &str) -> Option<PathBuf> {
    None
}

fn pick_file_dialog(filter: &str) -> Option<PathBuf> {
    let filter = filter.replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Filter='{}'; if($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK){{$b=[Text.Encoding]::UTF8.GetBytes($d.FileName); [Console]::Write(([BitConverter]::ToString($b)).Replace('-',''))}}",
        filter
    );
    run_picker_script(&script)
}

fn save_file_dialog(file_name: &str, filter: &str) -> Option<PathBuf> {
    let file_name = file_name.replace('\'', "''");
    let filter = filter.replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.SaveFileDialog; $d.FileName='{}'; $d.Filter='{}'; if($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK){{$b=[Text.Encoding]::UTF8.GetBytes($d.FileName); [Console]::Write(([BitConverter]::ToString($b)).Replace('-',''))}}",
        file_name, filter
    );
    run_picker_script(&script)
}

fn pick_folder_dialog() -> Option<PathBuf> {
    let script = "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.FolderBrowserDialog; if($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK){$b=[Text.Encoding]::UTF8.GetBytes($d.SelectedPath); [Console]::Write(([BitConverter]::ToString($b)).Replace('-',''))}";
    run_picker_script(script)
}


fn open_registry_editor() -> Result<(), String> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-WindowStyle",
        "Hidden",
        "-Command",
        "Start-Process -FilePath 'regedit.exe' -Verb RunAs",
    ]);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn open_default(path: &Path) {
    let quoted = path.to_string_lossy().replace('\'', "''");
    let script = format!("Start-Process -LiteralPath '{}'", quoted);
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-Command", &script]);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let _ = command.spawn();
}

fn open_in_folder(path: &Path) {
    let argument = format!("/select,{}", path.display());
    let _ = Command::new("explorer.exe").arg(argument).spawn();
}


fn timestamp_slug() -> String {
    let raw = run_capture(
        "powershell.exe",
        &["-NoProfile", "-Command", "Get-Date -Format 'yyyy-MM-dd_HH-mm-ss'"],
    );
    let value: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    if value.len() >= 10 {
        value
    } else {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("backup-{seconds}")
    }
}

fn format_optional_percent(value: Option<f32>) -> String {
    value
        .map(|value| format!("{value:.1}%"))
        .unwrap_or_else(|| "não informado".into())
}

fn format_optional_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "não informado".into())
}

fn draw_fragmentation_map(
    ui: &mut egui::Ui,
    snapshot: &defrag::Snapshot,
    allocated_color: egui::Color32,
) {
    let columns = 48usize;
    let rows = 7usize;
    let count = columns * rows;
    let gap = 2.0f32;
    let width = ui.available_width().max(240.0);
    let cell = ((width - gap * (columns.saturating_sub(1) as f32)) / columns as f32)
        .clamp(3.0, 13.0);
    let height = rows as f32 * cell + gap * (rows.saturating_sub(1) as f32);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(columns as f32 * cell + gap * (columns.saturating_sub(1) as f32), height),
        egui::Sense::hover(),
    );

    let used_ratio = if snapshot.info.size > 0 {
        1.0 - (snapshot.info.free as f64 / snapshot.info.size as f64)
    } else {
        0.0
    }
    .clamp(0.0, 1.0);
    let used_cells = ((count as f64 * used_ratio).round() as usize).min(count);
    let frag_ratio = snapshot
        .fragmentation_percent
        .unwrap_or(0.0)
        .clamp(0.0, 100.0) as f64
        / 100.0;
    let fragmented_cells =
        ((used_cells as f64 * frag_ratio).round() as usize).min(used_cells);

    for index in 0..used_cells {
        let row = index / columns;
        let col = index % columns;
        let min = egui::pos2(
            rect.min.x + col as f32 * (cell + gap),
            rect.min.y + row as f32 * (cell + gap),
        );
        let cell_rect = egui::Rect::from_min_size(min, egui::vec2(cell, cell));

        // Espalha os blocos fragmentados proporcionalmente pelo espaço usado
        // para evitar sugerir uma posição física que o defrag.exe não informa.
        let red = fragmented_cells > 0
            && ((index.wrapping_mul(97).wrapping_add(31)) % used_cells.max(1))
                < fragmented_cells;
        let color = if red {
            egui::Color32::from_rgb(220, 55, 55)
        } else {
            allocated_color
        };
        ui.painter().rect_filled(cell_rect, 1.0, color);
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

fn default_exclusions() -> Vec<String> {
    let windows = env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let root = PathBuf::from(windows);
    vec![
        root.join("ServiceProfiles")
            .join("LocalService")
            .join("AppData")
            .join("Local")
            .join("Temp")
            .to_string_lossy()
            .to_string(),
        root.join("ServiceProfiles")
            .join("NetworkService")
            .join("AppData")
            .join("Local")
            .join("Temp")
            .to_string_lossy()
            .to_string(),
        root.join("SystemTemp").to_string_lossy().to_string(),
    ]
}

fn normalize_path_string(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn contains_path(values: &[String], value: &str) -> bool {
    let needle = normalize_path_string(value);
    values
        .iter()
        .any(|item| normalize_path_string(item) == needle)
}

fn merge_default_exclusions(values: Vec<String>) -> Vec<String> {
    let mut merged = default_exclusions();
    for value in values {
        if !value.trim().is_empty() && !contains_path(&merged, &value) {
            merged.push(value);
        }
    }
    merged
}

fn is_default_exclusion(value: &str) -> bool {
    contains_path(&default_exclusions(), value)
}

fn load_exclusions() -> Vec<String> {
    let saved = fs::read_to_string(config_dir().join("exclusions.txt"))
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let merged = merge_default_exclusions(saved);
    let _ = fs::write(config_dir().join("exclusions.txt"), merged.join("\n"));
    merged
}

fn save_exclusions(v: &[String]) {
    let _ = fs::write(config_dir().join("exclusions.txt"), v.join("\n"));
}

fn safe_candidates() -> Vec<(&'static str, PathBuf)> {
    let mut v = vec![];
    if let Ok(p) = env::var("TEMP") {
        v.push(("Temporários do usuário", PathBuf::from(p)));
    }
    if let Ok(p) = env::var("LOCALAPPDATA") {
        let local = PathBuf::from(p);
        v.push(("CrashDumps", local.join("CrashDumps")));
        v.push(("DirectX Shader Cache", local.join("D3DSCache")));
        v.push(("NVIDIA DXCache", local.join("NVIDIA").join("DXCache")));
        v.push(("NVIDIA GLCache", local.join("NVIDIA").join("GLCache")));
        v.push(("pip cache", local.join("pip").join("Cache")));
        v.push(("Poetry cache", local.join("pypoetry").join("Cache")));
        v.push(("uv cache", local.join("uv").join("cache")));
    }
    if let Ok(p) = env::var("USERPROFILE") {
        let user = PathBuf::from(p);
        v.push(("pip cache (.cache)", user.join(".cache").join("pip")));
        v.push(("Poetry cache (.cache)", user.join(".cache").join("pypoetry")));
        v.push(("uv cache (.cache)", user.join(".cache").join("uv")));
    }
    if let Ok(p) = env::var("WINDIR") {
        v.push(("Windows Temp", PathBuf::from(p).join("Temp")));
    }
    v
}

fn is_excluded(path: &Path, exclusions: &[String]) -> bool {
    let path = normalize_path_string(&path.to_string_lossy());
    exclusions.iter().any(|entry| {
        let entry = normalize_path_string(entry);
        !entry.is_empty()
            && (path == entry
                || path
                    .strip_prefix(&entry)
                    .is_some_and(|rest| rest.starts_with('\\')))
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

fn clean_directory_contents_report(path: &Path, exclusions: &[String]) -> Vec<String> {
    fn visit(path: &Path, exclusions: &[String], failures: &mut Vec<String>) {
        if is_excluded(path, exclusions) {
            return;
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                failures.push(format!("{}: {}", path.display(), error));
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    failures.push(format!("Falha ao ler item: {error}"));
                    continue;
                }
            };
            let child = entry.path();
            if is_excluded(&child, exclusions) {
                continue;
            }
            let meta = match fs::symlink_metadata(&child) {
                Ok(meta) => meta,
                Err(error) => {
                    failures.push(format!("{}: {}", child.display(), error));
                    continue;
                }
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                visit(&child, exclusions, failures);
                match fs::read_dir(&child) {
                    Ok(mut left) => {
                        if left.next().is_none() {
                            if let Err(error) = fs::remove_dir(&child) {
                                failures.push(format!("{}: {}", child.display(), error));
                            }
                        }
                    }
                    Err(error) => failures.push(format!("{}: {}", child.display(), error)),
                }
            } else if let Err(error) = fs::remove_file(&child) {
                failures.push(format!("{}: {}", child.display(), error));
            }
        }
    }

    let mut failures = Vec::new();
    visit(path, exclusions, &mut failures);
    failures
}

fn summarize_failures(failures: &[String]) -> String {
    if failures.is_empty() {
        return String::new();
    }
    if failures.len() == 1 {
        failures[0].clone()
    } else {
        format!("{} falhas; primeira: {}", failures.len(), failures[0])
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

fn append_report_section(out: &mut String, title: &str, body: String) {
    out.push_str("\n========================================\n");
    out.push_str(title);
    out.push_str("\n========================================\n");
    out.push_str(body.trim());
    out.push('\n');
}

fn network_diagnostics() -> String {
    let mut out = String::from("DIAGNÓSTICO DE REDE — APOCALIPSE FAXINA\n");
    append_report_section(&mut out, "CONFIGURAÇÃO IP / DNS / GATEWAY", run_capture("ipconfig.exe", &["/all"]));
    append_report_section(&mut out, "ROTAS IPv4", run_capture("route.exe", &["print", "-4"]));
    append_report_section(&mut out, "RESOLUÇÃO DE NOMES", run_capture("nslookup.exe", &["example.com"]));
    append_report_section(&mut out, "CONECTIVIDADE IP (ICMP PODE SER BLOQUEADO)", run_capture("ping.exe", &["1.1.1.1", "-n", "2"]));
    append_report_section(&mut out, "PROXY WINHTTP", run_capture("netsh.exe", &["winhttp", "show", "proxy"]));

    let ps = r#"
$ErrorActionPreference='SilentlyContinue'
'--- Adaptadores ---'
Get-NetAdapter | Select-Object Name,InterfaceDescription,Status,LinkSpeed,MacAddress | Format-Table -AutoSize | Out-String -Width 240
'--- DNS configurado ---'
Get-DnsClientServerAddress | Where-Object {$_.ServerAddresses.Count -gt 0} | Select-Object InterfaceAlias,AddressFamily,@{N='ServidoresDNS';E={$_.ServerAddresses -join ', '}} | Format-Table -AutoSize | Out-String -Width 240
'--- Gateway padrão ---'
Get-NetRoute -DestinationPrefix '0.0.0.0/0' | Sort-Object RouteMetric | Select-Object -First 5 InterfaceAlias,NextHop,RouteMetric | Format-Table -AutoSize | Out-String -Width 240
'--- Teste HTTPS ---'
Test-NetConnection -ComputerName 'www.microsoft.com' -Port 443 -InformationLevel Detailed | Select-Object ComputerName,RemoteAddress,RemotePort,TcpTestSucceeded | Format-List | Out-String -Width 240
'--- Proxy do usuário ---'
Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' | Select-Object ProxyEnable,ProxyServer,AutoConfigURL | Format-List | Out-String -Width 240
'--- VPNs ---'
Get-VpnConnection | Select-Object Name,ServerAddress,TunnelType,ConnectionStatus | Format-Table -AutoSize | Out-String -Width 240
"#;
    append_report_section(&mut out, "ADAPTADORES / DNS / GATEWAY / HTTPS / PROXY / VPN", run_capture("powershell.exe", &["-NoProfile", "-Command", ps]));
    out
}

fn backup_network_config() -> std::io::Result<PathBuf> {
    let raw_stamp = run_capture(
        "powershell.exe",
        &["-NoProfile", "-Command", "Get-Date -Format 'yyyy-MM-dd_HH-mm-ss'"],
    );
    let mut stamp: String = raw_stamp
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();

    if stamp.len() < 10 {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        stamp = format!("rede-{secs}");
    }

    let dir = portable_root()
        .join("backups")
        .join("network")
        .join(stamp);
    fs::create_dir_all(&dir)?;

    fs::write(dir.join("ipconfig-all.txt"), run_capture("ipconfig.exe", &["/all"]))?;
    fs::write(dir.join("routes-ipv4.txt"), run_capture("route.exe", &["print", "-4"]))?;
    fs::write(
        dir.join("proxy-winhttp.txt"),
        run_capture("netsh.exe", &["winhttp", "show", "proxy"]),
    )?;

    let ps = r#"
$ErrorActionPreference='SilentlyContinue'
'--- IP ---'
Get-NetIPConfiguration | Format-List * | Out-String -Width 300
'--- DNS ---'
Get-DnsClientServerAddress | Format-Table -AutoSize | Out-String -Width 300
'--- Adaptadores ---'
Get-NetAdapter | Select-Object Name,InterfaceDescription,Status,MacAddress,LinkSpeed | Format-Table -AutoSize | Out-String -Width 300
'--- Proxy do usuário ---'
Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' | Select-Object ProxyEnable,ProxyServer,AutoConfigURL | Format-List | Out-String -Width 300
'--- VPNs ---'
Get-VpnConnection | Format-List * | Out-String -Width 300
"#;
    fs::write(
        dir.join("configuracao-powershell.txt"),
        run_capture("powershell.exe", &["-NoProfile", "-Command", ps]),
    )?;

    fs::write(
        dir.join("manifesto.txt"),
        "Backup preventivo criado pelo Apocalipse Faxina antes de reparos de rede.\nContém somente diagnóstico/configuração para consulta e recuperação manual.\n",
    )?;

    Ok(dir)
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
