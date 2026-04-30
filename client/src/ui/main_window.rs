use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{IPT_GROUND, IPT_SELF, IPT_SPAWNS, IPT_TARGET, IPT_WORLD, IPT_ZONE};

use crate::config::ClientConfig;
use crate::data::timers::TimerStore;
use crate::data::{apply_packet, AppData};
use crate::net::ServerConnection;
use crate::protocol::decode_packet;
use crate::ui::ground_list;
use crate::ui::login::LoginDialog;
use crate::ui::map_pane::MapPane;
use crate::ui::options::OptionsDialog;
use crate::ui::spawn_list;
use crate::ui::timer_list;

const TICK_REQUEST: i32 =
    IPT_ZONE | IPT_SELF | IPT_TARGET | IPT_SPAWNS | IPT_GROUND | IPT_WORLD;
const TICK_DELAY_MS: u64 = 250;
const RECONNECT_DELAY_SECS: u64 = 2;

#[derive(Default, PartialEq)]
enum BottomTab {
    #[default]
    Timers,
    Ground,
}

pub struct MainApp {
    data: Arc<Mutex<AppData>>,
    config: ClientConfig,
    config_path: PathBuf,
    map_pane: MapPane,
    login: LoginDialog,
    options: OptionsDialog,
    bottom_tab: BottomTab,
    stop: Arc<AtomicBool>,
    server_addr: Arc<Mutex<Option<SocketAddr>>>,
    prev_zone: String,
}

impl MainApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        server_addr: Option<SocketAddr>,
        config_path: PathBuf,
    ) -> Self {
        let config = ClientConfig::load(&config_path);
        let data = Arc::new(Mutex::new(AppData::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let addr_cell: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(server_addr));

        start_network_thread(Arc::clone(&addr_cell), Arc::clone(&data), Arc::clone(&stop));

        let login = LoginDialog::new(server_addr);
        let options = OptionsDialog::new(&config, false);

        Self {
            data,
            config,
            config_path,
            map_pane: MapPane::default(),
            login,
            options,
            bottom_tab: BottomTab::default(),
            stop,
            server_addr: addr_cell,
            prev_zone: String::new(),
        }
    }

    fn save_config(&self) {
        let _ = self.config.save(&self.config_path);
    }

    fn handle_zone_change(&mut self, new_zone: String) {
        if !self.prev_zone.is_empty() {
            let data = self.data.lock().unwrap();
            let _ = data.timers.save(&self.prev_zone, &self.config.timer_dir);
        }
        let new_timers = TimerStore::load(&new_zone, &self.config.timer_dir);
        let mut data = self.data.lock().unwrap();
        data.timers = new_timers;
        self.prev_zone = new_zone;
    }
}

impl eframe::App for MainApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(TICK_DELAY_MS));

        let zone_name = self.data.lock().unwrap().zone_name.clone();
        if !zone_name.is_empty() && zone_name != self.prev_zone {
            self.handle_zone_change(zone_name);
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.prev_zone.is_empty() {
                let data = self.data.lock().unwrap();
                let _ = data.timers.save(&self.prev_zone, &self.config.timer_dir);
            }
            self.save_config();
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Floating dialogs
        if let Some(new_addr) = self.login.show(&ctx) {
            *self.server_addr.lock().unwrap() = Some(new_addr);
        }
        if let Some(new_config) = self.options.show(&ctx) {
            let trails = self.options.trails_enabled;
            self.config = new_config;
            self.data.lock().unwrap().trails_enabled = trails;
            let _ = self.config.save(&self.config_path);
        }

        // Menu bar
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Connect…").clicked() {
                    self.login.open = true;
                    ui.close();
                }
                if ui.button("Options…").clicked() {
                    let trails = self.data.lock().unwrap().trails_enabled;
                    self.options.sync_from(&self.config, trails);
                    self.options.open = true;
                    ui.close();
                }
            });
        });

        // Right panel: spawn list
        egui::Panel::right("spawn_panel")
            .resizable(true)
            .default_size(230.0)
            .show_inside(ui, |ui| {
                let data = self.data.lock().unwrap();
                ui.heading(format!("Spawns ({})", data.spawns.len()));
                ui.separator();
                spawn_list::show(ui, &data);
            });

        // Bottom panel: timers / ground tabs
        egui::Panel::bottom("bottom_panel")
            .resizable(true)
            .default_size(140.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.bottom_tab, BottomTab::Timers, "Timers");
                    ui.selectable_value(&mut self.bottom_tab, BottomTab::Ground, "Ground Items");
                });
                ui.separator();
                match self.bottom_tab {
                    BottomTab::Timers => {
                        let mut data = self.data.lock().unwrap();
                        timer_list::show(ui, &mut data.timers);
                    }
                    BottomTab::Ground => {
                        let data = self.data.lock().unwrap();
                        ground_list::show(ui, &data);
                    }
                }
            });

        // Center: map canvas
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let data = self.data.lock().unwrap();
            self.map_pane.show(ui, &data);
        });
    }
}

fn start_network_thread(
    addr_cell: Arc<Mutex<Option<SocketAddr>>>,
    data: Arc<Mutex<AppData>>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let addr = *addr_cell.lock().unwrap();
            let Some(addr) = addr else {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            };
            match ServerConnection::connect_with_timeout(addr, 5000) {
                Err(_) => {
                    std::thread::sleep(Duration::from_secs(RECONNECT_DELAY_SECS));
                }
                Ok(mut conn) => {
                    loop {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        // Reconnect if address changed
                        if addr_cell.lock().unwrap().as_ref() != Some(&addr) {
                            break;
                        }
                        match conn.tick(TICK_REQUEST) {
                            Ok(records) => {
                                let mut d = data.lock().unwrap();
                                for rec in records {
                                    apply_packet(&mut d, decode_packet(rec));
                                }
                            }
                            Err(_) => break,
                        }
                        std::thread::sleep(Duration::from_millis(TICK_DELAY_MS));
                    }
                }
            }
        }
    });
}