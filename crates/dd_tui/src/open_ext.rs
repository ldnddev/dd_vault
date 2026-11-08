//! Open a URL or vault file in the operating system's default app.
//!
//! Tests record the target and do not spawn. A normal build calls
//! [`dd_vault_core::launch_default_app`].

#[cfg(test)]
use std::cell::RefCell;

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
    record_or_spawn(target)
}

#[cfg(test)]
fn record_or_spawn(target: &str) -> Result<(), String> {
    LAST_OPENED.with(|slot| *slot.borrow_mut() = Some(target.to_string()));
    Ok(())
}

#[cfg(not(test))]
fn record_or_spawn(target: &str) -> Result<(), String> {
    dd_vault_core::launch_default_app(target)
}
