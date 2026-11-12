use dioxus::desktop::{tao, use_window, use_wry_event_handler, Config, WindowEvent};
use dioxus::prelude::*;
use map_editor_demo::Demo;

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new().with_window(
                tao::window::WindowBuilder::new()
                    .with_title("Dioxus Map Editor · Drawing Lab")
                    .with_inner_size(tao::dpi::LogicalSize::new(1240.0, 900.0)),
            ),
        )
        .launch(App);
}

#[component]
fn App() -> Element {
    let mut epoch = use_signal(|| 0_u64);
    let window_id = use_window().id();
    use_wry_event_handler(move |event, _| {
        if let tao::event::Event::WindowEvent {
            window_id: id,
            event,
            ..
        } = event
        {
            if *id == window_id
                && matches!(
                    event,
                    WindowEvent::Focused(false)
                        | WindowEvent::Resized(_)
                        | WindowEvent::ScaleFactorChanged { .. }
                )
            {
                let next = epoch.peek().wrapping_add(1);
                epoch.set(next);
            }
        }
    });
    rsx! { Demo { invalidation: epoch() } }
}
