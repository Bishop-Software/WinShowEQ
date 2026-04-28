use std::sync::{Arc, Mutex};

use super::GuiState;

pub struct WinShowEQApp {
    state: Arc<Mutex<GuiState>>,
}

impl WinShowEQApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, state: Arc<Mutex<GuiState>>) -> Self {
        Self { state }
    }
}

impl eframe::App for WinShowEQApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Poll the server thread for new state every 100 ms even without user input.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        ui.heading("WinShowEQ");
    }
}