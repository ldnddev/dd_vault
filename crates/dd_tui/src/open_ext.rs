//! Open a URL or vault file in the operating system's default app.

#[cfg(test)]
use std::cell::RefCell;

#[cfg(not(test))]
use std::process::{Command, Stdio};

#[cfg(test)]
thread_local! {
    static LAST_OPENED: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub fn last_opened() -> Option<String> {
    LAST_OPENED.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
pub fn clear_opened() {
    LAST_OPENED.with(|slot| *slot.borrow_mut() = None);
}

pub fn launch_default_app(target: &str) -> Result<(), String> {
    if target.chars().any(|c| c.is_control()) {
        return Err("refusing to open a target with control characters".into());
    }
    launch(target)
}

#[cfg(test)]
fn launch(target: &str) -> Result<(), String> {
    LAST_OPENED.with(|slot| *slot.borrow_mut() = Some(target.to_string()));
    Ok(())
}

#[cfg(not(test))]
fn launch(target: &str) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(program)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not open with {program}: {err}"))
}
