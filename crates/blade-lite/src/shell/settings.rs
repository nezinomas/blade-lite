//! The Settings group's buttons: export, import and reset.

use std::path::Path;
use std::rc::Rc;

use slint::ComponentHandle;

use blade_lite::AppError;
use platform::FileDialog;

use super::windows::hwnd;
use super::{AppOn, Parts, Shell};

impl<P: Parts> Shell<P> {
    /// Asks for a file to write, then writes the settings there.
    pub fn export_settings(self: &Rc<Self>) {
        self.with_file(FileDialog::Save, |app, path| app.export_settings(path));
    }

    /// Asks for a file to read, then sends and saves it.
    pub fn import_settings(self: &Rc<Self>) {
        self.with_file(FileDialog::Open, AppOn::<P>::import_settings);
    }

    /// Asks for a file over the window, then acts on it as a click does. A
    /// cancel does nothing. The dialog blocks the window while it is open,
    /// and no borrow is held, so the flyout still works then.
    fn with_file(
        self: &Rc<Self>,
        dialog: FileDialog,
        act: impl FnOnce(&mut AppOn<P>, &Path) -> Result<(), AppError>,
    ) {
        match P::pick_file(hwnd(self.window.window()), dialog) {
            Ok(Some(path)) => self.user_act(|app| act(app, &path)),
            Ok(None) => {}
            Err(error) => self.refresh(Some(&AppError::FileDialog(error))),
        }
    }
}
