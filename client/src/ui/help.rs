/// Help dialog with tabbed sections covering keyboard shortcuts and usage guide.
pub struct HelpDialog {
    pub open: bool,
    tab: HelpTab,
}

#[derive(Clone, PartialEq)]
enum HelpTab {
    Shortcuts,
    MapControls,
    SpawnList,
    Timers,
    Alerts,
}

impl Default for HelpDialog {
    fn default() -> Self {
        Self { open: false, tab: HelpTab::Shortcuts }
    }
}

impl HelpDialog {
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.open = false;
            return;
        }

        let mut open = true;
        egui::Window::new("Help")
            .collapsible(false)
            .resizable(true)
            .default_size([520.0, 440.0])
            .open(&mut open)
            .show(ctx, |ui| {
                // Tab bar
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, HelpTab::Shortcuts, "Keyboard Shortcuts");
                    ui.selectable_value(&mut self.tab, HelpTab::MapControls, "Map Controls");
                    ui.selectable_value(&mut self.tab, HelpTab::SpawnList, "Spawn List");
                    ui.selectable_value(&mut self.tab, HelpTab::Timers, "Timers");
                    ui.selectable_value(&mut self.tab, HelpTab::Alerts, "Alerts");
                });
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    match self.tab {
                        HelpTab::Shortcuts => show_shortcuts(ui),
                        HelpTab::MapControls => show_map_controls(ui),
                        HelpTab::SpawnList => show_spawn_list(ui),
                        HelpTab::Timers => show_timers(ui),
                        HelpTab::Alerts => show_alerts(ui),
                    }
                });

                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            self.open = false;
                        }
                    });
                });
            });

        if !open {
            self.open = false;
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title).strong().size(13.0));
    ui.separator();
}

fn kv(ui: &mut egui::Ui, key: &str, desc: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [130.0, 0.0],
            egui::Label::new(egui::RichText::new(key).monospace()),
        );
        ui.label(desc);
    });
}

fn bullet(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.label("•");
        ui.label(text);
    });
}

fn show_shortcuts(ui: &mut egui::Ui) {
    section(ui, "Map Navigation");
    kv(ui, "+ / =",      "Zoom in");
    kv(ui, "-",          "Zoom out");
    kv(ui, "Scroll",     "Zoom in / out (map focused)");
    kv(ui, "Home",       "Center map on player");
    kv(ui, "Drag",       "Pan the map");

    section(ui, "Panel Visibility");
    kv(ui, "F5",         "Toggle Spawns panel");
    kv(ui, "F6",         "Toggle Timers panel");
    kv(ui, "F7",         "Toggle Ground Items panel");

    section(ui, "Display");
    kv(ui, "T",          "Toggle mob trails");

    section(ui, "Search");
    kv(ui, "Ctrl+F",     "Open Find Spawn dialog");
    kv(ui, "ESC",        "Close search / clear bearing line");

    ui.add_space(6.0);
}

fn show_map_controls(ui: &mut egui::Ui) {
    section(ui, "Navigation");
    bullet(ui, "Left-click and drag — pan the map");
    bullet(ui, "Scroll wheel — zoom in / out");
    bullet(ui, "Home key — center on player");

    section(ui, "Bearing Line");
    bullet(ui, "Shift+left-click — draw a line from the player to the clicked point");
    bullet(ui, "Shows distance (EQ units), angle (degrees), and cardinal direction");
    bullet(ui, "ESC or plain left-click — clear the bearing line");

    section(ui, "Spawn Interaction");
    bullet(ui, "Left-click a spawn dot — select it (gold ring on map, gold bar in list)");
    bullet(ui, "Click the same dot again — deselect");
    bullet(ui, "Right-click a spawn dot — context menu (Add Timer, Add to Filter, Add Map Text)");

    section(ui, "Spawn Colors");
    bullet(ui, "Red  — Danger");
    bullet(ui, "Orange — Caution");
    bullet(ui, "Green — Hunt");
    bullet(ui, "Purple — Rare");
    bullet(ui, "Orange ring — current EQ target");
    bullet(ui, "Custom colors can be set in cfg/spawn_colors.json");

    ui.add_space(6.0);
}

fn show_spawn_list(ui: &mut egui::Ui) {
    section(ui, "Selection");
    bullet(ui, "Left-click a row — select spawn (gold ring on map, gold accent bar in list)");
    bullet(ui, "Click the same row again — deselect");
    bullet(ui, "Double-click a row — center the map on that spawn");
    bullet(ui, "Orange right-edge bar — current EQ target");

    section(ui, "Sorting");
    bullet(ui, "Click any column header to sort by that column");
    bullet(ui, "Click again to reverse sort order");

    section(ui, "Context Menu (right-click)");
    bullet(ui, "Add Timer — track respawn time for this mob");
    bullet(ui, "Add to Filter — categorize as Hunt / Caution / Danger / Rare");
    bullet(ui, "Add Map Text — place a note at the spawn's position");

    section(ui, "Search");
    bullet(ui, "Ctrl+F — find spawns by partial name (case-insensitive)");
    bullet(ui, "Matching spawns get a white ring on the map and a cyan bar in the list");

    section(ui, "Filters");
    bullet(ui, "Filters are loaded from the configured filter directory (seqfilters.xml)");
    bullet(ui, "Each spawn is classified as Hunt / Caution / Danger / Rare based on name");

    ui.add_space(6.0);
}

fn show_timers(ui: &mut egui::Ui) {
    section(ui, "Adding Timers");
    bullet(ui, "Right-click a spawn in the list → Add Timer");
    bullet(ui, "Enter the respawn time in minutes, then click Add");

    section(ui, "Timer Display");
    bullet(ui, "Each timer shows the mob name, position, and countdown");
    bullet(ui, "Expired timers are shown in red");
    bullet(ui, "Click a column header to sort");

    section(ui, "Persistence");
    bullet(ui, "Timers are saved per zone and restored automatically on zone entry");
    bullet(ui, "Stored in the configured timer directory");

    ui.add_space(6.0);
}

fn show_alerts(ui: &mut egui::Ui) {
    section(ui, "Configuration");
    bullet(ui, "Open File → Options → Alerts tab to configure alert behavior");
    bullet(ui, "Each category (Danger, Caution, Hunt, Rare) has its own mode");

    section(ui, "Alert Modes");
    kv(ui, "None",    "No alert");
    kv(ui, "Beep",    "System beep");
    kv(ui, "Speech",  "Text-to-speech announcement");
    kv(ui, "Sound",   "Play a custom sound file (.wav)");

    section(ui, "Discord");
    bullet(ui, "Configure a webhook URL in Options → Discord");
    bullet(ui, "Enable per-category posting (Danger, Hunt)");
    bullet(ui, "Posts spawn name, level, and location to your Discord channel");

    ui.add_space(6.0);
}