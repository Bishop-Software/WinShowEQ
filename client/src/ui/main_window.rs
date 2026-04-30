use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{IPT_GROUND, IPT_SELF, IPT_SPAWNS, IPT_TARGET, IPT_WORLD, IPT_ZONE};

use crate::data::{apply_packet, AppData};
use crate::net::ServerConnection;
use crate::protocol::decode_packet;
use crate::ui::map_pane::MapPane;

const TICK_REQUEST: i32 =
    IPT_ZONE | IPT_SELF | IPT_TARGET | IPT_SPAWNS | IPT_GROUND | IPT_WORLD;
const TICK_DELAY_MS: u64 = 1000;
const RECONNECT_DELAY_SECS: u64 = 2;

pub struct MainApp {
    data: Arc<Mutex<AppData>>,
    map_pane: MapPane,
    stop: Arc<AtomicBool>,
}

impl MainApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        server_addr: Option<SocketAddr>,
    ) -> Self {
        let data = Arc::new(Mutex::new(AppData::default()));
        let stop = Arc::new(AtomicBool::new(false));

        if let Some(addr) = server_addr {
            start_network_thread(addr, Arc::clone(&data), Arc::clone(&stop));
        }

        Self {
            data,
            map_pane: MapPane::default(),
            stop,
        }
    }
}

impl eframe::App for MainApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(TICK_DELAY_MS));

        if ctx.input(|i| i.viewport().close_requested()) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let data = self.data.lock().unwrap();
        self.map_pane.show(ui, &data);
    }
}

fn start_network_thread(
    addr: SocketAddr,
    data: Arc<Mutex<AppData>>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match ServerConnection::connect_with_timeout(addr, 5000) {
                Err(_) => {
                    std::thread::sleep(Duration::from_secs(RECONNECT_DELAY_SECS));
                }
                Ok(mut conn) => {
                    while !stop.load(Ordering::Relaxed) {
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