/// About dialog displaying version and credits.
pub struct AboutDialog {
    pub open: bool,
}

impl AboutDialog {
    pub fn new() -> Self {
        Self { open: false }
    }

    /// Display the about dialog.
    pub fn show(&mut self, ctx: &egui::Context) {
        if self.open {
            let mut open = true;
            egui::Window::new("About WinShowEQ")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("WinShowEQ");
                        ui.add_space(8.0);

                        ui.label(egui::RichText::new("Version 0.1.0").strong());
                        ui.label("Rust rewrite of MySEQ EverQuest map overlay");
                        ui.add_space(12.0);

                        ui.separator();
                        ui.add_space(8.0);

                        ui.label(egui::RichText::new("Credits").strong());
                        ui.label("Project: Bishop Software");
                        ui.label("Based on the Original MySEQ");
                        ui.add_space(4.0);

                        ui.label(egui::RichText::new("Libraries").strong());
                        ui.horizontal(|ui| {
                            if ui.link("egui").clicked() {
                                let _ = open_url("https://github.com/emilk/egui");
                            }
                            ui.label("— Immediate mode GUI");
                        });
                        ui.horizontal(|ui| {
                            if ui.link("eframe").clicked() {
                                let _ = open_url(
                                    "https://github.com/emilk/egui/tree/master/crates/eframe",
                                );
                            }
                            ui.label("— Desktop application framework");
                        });
                        ui.horizontal(|ui| {
                            if ui.link("egui_dock").clicked() {
                                let _ = open_url("https://github.com/domathid/egui_dock");
                            }
                            ui.label("— Dockable panel layout");
                        });
                        ui.horizontal(|ui| {
                            if ui.link("rodio").clicked() {
                                let _ = open_url("https://github.com/RustAudio/rodio");
                            }
                            ui.label("— Audio playback");
                        });
                        ui.horizontal(|ui| {
                            if ui.link("tts").clicked() {
                                let _ = open_url("https://github.com/nateshmbhat/tts-rs");
                            }
                            ui.label("— Text-to-speech");
                        });
                        ui.horizontal(|ui| {
                            if ui.link("ureq").clicked() {
                                let _ = open_url("https://github.com/algesten/ureq");
                            }
                            ui.label("— HTTP client");
                        });
                        ui.add_space(12.0);

                        ui.separator();
                        ui.add_space(8.0);

                        ui.label(egui::RichText::new("Icons").strong());
                        ui.horizontal(|ui| {
                            if ui.link("paul-j").clicked() {
                                let _ = open_url("https://www.flaticon.com/authors/paul-j");
                            }
                            ui.label("— Toolbar icons via Flaticon");
                        });
                        ui.add_space(12.0);

                        ui.separator();
                        ui.add_space(8.0);

                        ui.label(egui::RichText::new("License").strong());
                        ui.label("GPL-3.0 — See LICENSE file for details");
                        ui.add_space(12.0);

                        ui.label(egui::RichText::new("Links").strong());
                        if ui.link("GitHub: Bishop-Software/WinShowEQ").clicked() {
                            let _ = open_url("https://github.com/Bishop-Software/WinShowEQ");
                        }
                        if ui.link("Original MySEQ Project").clicked() {
                            let _ = open_url("https://sourceforge.net/projects/seq/");
                        }
                        ui.add_space(12.0);

                        if ui.button("Close").clicked() {
                            self.open = false;
                        }
                    });
                });
            if !open {
                self.open = false;
            }
        }
    }
}

/// Attempt to open a URL in the default browser.
#[cfg(target_os = "windows")]
fn open_url(url: &str) -> std::io::Result<()> {
    std::process::Command::new("cmd")
        .args(["/C", "start", url])
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_url(url: &str) -> std::io::Result<()> {
    std::process::Command::new("open").arg(url).spawn()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn open_url(url: &str) -> std::io::Result<()> {
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    Ok(())
}
