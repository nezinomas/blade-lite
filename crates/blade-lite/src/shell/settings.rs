//! The Settings group's buttons: export, import and reset.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use blade_lite::AppError;
use platform::FileDialog;
use slint::ComponentHandle;

use super::windows::hwnd;
use super::{AppOn, Parts, PickedFile, Shell};

thread_local! {
    /// What to do with the file the open dialog gives, on the app's thread.
    pub(super) static PICKED: RefCell<Option<PickedFile>> = const { RefCell::new(None) };
}

impl<P: Parts> Shell<P> {
    /// Asks for a file to write, then writes the settings there.
    pub fn export_settings(self: &Rc<Self>) {
        self.with_file(FileDialog::Save, |app, path| app.export_settings(path));
    }

    /// Asks for a file to read, then sends and saves it.
    pub fn import_settings(self: &Rc<Self>) {
        self.with_file(FileDialog::Open, AppOn::<P>::import_settings);
    }

    /// Asks for a file over the window, then acts on it as a click does,
    /// so the power events that came while the dialog was open are
    /// followed first. A cancel does nothing. One dialog at a time.
    fn with_file(
        self: &Rc<Self>,
        dialog: FileDialog,
        act: impl FnOnce(&mut AppOn<P>, &Path) -> Result<(), AppError> + 'static,
    ) {
        if self.dialog_open.get() {
            return;
        }
        let owner = hwnd(self.window.window());
        self.dialog_open.set(true);
        let shell = Rc::downgrade(self);
        P::pick_file(
            owner,
            dialog,
            Box::new(move |picked| {
                let Some(shell) = shell.upgrade() else {
                    return;
                };
                shell.dialog_open.set(false);
                match picked {
                    Ok(Some(path)) => {
                        // The dialog does not block the app, so the laptop
                        // may have slept while it was open.
                        shell.follow_own_power();
                        if shell.resuming.get() > 0 {
                            shell.refresh(Some(&AppError::JustWoke));
                        } else {
                            shell.user_act(|app| act(app, &path));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => shell.refresh(Some(&AppError::FileDialog(error))),
                }
            }),
        );
    }
}
