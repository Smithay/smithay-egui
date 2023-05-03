use anyhow::Result;
use smithay::{
    backend::{
        renderer::{
            element::{texture::TextureRenderElement, Element, RenderElement},
            gles::GlesTexture,
            glow::GlowRenderer,
            Frame, Renderer,
        },
        winit,
    },
    input::{
        keyboard::{FilterResult, XkbConfig},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
        SeatHandler, SeatState,
    },
    utils::{Rectangle, Transform, SERIAL_COUNTER},
};
use smithay_egui::EguiState;

// This example provides a minimal example to:
// - Setup and `Renderer` and get `InputEvents` via winit.
// - Pass those input events to egui
// - Render an egui interface using that renderer.

// It does not show of the best practices to do so,
// neither does the example show-case a real wayland compositor.
// For that take a look into [`anvil`](https://github.com/Smithay/smithay/tree/master/anvil).

// This is only meant to provide a starting point to integrate egui into an already existing compositor

struct State(SeatState<State>);
impl SeatHandler for State {
    type KeyboardFocus = EguiState;
    type PointerFocus = EguiState;
    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.0
    }
}

fn main() -> Result<()> {
    // setup logger
    tracing_subscriber::fmt().compact().init();
    // create a winit-backend
    let (mut backend, mut input) =
        winit::init::<GlowRenderer>().map_err(|_| anyhow::anyhow!("Winit failed to start"))?;
    // create an `EguiState`. Usually this would be part of your global smithay state
    let egui = EguiState::new(Rectangle::from_loc_and_size(
        (0, 0),
        backend.window_size().physical_size.to_logical(1),
    ));
    // you might also need additional structs to store your ui-state, like the demo_lib does
    let mut demo_ui = egui_demo_lib::DemoWindows::default();

    let mut seat_state = SeatState::new();
    let mut seat = seat_state.new_seat("seat-0");
    let mut state = State(seat_state);
    let keyboard = seat.add_keyboard(XkbConfig::default(), 200, 25)?;
    keyboard.set_focus(&mut state, Some(egui.clone()), SERIAL_COUNTER.next_serial());
    let pointer = seat.add_pointer();

    loop {
        input.dispatch_new_events(|event| {
            use smithay::backend::{
                input::{
                    AbsolutePositionEvent, Axis, AxisSource, Event, InputEvent, KeyboardKeyEvent,
                    PointerAxisEvent, PointerButtonEvent,
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
                            &mut state,
                            event.key_code(),
                            event.state(),
                            SERIAL_COUNTER.next_serial(),
                            event.time_msec(),
                            |_data, _modifiers, _handle| FilterResult::Forward,
                        )
                        .unwrap_or(()),
                    // Winit only produces `PointerMotionAbsolute` events, but a real compositor needs to handle this for `PointerMotion` events as well.
                    // Meaning: you need to compute the absolute position and pass that to egui.
                    InputEvent::PointerMotionAbsolute { event } => {
                        let pos = event.position();
                        pointer.motion(
                            &mut state,
                            Some((egui.clone(), (0, 0).into())),
                            &MotionEvent {
                                location: (pos.x, pos.y).into(),
                                serial: SERIAL_COUNTER.next_serial(),
                                time: event.time_msec(),
                            },
                        );
                    }
                    // NOTE: you should check with `EguiState::wwants_pointer`, if the pointer is above any egui element before forwarding it.
                    // Otherwise forward it to clients as usual.
                    InputEvent::PointerButton { event } => pointer.button(
                        &mut state,
                        &ButtonEvent {
                            button: event.button_code(),
                            state: event.state().into(),
                            serial: SERIAL_COUNTER.next_serial(),
                            time: event.time_msec(),
                        },
                    ),
                    // NOTE: you should check with `EguiState::wwants_pointer`, if the pointer is above any egui element before forwarding it.
                    // Otherwise forward it to clients as usual.
                    InputEvent::PointerAxis { event } => {
                        let horizontal_amount =
                            event.amount(Axis::Horizontal).unwrap_or_else(|| {
                                event.amount_discrete(Axis::Horizontal).unwrap_or(0.0) * 3.0
                            });
                        let vertical_amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
                            event.amount_discrete(Axis::Vertical).unwrap_or(0.0) * 3.0
                        });
                        let horizontal_amount_discrete = event.amount_discrete(Axis::Horizontal);
                        let vertical_amount_discrete = event.amount_discrete(Axis::Vertical);

                        {
                            let mut frame =
                                AxisFrame::new(event.time_msec()).source(event.source());
                            if horizontal_amount != 0.0 {
                                frame = frame.value(Axis::Horizontal, horizontal_amount);
                                if let Some(discrete) = horizontal_amount_discrete {
                                    frame = frame.discrete(Axis::Horizontal, discrete as i32);
                                }
                            } else if event.source() == AxisSource::Finger {
                                frame = frame.stop(Axis::Horizontal);
                            }
                            if vertical_amount != 0.0 {
                                frame = frame.value(Axis::Vertical, vertical_amount);
                                if let Some(discrete) = vertical_amount_discrete {
                                    frame = frame.discrete(Axis::Vertical, discrete as i32);
                                }
                            } else if event.source() == AxisSource::Finger {
                                frame = frame.stop(Axis::Vertical);
                            }
                            pointer.axis(&mut state, frame);
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        })?;

        let size = backend.window_size().physical_size;
        // Here we compute the rendered egui frame
        let egui_frame: TextureRenderElement<GlesTexture> = egui
            .render(
                |ctx| demo_ui.ui(ctx),
                backend.renderer(),
                // Just render it over the whole window, but you may limit the area
                Rectangle::from_loc_and_size((0, 0), size.to_logical(1)),
                // we also completely ignore the scale *everywhere* in this example, but egui is HiDPI-ready
                1.0,
                1.0,
            )
            .expect("Failed to render egui");

        // Lastly put the rendered frame on the screen
        backend.bind()?;
        let renderer = backend.renderer();
        {
            let mut frame = renderer.render(size, Transform::Flipped180)?;
            frame.clear(
                [1.0, 1.0, 1.0, 1.0],
                &[Rectangle::from_loc_and_size((0, 0), size)],
            )?;
            RenderElement::<GlowRenderer>::draw(
                &egui_frame,
                &mut frame,
                egui_frame.src(),
                egui_frame.geometry(1.0.into()),
                &[Rectangle::from_loc_and_size((0, 0), size)],
            )?;
        }
        backend.submit(None)?;
    }
}
