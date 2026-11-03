//! Kontextsensitive WinHelp-Integration (`M4W_GER.HLP`) und Hilfe-Fenster-Steuerung.

pub use super::winhelp::{
    HelpClickResult, HelpTopic, HelpViewerState, HELP_WINDOW_HEIGHT, HELP_WINDOW_WIDTH,
};

use super::winit::event_loop::ActiveEventLoop;

use super::app::{Mod4WinApp, WindowKind};

impl Mod4WinApp {
    /// Liefert das kontextsensitive WinHelp-Thema (`|TOPIC`) für ein geöffnetes Dialogfenster (`F1` / `Help`-Button).
    pub fn context_help_topic_for_window(&self, kind: WindowKind) -> &'static str {
        match kind {
            WindowKind::Main => {
                if self.is_mini {
                    "MiniState"
                } else {
                    "HauptDlg"
                }
            }
            WindowKind::ModuleInfo => "InfoDlg",
            WindowKind::InstrumentInfo => "InstrWindow",
            WindowKind::Effects => "EffWnd",
            WindowKind::EffectsHelp => "Effects",
            WindowKind::QuickSelect => "QuickSelBox",
            WindowKind::OpenDialog => "OpenDlg",
            WindowKind::SetupDialog => "SetupDlg",
            WindowKind::WavAskDialog => "RecordingDlg",
            WindowKind::FileSelectDialog => "FselDlg",
            WindowKind::FileManager => "fmdlg",
            WindowKind::HotkeyEditor => "keyseldlg",
            WindowKind::ArchiveSetup => {
                if self.archive_dlg.more_arc_type.is_some() {
                    "arcparm"
                } else {
                    "ArcOptDlg"
                }
            }
            WindowKind::About
            | WindowKind::HelpViewer
            | WindowKind::FileMenu
            | WindowKind::ConfirmDialog => "HauptDlg",
        }
    }

    pub(crate) fn open_help_topic(&mut self, event_loop: &ActiveEventLoop, topic_id: &str) {
        self.help_dlg.set_language(self.setup_cfg.language);
        self.help_dlg.open_topic(topic_id);
        self.open_window(event_loop, WindowKind::HelpViewer);
    }
}

