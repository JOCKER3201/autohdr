use gtk4 as gtk;
use gtk::prelude::*;
use serde::{Serialize, Deserialize};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::cell::RefCell;

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum OutputFormat {
    #[serde(rename = "pq")] PQ,
    #[serde(rename = "scrgb")] ScRGB,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct HdrConfig {
    pub max_lum: f32,
    pub mid_lum: f32,
    pub sat: f32,
    pub vibrance: f32,
    pub intensity: f32,
    pub black_level: f32,
    pub rcas_strength: f32,
    pub fxaa_strength: f32,
    pub sdr_brightness: f32,
    pub preferred_format: OutputFormat,
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
            black_level: 0.0,
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
    let s_int = create_row(&grid, 4, "Intensity", 0.0, 1.0, 0.05, 1.0);
    let s_black = create_row(&grid, 5, "Black Level", -1.0, 1.0, 0.05, 0.0);
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
        let s_black = s_black.clone();
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
                            s_black.set_value(config.black_level as f64);
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
        let s_black = s_black.clone();
        let s_rcas = s_rcas.clone();
        let s_fxaa = s_fxaa.clone();
        let s_sdr = s_sdr.clone();
        let format_dropdown = format_dropdown.clone();

        move |_| {
            let mut st = state.borrow_mut();
            if let Some(ref path) = st.current_file {
                let new_config = HdrConfig {
                    max_lum: s_max_lum.value() as f32,
                    mid_lum: s_mid_lum.value() as f32,
                    sat: s_sat.value() as f32,
                    vibrance: s_vib.value() as f32,
                    intensity: s_int.value() as f32,
                    black_level: s_black.value() as f32,
                    rcas_strength: s_rcas.value() as f32,
                    fxaa_strength: s_fxaa.value() as f32,
                    sdr_brightness: s_sdr.value() as f32,
                    preferred_format: if format_dropdown.selected() == 0 { OutputFormat::PQ } else { OutputFormat::ScRGB },
                };
                
                if let Ok(toml_str) = toml::to_string_pretty(&new_config) {
                    if let Ok(_) = fs::write(path, toml_str) {
                        println!("Saved config to {:?}", path);
                    }
                }
                st.current_config = new_config;
            }
        }
    });

    window.present();
}
