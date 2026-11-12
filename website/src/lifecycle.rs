#[cfg(target_arch = "wasm32")]
use dioxus::prelude::*;
// Rust bindings only: no eval, custom JS or generated application script.
#[cfg(target_arch = "wasm32")]
pub(crate) struct WebLifecycle {
    listeners: Vec<Listener>,
}

#[cfg(target_arch = "wasm32")]
struct Listener {
    target: web_sys::EventTarget,
    name: &'static str,
    callback: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>,
}

#[cfg(target_arch = "wasm32")]
impl WebLifecycle {
    pub(crate) fn install(mut epoch: Signal<u64>) -> Self {
        use wasm_bindgen::{closure::Closure, JsCast};
        let mut listeners = Vec::new();
        if let Some(window) = web_sys::window() {
            let target: web_sys::EventTarget = window.into();
            for name in ["blur", "visibilitychange", "scroll", "resize"] {
                let source = target.clone();
                let callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
                    // Window capture also sees blur from an input or toolbar
                    // button. Taking canvas focus must not cancel its gesture.
                    if name == "blur" && event.target().as_ref() != Some(&source) {
                        return;
                    }
                    let next = epoch.peek().wrapping_add(1);
                    epoch.set(next);
                }) as Box<dyn FnMut(web_sys::Event)>);
                match target.add_event_listener_with_callback_and_bool(
                    name,
                    callback.as_ref().unchecked_ref(),
                    true,
                ) {
                    Ok(()) => listeners.push(Listener {
                        target: target.clone(),
                        name,
                        callback,
                    }),
                    Err(_) => eprintln!("Could not install lifecycle listener: {name}"),
                }
            }
        }
        Self { listeners }
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for WebLifecycle {
    fn drop(&mut self) {
        use wasm_bindgen::JsCast;
        for Listener {
            target,
            name,
            callback,
        } in &self.listeners
        {
            let _ = target.remove_event_listener_with_callback_and_bool(
                name,
                callback.as_ref().unchecked_ref(),
                true,
            );
        }
    }
}
