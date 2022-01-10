use anyhow::Result;
use smithay::{
    backend::{
        renderer::{Renderer, Frame, Transform},
        winit,
    },
    reexports::wayland_server::Display,
    utils::Rectangle,
    wayland::{
        seat::{FilterResult, ModifiersState, Seat, XkbConfig},
        SERIAL_COUNTER,
    },
};
use smithay_egui::EguiState;
use std::cell::RefCell;

// This example provides a minimal example to:
// - Setup and `Renderer` and get `InputEvents` via winit.
// - Pass those input events to egui
// - Render an egui interface using that renderer.

// It does not show of the best practices to do so,
// neither does the example show-case a real wayland compositor.
// For that take a look into [`anvil`](https://github.com/Smithay/smithay/tree/master/anvil).

// This is only meant to provide a starting point to integrate egui into an already existing compositor

fn main() -> Result<()> {
    // setup logger
    let _guard = setup_logger();
    // create a winit-backend
    let (mut backend, mut input) = winit::init(None)?;
    // create an `EguiState`. Usually this would be part of your global smithay state
    let mut egui = EguiState::new();
    // you might also need additional structs to store your ui-state, like the demo_lib does
    let mut demo_ui = egui_demo_lib::DemoWindows::default();
    // this is likely already part of your ui-state for `send_frames` and similar
    let start_time = std::time::Instant::now();
    // We need to track the current set of modifiers, because egui expects them to be passed for many events
    let modifiers = RefCell::new(ModifiersState::default());

    // Usually you should already have a seat
    let mut display = Display::new();
    let (mut seat, _global) = Seat::new(&mut display, "seat-0".to_string(), None);
    // For a real compositor we would add a socket here and put the display inside an event loop,
    // but all we need for this example is the seat for it's input handling
    let keyboard = seat.add_keyboard(XkbConfig::default(), 200, 25, |_seat, _focus| {})?;

    loop {
        input.dispatch_new_events(|event| {
            use smithay::backend::{
                input::{
                    Axis, ButtonState, Event, InputEvent, KeyState, KeyboardKeyEvent,
                    PointerAxisEvent, PointerButtonEvent, PointerMotionAbsoluteEvent,
                },
                winit::WinitEvent::*,
            };
            match event {
                // Handle input events by passing them into smithay-egui
                Input(event) => match event {
                    // egui tracks pointers
                    InputEvent::DeviceAdded { device } => egui.handle_device_added(&device),
                    InputEvent::DeviceRemoved { device } => egui.handle_device_added(&device),
                    // we rely on the filter-closure of the keyboard.input call to get the values we need for egui.
                    //
                    // NOTE: usually you would need to check `EguiState::wants_keyboard_input` or track focus of egui
                    //       using the methods provided in `EguiState.context().memory()` separately to figure out
                    //       if an event should be forwarded to egui or not.
                    InputEvent::Keyboard { event } => keyboard
                        .input(
                            event.key_code(),
                            event.state(),
                            SERIAL_COUNTER.next_serial(),
                            event.time(),
                            |new_modifiers, handle| {
                                egui.handle_keyboard(
                                    handle.raw_syms(),
                                    event.state() == KeyState::Pressed,
                                    new_modifiers.clone(),
                                );
                                *modifiers.borrow_mut() = new_modifiers.clone();
                                FilterResult::Intercept(())
                            },
                        )
                        .unwrap_or(()),
                    // Winit only produces `PointerMotionAbsolute` events, but a real compositor needs to handle this for `PointerMotion` events as well.
                    // Meaning: you need to compute the absolute position and pass that to egui.
                    InputEvent::PointerMotionAbsolute { event } => egui.handle_pointer_motion(
                        event
                            .position_transformed(backend.window_size().physical_size.to_logical(1))
                            .to_i32_round()
                    ),
                    // NOTE: you should check with `EguiState::wwants_pointer`, if the pointer is above any egui element before forwarding it.
                    // Otherwise forward it to clients as usual.
                    InputEvent::PointerButton { event } => {
                        if let Some(button) = event.button() {
                            egui.handle_pointer_button(
                                button,
                                event.state() == ButtonState::Pressed,
                                modifiers.borrow().clone(),
                            );
                        }
                    }
                    // NOTE: you should check with `EguiState::wwants_pointer`, if the pointer is above any egui element before forwarding it.
                    // Otherwise forward it to clients as usual.
                    InputEvent::PointerAxis { event } => egui.handle_pointer_axis(
                        event
                            .amount_discrete(Axis::Horizontal)
                            .or_else(|| event.amount(Axis::Horizontal).map(|x| x * 3.0))
                            .unwrap_or(0.0),
                        event
                            .amount_discrete(Axis::Vertical)
                            .or_else(|| event.amount(Axis::Vertical).map(|x| x * 3.0))
                            .unwrap_or(0.0),
                    ),
                    _ => {}
                },
                _ => {}
            }
        })?;
        
        let size = backend.window_size().physical_size;

        // Here we compute the rendered egui frame
        let egui_frame = egui.run(
            |ctx| demo_ui.ui(ctx),
            // Just render it over the whole window, but you may limit the area
            Rectangle::from_loc_and_size((0, 0), size.to_logical(1)),
            size,
            // we also completely ignore the scale *everywhere* in this example, but egui is HiDPI-ready
            1.0,
            1.0,
            &start_time,
            modifiers.borrow().clone(),
        );

        // Lastly put the rendered frame on the screen
        backend.bind()?;
        let renderer = backend.renderer();
        renderer
            .render(size, Transform::Flipped180, |renderer, frame| {
                frame.clear([1.0, 1.0, 1.0, 1.0], &[Rectangle::from_loc_and_size((0, 0), size)])?;
                unsafe { egui_frame.draw(renderer, frame) }
            })?
            .map_err(|err| anyhow::format_err!("{}", err))?;
        backend.submit(None, 1.0)?;
    }
}

fn setup_logger() -> Result<slog_scope::GlobalLoggerGuard> {
    use slog::Drain;

    let decorator = slog_term::TermDecorator::new().stderr().build();
    // usually we would not want to use a Mutex here, but this is usefull for a prototype,
    // to make sure we do not miss any in-flight messages, when we crash.
    let logger = slog::Logger::root(
        std::sync::Mutex::new(
            slog_term::CompactFormat::new(decorator)
                .build()
                .ignore_res(),
        )
        .fuse(),
        slog::o!(),
    );
    let guard = slog_scope::set_global_logger(logger);
    slog_stdlog::init().unwrap();
    Ok(guard)
}