use anyhow::Result;
use smithay::{
    backend::{
        renderer::{Renderer, Transform},
        winit,
    },
    reexports::wayland_server::Display,
    wayland::{
        SERIAL_COUNTER,
        seat::{Seat, ModifiersState, FilterResult, XkbConfig},
    },
    utils::Rectangle,
};
use smithay_egui::{EguiState};
use std::cell::RefCell;

fn main() -> Result<()> {
    let (mut backend, mut input) = winit::init(None)?;
    let mut egui = EguiState::new();
    let mut demo_ui = egui_demo_lib::DemoWindows::default(); 
    let start_time = std::time::Instant::now();
    let modifiers = RefCell::new(ModifiersState::default());

    let mut display = Display::new();
    let (mut seat, _global) = Seat::new(&mut display, "seat-0".to_string(), None);
    // usually we would add a socket here and put the display inside an event loop,
    // but all we need for this example is the seat for input handling
    let keyboard = seat.add_keyboard(XkbConfig::default(), 200, 25, |_seat, _focus| {})?;

    loop {
        input.dispatch_new_events(|event| {
            use smithay::backend::{
                input::{InputEvent, Event, KeyboardKeyEvent, PointerMotionAbsoluteEvent, PointerButtonEvent, PointerAxisEvent, Axis, KeyState, ButtonState},
                winit::WinitEvent::*,
            };
            match event {
                Input(event) => match event {
                    InputEvent::DeviceAdded { device } => egui.handle_device_added(&device), 
                    InputEvent::DeviceRemoved { device } => egui.handle_device_added(&device),
                    InputEvent::Keyboard { event } => keyboard.input(event.key_code(), event.state(), SERIAL_COUNTER.next_serial(), event.time(), |new_modifiers, handle| {
                        egui.handle_keyboard(handle.raw_syms(), event.state() == KeyState::Pressed, new_modifiers.clone());
                        *modifiers.borrow_mut() = new_modifiers.clone();
                        FilterResult::Intercept(())
                    }).unwrap_or(()),
                    InputEvent::PointerMotionAbsolute { event } => egui.handle_pointer_motion(event.position_transformed(backend.window_size().physical_size.to_logical(1)).to_i32_round()),
                    InputEvent::PointerButton { event } => if let Some(button) = event.button() { egui.handle_pointer_button(button, event.state() == ButtonState::Pressed, modifiers.borrow().clone()); },
                    InputEvent::PointerAxis { event } => egui.handle_pointer_axis(
                        event.amount_discrete(Axis::Horizontal).or_else(|| event.amount(Axis::Horizontal).map(|x| x * 3.0)).unwrap_or(0.0),
                        event.amount_discrete(Axis::Vertical).or_else(|| event.amount(Axis::Vertical).map(|x| x * 3.0)).unwrap_or(0.0),
                    ),
                    _ => {},
                }
                _ => {},
            }
        })?;
        
        let size = backend.window_size().physical_size;
        let frame = egui.run(
            |ctx| {
                demo_ui.ui(ctx)
            },
            Rectangle::from_loc_and_size((0, 0),
            size.to_logical(1)),
            size,
            1.0,
            &start_time,
            modifiers.borrow().clone());
        
        backend.bind()?;
        let renderer = backend.renderer();
        renderer.render(size, Transform::Flipped180, |_renderer, _frame| {
            frame.draw()
        })?.map_err(|err| anyhow::format_err!("{}", err))?;
        backend.submit(None, 1.0)?;
    }
}