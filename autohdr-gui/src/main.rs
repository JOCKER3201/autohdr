use gtk4 as gtk;
use gtk::prelude::*;
use serde::{Serialize, Deserialize};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::cell::RefCell;

#[derive(Deserialize)]
struct KScreenDoctorOutput { outputs: Vec<KScreenOutput> }
#[derive(Deserialize)]
struct KScreenOutput { 
    name: String, 
    primary: bool, 
    #[serde(rename = "maxBrightnessOverride")] max_brightness_override: Option<f32>, 
    #[serde(rename = "maxBrightness")] max_brightness: Option<f32>,
    #[serde(rename = "sdrBrightness")] sdr_brightness: Option<f32> 
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum OutputFormat {
    #[serde(rename = "pq")] PQ,
    #[serde(rename = "scrgb")] ScRGB,
}

impl Default for OutputFormat {
    fn default() -> Self { OutputFormat::PQ }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
struct HdrConfig {
    pub max_lum: f32,
    pub mid_lum: f32,
    pub sat: f32,
    pub vibrance: f32,
    pub intensity: f32,
    pub toe: f32,
    pub rcas_strength: f32,
    pub fxaa_strength: f32,
    pub sdr_brightness: f32,
    pub preferred_format: OutputFormat,
}

impl HdrConfig {
    fn get_edid_luminance(connector: &str) -> (Option<f32>, Option<f32>) {
        let drm_dir = "/sys/class/drm";
        if let Ok(entries) = std::fs::read_dir(drm_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.contains(connector) && name.contains("card") {
                    let edid_path = entry.path().join("edid");
                    if let Ok(edid) = std::fs::read(edid_path) {
                        if edid.len() >= 128 {
                            let extensions = edid[126] as usize;
                            for ext in 1..=extensions {
                                let block_start = ext * 128;
                                if edid.len() < block_start + 128 { break; }
                                let block = &edid[block_start..block_start + 128];
                                if block[0] == 0x02 { // CEA-861 Extension
                                    let d_start = block[2] as usize;
                                    let mut i = 4;
                                    while i < d_start && i < 127 {
                                        let tag = (block[i] & 0xE0) >> 5;
                                        let len = (block[i] & 0x1F) as usize;
                                        if tag == 0x07 && len >= 3 {
                                            if block[i+1] == 0x06 {
                                                let mut max_lum = None;
                                                let mut avg_lum = None;
                                                if len >= 4 {
                                                    let v = block[i+4]; 
                                                    if v > 0 { max_lum = Some(50.0 * (v as f32 / 32.0).exp2()); }
                                                }
                                                if len >= 5 {
                                                    let v = block[i+5];
                                                    if v > 0 { avg_lum = Some(50.0 * (v as f32 / 32.0).exp2()); }
                                                }
                                                return (max_lum, avg_lum);
                                            }
                                        }
                                        i += 1 + len;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        (None, None)
    }

    fn detect_system_config() -> (Option<f32>, Option<f32>) {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
        let mut connector_name = String::new();
        let mut sys_max_lum = None;
        let mut sys_sdr_brightness = None;

        if desktop.contains("kde") {
            if let Ok(output) = std::process::Command::new("kscreen-doctor").arg("-j").output() {
                if let Ok(data) = serde_json::from_slice::<KScreenDoctorOutput>(&output.stdout) {
                    if let Some(primary) = data.outputs.into_iter().find(|o| o.primary) {
                        connector_name = primary.name;
                        sys_max_lum = primary.max_brightness_override.or(primary.max_brightness);
                        sys_sdr_brightness = primary.sdr_brightness;
                    }
                }
            }
        } else if desktop.contains("gnome") {
            let home = std::env::var("HOME").unwrap_or_default();
            let path = format!("{}/.config/monitors.xml", home);
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Some(pos) = content.find("<primary>yes</primary>") {
                    let start = content[..pos].rfind("<logicalmonitor>").unwrap_or(0);
                    let end = content[pos..].find("</logicalmonitor>").map(|e| pos + e).unwrap_or(content.len());
                    let block = &content[start..end];
                    if let Some(c_start) = block.find("<connector>") {
                        if let Some(c_end) = block[c_start..].find("</connector>") {
                            connector_name = block[c_start + 11..c_start + c_end].to_string();
                        }
                    }
                }
            }
        }

        if !connector_name.is_empty() {
            let (edid_max, edid_avg) = Self::get_edid_luminance(&connector_name);
            return (sys_max_lum.or(edid_max), sys_sdr_brightness.or(edid_avg));
        }
        (None, None)
    }
}

impl Default for HdrConfig {
    fn default() -> Self {
        let (sys_max, sys_mid) = Self::detect_system_config();
        let max_lum = sys_max.unwrap_or(1000.0);
        let mid_lum = sys_mid.unwrap_or(max_lum * 0.3);

        Self {
            max_lum,
            mid_lum,
            sat: 1.0,
            vibrance: 0.0,
            intensity: 1.0,
            toe: 0.0,
            rcas_strength: 0.0,
            fxaa_strength: 0.0,
            sdr_brightness: 200.0,
            preferred_format: OutputFormat::PQ,
        }
    }
}

struct AppState {
    current_config: HdrConfig,
    current_file: Option<PathBuf>,
}

fn main() -> gtk::glib::ExitCode {
    let application = gtk::Application::builder()
        .application_id("com.github.autohdr.gui")
        .build();

    application.connect_activate(build_ui);
    application.run()
}

fn build_ui(app: &gtk::Application) {
    let state = Rc::new(RefCell::new(AppState {
        current_config: HdrConfig {
            max_lum: 1000.0,
            mid_lum: 300.0,
            sat: 1.0,
            vibrance: 0.0,
            intensity: 1.0,
            toe: 0.0,
            rcas_strength: 0.0,
            fxaa_strength: 0.0,
            sdr_brightness: 200.0,
            preferred_format: OutputFormat::PQ,
        },
        current_file: None,
    }));

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("AutoHDR Configuration")
        .default_width(800)
        .default_height(600)
        .build();

    let main_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    window.set_child(Some(&main_box));

    // --- Sidebar ---
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 10);
    sidebar.set_width_request(200);
    sidebar.add_css_class("sidebar");
    
    let list_label = gtk::Label::new(Some("Configurations"));
    list_label.set_margin_top(10);
    sidebar.append(&list_label);

    let list_box = gtk::ListBox::new();
    list_box.set_selection_mode(gtk::SelectionMode::Single);
    
    let scrolled_list = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list_box)
        .vexpand(true)
        .build();
    sidebar.append(&scrolled_list);

    let refresh_btn = gtk::Button::with_label("Refresh List");
    sidebar.append(&refresh_btn);

    main_box.append(&sidebar);

    // --- Main Panel ---
    let main_panel = gtk::Box::new(gtk::Orientation::Vertical, 20);
    main_panel.set_hexpand(true);
    main_panel.set_margin_start(20);
    main_panel.set_margin_end(20);
    main_panel.set_margin_top(20);
    main_panel.set_margin_bottom(20);

    let grid = gtk::Grid::new();
    grid.set_column_spacing(15);
    grid.set_row_spacing(10);
    main_panel.append(&grid);

    // UI Controls

    fn create_row(grid: &gtk::Grid, row: i32, label: &str, min: f64, max: f64, step: f64, value: f64) -> gtk::Scale {
        let label_widget = gtk::Label::new(Some(label));
        label_widget.set_halign(gtk::Align::Start);
        grid.attach(&label_widget, 0, row, 1, 1);

        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
        scale.set_value(value);
        scale.set_hexpand(true);
        scale.set_draw_value(true);
        grid.attach(&scale, 1, row, 1, 1);
        scale
    }

    let s_max_lum = create_row(&grid, 0, "Max Luminance (nits)", 100.0, 2000.0, 10.0, 1000.0);
    let s_mid_lum = create_row(&grid, 1, "Mid Luminance (nits)", 10.0, 1000.0, 5.0, 300.0);
    let s_sat = create_row(&grid, 2, "Saturation", 0.0, 2.0, 0.05, 1.0);
    let s_vib = create_row(&grid, 3, "Vibrance", 0.0, 2.0, 0.05, 0.0);
    let s_int = create_row(&grid, 4, "Intensity", 0.0, 10.0, 0.05, 1.0);
    let s_toe = create_row(&grid, 5, "Intelligent Toe", -1.0, 1.0, 0.05, 0.0);
    let s_rcas = create_row(&grid, 6, "RCAS Sharpening", 0.0, 1.0, 0.05, 0.0);
    let s_fxaa = create_row(&grid, 7, "FXAA Anti-Aliasing", 0.0, 1.0, 0.05, 0.0);
    let s_sdr = create_row(&grid, 8, "SDR Brightness (nits)", 50.0, 500.0, 10.0, 200.0);

    let format_label = gtk::Label::new(Some("Output Format"));
    format_label.set_halign(gtk::Align::Start);
    grid.attach(&format_label, 0, 9, 1, 1);

    let format_dropdown = gtk::DropDown::from_strings(&["PQ", "scRGB"]);
    grid.attach(&format_dropdown, 1, 9, 1, 1);

    let save_btn = gtk::Button::with_label("Save Configuration");
    save_btn.add_css_class("suggested-action");
    save_btn.set_margin_top(20);
    main_panel.append(&save_btn);

    main_box.append(&main_panel);

    // --- Interaction Logic ---

    let refresh_list = {
        let list_box = list_box.clone();
        move || {
            while let Some(child) = list_box.first_child() {
                list_box.remove(&child);
            }

            if let Some(config_dir) = dirs::config_dir().map(|p| p.join("autohdr")) {
                if let Ok(entries) = fs::read_dir(config_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|s| s.to_str()) == Some("conf") {
                            let filename = path.file_name().unwrap().to_string_lossy().to_string();
                            let row = gtk::Label::new(Some(&filename));
                            row.set_margin_start(10);
                            row.set_margin_end(10);
                            row.set_margin_top(10);
                            row.set_margin_bottom(10);
                            list_box.append(&row);
                        }
                    }
                }
            }
        }
    };

    refresh_list();

    refresh_btn.connect_clicked({
        let refresh = refresh_list.clone();
        move |_| refresh()
    });

    list_box.connect_row_selected({
        let state = state.clone();
        let s_max_lum = s_max_lum.clone();
        let s_mid_lum = s_mid_lum.clone();
        let s_sat = s_sat.clone();
        let s_vib = s_vib.clone();
        let s_int = s_int.clone();
        let s_toe = s_toe.clone();
        let s_rcas = s_rcas.clone();
        let s_fxaa = s_fxaa.clone();
        let s_sdr = s_sdr.clone();
        let format_dropdown = format_dropdown.clone();

        move |_, row| {
            if let Some(row) = row {
                let label = row.child().unwrap().downcast::<gtk::Label>().unwrap();
                let filename = label.text().to_string();
                if let Some(config_dir) = dirs::config_dir().map(|p| p.join("autohdr")) {
                    let full_path = config_dir.join(filename);
                    if let Ok(content) = fs::read_to_string(&full_path) {
                        if let Ok(config) = toml::from_str::<HdrConfig>(&content) {
                            let mut st = state.borrow_mut();
                            st.current_config = config.clone();
                            st.current_file = Some(full_path);

                            // Update UI
                            s_max_lum.set_value(config.max_lum as f64);
                            s_mid_lum.set_value(config.mid_lum as f64);
                            s_sat.set_value(config.sat as f64);
                            s_vib.set_value(config.vibrance as f64);
                            s_int.set_value(config.intensity as f64);
                            s_toe.set_value(config.toe as f64);
                            s_rcas.set_value(config.rcas_strength as f64);
                            s_fxaa.set_value(config.fxaa_strength as f64);
                            s_sdr.set_value(config.sdr_brightness as f64);
                            format_dropdown.set_selected(match config.preferred_format {
                                OutputFormat::PQ => 0,
                                OutputFormat::ScRGB => 1,
                            });
                        }
                    }
                }
            }
        }
    });

    save_btn.connect_clicked({
        let state = state.clone();
        let s_max_lum = s_max_lum.clone();
        let s_mid_lum = s_mid_lum.clone();
        let s_sat = s_sat.clone();
        let s_vib = s_vib.clone();
        let s_int = s_int.clone();
        let s_toe = s_toe.clone();
        let s_rcas = s_rcas.clone();
        let s_fxaa = s_fxaa.clone();
        let s_sdr = s_sdr.clone();
        let format_dropdown = format_dropdown.clone();

        move |_| {
            let mut st = state.borrow_mut();
            if let Some(ref path) = st.current_file {
                let content = fs::read_to_string(path).unwrap_or_default();
                let mut doc = content.parse::<toml_edit::DocumentMut>().unwrap_or_default();

                let settings = [
                    ("max_lum", s_max_lum.value() as f32 as f64),
                    ("mid_lum", s_mid_lum.value() as f32 as f64),
                    ("sat", s_sat.value() as f32 as f64),
                    ("vibrance", s_vib.value() as f32 as f64),
                    ("intensity", s_int.value() as f32 as f64),
                    ("toe", s_toe.value() as f32 as f64),
                    ("rcas_strength", s_rcas.value() as f32 as f64),
                    ("fxaa_strength", s_fxaa.value() as f32 as f64),
                    ("sdr_brightness", s_sdr.value() as f32 as f64),
                ];

                let ORDER = [
                    "max_lum", "mid_lum", "sat", "vibrance", "intensity", 
                    "toe", "rcas_strength", "fxaa_strength", 
                    "sdr_brightness", "preferred_format"
                ];

                for (key, val) in settings {
                    doc[key] = toml_edit::value(val);
                }
                doc["preferred_format"] = toml_edit::value(if format_dropdown.selected() == 0 { "pq" } else { "scrgb" });

                // Ensure order for newly added keys
                let root = doc.as_table_mut();
                for i in 0..ORDER.len() {
                    let key = ORDER[i];
                    if root.contains_key(key) {
                        // If it's already there, we might want to move it to maintain relative order 
                        // only if it was JUST added (but toml_edit keeps existing ones).
                        // To keep it simple and fulfill "appear in the place they default to":
                        // we can remove and re-insert in order, but that loses comments.
                        // Instead, let's only fix order for keys that are not at the right relative spot.
                    }
                }
                
                // Better approach: create a new ordered document but copy comments from old one? 
                // No, toml_edit is better at this. Let's use the property that 
                // if we want elements to appear in order, we should insert them in order if missing.
                
                let mut final_doc = toml_edit::DocumentMut::new();
                for key in ORDER {
                    if let Some(v) = root.remove(key) {
                        final_doc[key] = v;
                    }
                }
                // Append anything else that was in the file (like user custom stuff)
                for (k, v) in root.iter() {
                    final_doc[k] = v.clone();
                }

                if let Ok(_) = fs::write(path, final_doc.to_string()) {
                    println!("Saved config to {:?}", path);
                }
                
                if let Ok(config) = toml::from_str::<HdrConfig>(&final_doc.to_string()) {
                    st.current_config = config;
                }
            }
        }
    });

    window.present();
}
