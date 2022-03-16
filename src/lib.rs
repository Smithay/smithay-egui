#[deny(missing_docs)]
use egui::{epaint::ClippedMesh, Context, CtxRef, Event, Output, Pos2, RawInput, Rect, Vec2};

use smithay::{
    backend::{
        input::{Device, DeviceCapability, MouseButton},
        renderer::gles2::{Gles2Error, Gles2Frame, Gles2Renderer},
    },
    utils::{Logical, Physical, Point, Rectangle},
    wayland::seat::{KeysymHandle, ModifiersState},
};

#[cfg(feature = "render_element")]
use smithay::desktop::space::{RenderElement, RenderZindex, SpaceOutputTuple};

#[cfg(feature = "render_element")]
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

mod rendering;
mod text;
mod types;
pub use self::types::{convert_button, convert_key, convert_modifiers};

#[cfg(feature = "render_element")]
static EGUI_ID: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "render_element")]
lazy_static::lazy_static! {
    static ref EGUI_IDS: Mutex<HashSet<usize>> = Mutex::new(HashSet::new());
}
#[cfg(feature = "render_element")]
fn next_id() -> usize {
    let mut ids = EGUI_IDS.lock().unwrap();
    debug_assert!(ids.len() != usize::MAX);
    let mut id = EGUI_ID.fetch_add(1, Ordering::SeqCst);
    while ids.iter().any(|k| *k == id) {
        id = EGUI_ID.fetch_add(1, Ordering::SeqCst);
    }

    ids.insert(id);
    id
}

/// Enum representing egui render mode
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum EguiMode {
    /// In this mode `EguiFrame` reports damage only when a input is passed to `EguiState`
    /// or a widget uses `[CtxRef::request_repaint]`
    Reactive,
    /// In this mode `EguiFrame` reports full damage on every draw
    /// The full EguiState is redraw on every draw
    Continuous,
}

/// Global smithay-egui state
pub struct EguiState {
    #[cfg(feature = "render_element")]
    id: usize,
    ctx: CtxRef,
    pointers: usize,
    last_pointer_position: Point<i32, Logical>,
    events: Vec<Event>,
    kbd: Option<text::KbdInternal>,
    #[cfg(feature = "render_element")]
    z_index: u8,
    mode: EguiMode,
}

/// A single rendered egui interface frame
pub struct EguiFrame {
    #[cfg(feature = "render_element")]
    state_id: usize,
    ctx: CtxRef,
    output: Output,
    mesh: Vec<ClippedMesh>,
    scale: f64,
    #[cfg(feature = "render_element")]
    area: Rectangle<i32, Logical>,
    alpha: f32,
    #[cfg(feature = "render_element")]
    z_index: u8,
    mode: EguiMode,
    previous: Rect,
}

impl EguiState {
    /// Creates a new `EguiState`
    pub fn new(mode: EguiMode) -> EguiState {
        EguiState {
            #[cfg(feature = "render_element")]
            id: next_id(),
            ctx: CtxRef::default(),
            pointers: 0,
            last_pointer_position: (0, 0).into(),
            events: Vec::new(),
            kbd: match text::KbdInternal::new() {
                Some(kbd) => Some(kbd),
                None => {
                    eprintln!("Failed to initialize keymap for text input in egui.");
                    None
                }
            },
            #[cfg(feature = "render_element")]
            z_index: RenderZindex::Overlay as u8,
            mode,
        }
    }

    /// Retrieve the underlying [`egui::Context`]
    pub fn context(&self) -> &Context {
        &*self.ctx
    }

    /// If true, egui is currently listening on text input (e.g. typing text in a TextEdit).
    pub fn wants_keyboard(&self) -> bool {
        self.ctx.wants_keyboard_input()
    }

    /// True if egui is currently interested in the pointer (mouse or touch).
    /// Could be the pointer is hovering over a Window or the user is dragging a widget.
    /// If false, the pointer is outside of any egui area and so you may want to forward it to other clients as usual.
    /// Returns false if a drag started outside of egui and then moved over an egui area.
    pub fn wants_pointer(&self) -> bool {
        self.ctx.wants_pointer_input()
    }

    /// Pass new input devices to `EguiState` for internal tracking
    pub fn handle_device_added(&mut self, device: &impl Device) {
        if device.has_capability(DeviceCapability::Pointer) {
            self.pointers += 1;
        }
    }

    /// Remove input devices to `EguiState` for internal tracking
    pub fn handle_device_removed(&mut self, device: &impl Device) {
        if device.has_capability(DeviceCapability::Pointer) {
            self.pointers -= 1;
        }
        if self.pointers == 0 {
            self.events.push(Event::PointerGone);
        }
    }

    /// Pass keyboard events into `EguiState`.
    ///
    /// You do not want to pass in events, egui should not react to, but you need to make sure they add up.
    /// So for every pressed event, you want to send a released one.
    ///
    /// You likely want to use the filter-closure of [`smithay::wayland::seat::KeyboardHandle::input`] to optain these values.
    /// Use [`smithay::wayland::seat::KeysymHandle::raw_syms`] and the provided [`smithay::wayland::seat::ModifiersState`].
    pub fn handle_keyboard(
        &mut self,
        handle: &KeysymHandle,
        pressed: bool,
        modifiers: ModifiersState,
    ) {
        if let Some(key) = convert_key(handle.raw_syms().iter().copied()) {
            self.events.push(Event::Key {
                key,
                pressed,
                modifiers: convert_modifiers(modifiers),
            });
        }

        if let Some(kbd) = self.kbd.as_mut() {
            kbd.key_input(handle.raw_code(), pressed);

            if pressed {
                let utf8 = kbd.get_utf8(handle.raw_code());
                /* utf8 contains the utf8 string generated by that keystroke
                 * it can contain 1, multiple characters, or even be empty
                 */
                self.events.push(Event::Text(utf8));
            }
        }
    }

    /// Pass new pointer coordinates to `EguiState`
    pub fn handle_pointer_motion(&mut self, position: Point<i32, Logical>) {
        self.last_pointer_position = position;
        self.events.push(Event::PointerMoved(Pos2::new(
            position.x as f32,
            position.y as f32,
        )))
    }

    /// Pass pointer button presses to `EguiState`
    ///
    /// Note: If you are unsure about *which* PointerButtonEvents to send to smithay-egui
    ///       instead of normal clients, check [`EguiState::wants_pointer`] to figure out,
    ///       if there is an egui-element below your pointer.
    pub fn handle_pointer_button(
        &mut self,
        button: MouseButton,
        pressed: bool,
        modifiers: ModifiersState,
    ) {
        if let Some(button) = convert_button(button) {
            self.events.push(Event::PointerButton {
                pos: Pos2::new(
                    self.last_pointer_position.x as f32,
                    self.last_pointer_position.y as f32,
                ),
                button,
                pressed,
                modifiers: convert_modifiers(modifiers),
            })
        }
    }

    /// Pass a pointer axis scrolling to `EguiState`
    ///
    /// Note: If you are unsure about *which* PointerAxisEvents to send to smithay-egui
    ///       instead of normal clients, check [`EguiState::wants_pointer`] to figure out,
    ///       if there is an egui-element below your pointer.
    pub fn handle_pointer_axis(&mut self, x_amount: f64, y_amount: f64) {
        self.events.push(Event::Scroll(Vec2 {
            x: x_amount as f32,
            y: y_amount as f32,
        }))
    }

    // TODO: touch inputs

    /// Produce a new frame of egui to draw onto your output buffer.
    ///
    /// - `ui` is your drawing function
    /// - `area` limits the space egui will be using.
    /// - `size` has to be the total size of the buffer the ui will be displayed in
    /// - `scale` is the scale egui should render in
    /// - `start_time` need to be a fixed point in time before the first `run` call to measure animation-times and the like.
    /// - `modifiers` should be the current state of modifiers pressed on the keyboards.
    pub fn run(
        &mut self,
        ui: impl FnOnce(&CtxRef),
        area: Rectangle<i32, Logical>,
        scale: f64,
        alpha: f32,
        start_time: &std::time::Instant,
        modifiers: ModifiersState,
    ) -> EguiFrame {
        let previous = self.ctx.used_rect();

        let screen_size = area.to_f64().to_physical(scale).to_i32_round::<i32>().size;
        let input = RawInput {
            screen_rect: Some(Rect {
                min: Pos2 {
                    x: 0.0,
                    y: 0.0,
                },
                max: Pos2 {
                    x: screen_size.w as f32,
                    y: screen_size.h as f32,
                },
            }),
            pixels_per_point: Some(scale as f32),
            time: Some(start_time.elapsed().as_secs_f64()),
            predicted_dt: 1.0 / 60.0,
            modifiers: convert_modifiers(modifiers),
            events: self.events.drain(..).collect(),
            hovered_files: Vec::with_capacity(0),
            dropped_files: Vec::with_capacity(0),
        };

        let (output, shapes) = self.ctx.run(input, ui);
        EguiFrame {
            #[cfg(feature = "render_element")]
            state_id: self.id,
            ctx: self.ctx.clone(),
            output,
            mesh: self.ctx.tessellate(shapes),
            scale,
            #[cfg(feature = "render_element")]
            area,
            alpha,
            #[cfg(feature = "render_element")]
            z_index: self.z_index,
            mode: self.mode,
            previous,
        }
    }

    /// Sets the z_index that is used by future `EguiFrame`s produced by this states
    /// [`EguiState::run`], when used as a `RenderElement`.
    #[cfg(feature = "render_element")]
    pub fn set_zindex(&mut self, index: u8) {
        self.z_index = index;
    }

    /// Sets the drawing mode of EguiState, refer to `EguiMode`
    pub fn set_mode(&mut self, mode: EguiMode) {
        self.mode = mode;
    }
}

impl EguiFrame {
    /// Draw this frame in the currently active GL-context
    pub unsafe fn draw(
        &self,
        r: &mut Gles2Renderer,
        frame: &Gles2Frame,
        location: Point<i32, Logical>,
        render_scale: f64,
        damage: &[Rectangle<i32, Logical>],
    ) -> Result<(), Gles2Error> {
        use rendering::GlState;

        let user_data = r.egl_context().user_data();
        if user_data.get::<GlState>().is_none() {
            let state = GlState::new(r, self.ctx.font_image())?;
            r.egl_context().user_data().insert_if_missing(|| state);
        }

        r.with_context(|r, gl| unsafe {
            let state = r.egl_context().user_data().get::<GlState>().unwrap();

            state.paint_meshes(
                frame,
                gl,
                location.to_f64().to_physical(self.scale).to_i32_round(),
                self.area.size.to_f64().to_physical(render_scale).to_i32_round(),
                1.0 / self.scale * render_scale,
                &damage
                    .into_iter()
                    .map(|rect| rect.to_f64().to_physical(self.scale).to_i32_round())
                    .collect::<Vec<_>>(),
                self.mesh.iter().cloned(),
                self.alpha,
            )
        })
        .and_then(std::convert::identity)
    }

    pub fn geometry(&self) -> Rectangle<i32, Logical> {
        self.area
    }
}

#[cfg(feature = "render_element")]
impl RenderElement<Gles2Renderer> for EguiFrame {
    fn id(&self) -> usize {
        self.state_id
    }

    fn geometry(&self) -> Rectangle<i32, Logical> {
        EguiFrame::geometry(self)
    }

    fn accumulated_damage(
        &self,
        _for_values: Option<SpaceOutputTuple<'_, '_>>,
    ) -> Vec<Rectangle<i32, Logical>> {
        if self.mode == EguiMode::Reactive && !self.output.needs_repaint {
            vec![]
        } else {
            let used = self.ctx.used_rect().union(self.previous);
            let margin = self.ctx.style().visuals.clip_rect_margin as f64;
            let window_shadow = self.ctx.style().visuals.window_shadow.extrusion as f64;
            let popup_shadow = self.ctx.style().visuals.popup_shadow.extrusion as f64;
            let offset = margin + window_shadow.max(popup_shadow);
            vec![
                Rectangle::<f64, Physical>::from_extemities(
                    (used.min.x as f64 - offset, used.min.y as f64 - offset),
                    (used.max.x as f64 + (offset * 2.0), used.max.y as f64 + (offset * 2.0))
                ).to_logical(self.scale)
                .to_i32_round()
            ]
        }
    }

    fn draw(
        &self,
        renderer: &mut Gles2Renderer,
        frame: &mut Gles2Frame,
        scale: f64,
        location: Point<i32, Logical>,
        damage: &[Rectangle<i32, Logical>],
        log: &slog::Logger,
    ) -> Result<(), Gles2Error> {
        if let Err(err) = unsafe {
            EguiFrame::draw(
                self,
                renderer,
                frame,
                location,
                scale,
                damage,
            )
        } {
            slog::error!(log, "egui rendering error: {}", err);
        }
        Ok(())
    }

    fn z_index(&self) -> u8 {
        self.z_index
    }
}
