use egui::{CtxRef, Context, Event, RawInput, Rect, Pos2, Vec2, Output, epaint::ClippedMesh};
use egui_glow::{glow::{self, Context as GlowContext, HasContext}, painter::Painter};

use smithay::{
    backend::{
        renderer::gles2::{Gles2Renderer, Gles2Frame},
        input::{InputBackend, Device, DeviceCapability, MouseButton},
    },
    utils::{Size, Rectangle, Logical, Physical},
    wayland::seat::{Keysym, ModifiersState},
};

#[cfg(feature = "render_element")]
use smithay::{
    backend::renderer::gles2::{Gles2Error, Gles2Texture},
    desktop::{
        space::RenderElement,
        Space,
    },
    utils::Point,
    wayland::output::Output as WlOutput,
};

#[cfg(feature = "render_element")]
use std::{
    collections::HashSet,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    }
};

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
    debug_assert!(!ids.len() == usize::MAX);
    let mut id = EGUI_ID.fetch_add(1, Ordering::SeqCst);
    while ids.iter().any(|k| *k == id) {
        id = EGUI_ID.fetch_add(1, Ordering::SeqCst);
    }

    ids.insert(id);
    id
}    

pub struct EguiState {
    id: usize,
    ctx: CtxRef,
    pointers: usize,
    last_pointer_position: Point<i32, Logical>,
    events: Vec<Event>,
}

pub struct EguiFrame {
    state_id: usize,
    ctx: CtxRef,
    _output: Output,
    mesh: Vec<ClippedMesh>,
    scale: f64,
    area: Rect,
    size: Size<i32, Physical>,
}

impl EguiState {
    pub fn new() -> EguiState {
        EguiState {
            id: next_id(),
            ctx: CtxRef::default(),
            pointers: 0,
            last_pointer_position: (0, 0).into(),
            events: Vec::new(),
        }
    }

    pub fn context(&self) -> &Context {
        &*self.ctx
    }

    pub fn wants_keyboard(&self) -> bool {
        self.ctx.wants_keyboard_input()
    }

    pub fn wants_pointer(&self) -> bool {
        self.ctx.wants_pointer_input()
    }

    pub fn handle_device_added(&mut self, device: &impl Device) {
        if device.has_capability(DeviceCapability::Pointer) {
            self.pointers += 1;
        }
    }

    pub fn handle_device_removed(&mut self, device: &impl Device) {
        if device.has_capability(DeviceCapability::Pointer) {
            self.pointers -= 1;
        }
        if self.pointers == 0 {
            self.events.push(Event::PointerGone);
        }
    }

    pub fn handle_keyboard<B: InputBackend>(&mut self, raw_syms: &[Keysym], pressed: bool, modifiers: ModifiersState) {
        if let Some(key) = convert_key(raw_syms.iter().copied()) {
            self.events.push(Event::Key {
                key,
                pressed,
                modifiers: convert_modifiers(modifiers), 
            });
        }
    }

    pub fn handle_pointer_motion<B: InputBackend>(&mut self, position: Point<i32, Logical>) {
        self.last_pointer_position = position;
        self.events.push(Event::PointerMoved(Pos2::new(position.x as f32, position.y as f32)))
    }

    pub fn handle_pointer_button<B: InputBackend>(&mut self, button: MouseButton, pressed: bool, modifiers: ModifiersState) {
        if let Some(button) = convert_button(button) {
            self.events.push(Event::PointerButton {
                pos: Pos2::new(self.last_pointer_position.x as f32, self.last_pointer_position.y as f32),
                button,
                pressed,
                modifiers: convert_modifiers(modifiers),
            })
        }
    }

    pub fn handle_pointer_axis<B: InputBackend>(&mut self, x_amount: f64, y_amount: f64) {
        self.events.push(Event::Scroll(Vec2 {
            x: x_amount as f32,
            y: y_amount as f32,
        }))
    }

    // TODO: touch inputs

    pub fn run(
        &mut self,
        ui: impl FnOnce(&CtxRef),
        area: Rectangle<i32, Logical>,
        size: Size<i32, Physical>,
        scale: f64,
        start_time: &std::time::Instant,
        modifiers: ModifiersState,
    ) -> EguiFrame
    {
        let area = area.to_f64().to_physical(scale).to_i32_round::<i32>();
        let input = RawInput {
            screen_rect: Some(Rect {
                min: Pos2 {
                    x: area.loc.x as f32,
                    y: area.loc.y as f32,
                },
                max: Pos2 {
                    x: area.loc.x as f32 + area.size.w as f32,
                    y: area.loc.y as f32 + area.size.h as f32,
                }
            }),
            pixels_per_point: Some(scale as f32),
            time: Some(start_time.elapsed().as_secs_f64()),
            predicted_dt: 1.0/60.0,
            modifiers: convert_modifiers(modifiers),
            events: self.events.drain(..).collect(),
            hovered_files: Vec::with_capacity(0),
            dropped_files: Vec::with_capacity(0),
        };

        let (_output, shapes) = self.ctx.run(input, ui);
        EguiFrame {
            state_id: self.id,
            ctx: self.ctx.clone(),
            _output,
            mesh: self.ctx.tessellate(shapes),
            scale,
            area: self.ctx.used_rect(),
            size,
        }
    }
}

impl EguiFrame {
    pub fn draw(
        &self,
    ) -> Result<(), String> {
        // TODO: cache this somehow
        let context = unsafe { GlowContext::from_loader_function(|sym | smithay::backend::egl::get_proc_address(sym)) };
        let mut painter = Painter::new(&context, None, "")?;
        painter.upload_egui_texture(&context, &*self.ctx.font_image());

        painter.paint_meshes(&context, [self.size.w as u32, self.size.h as u32], self.scale as f32, self.mesh.clone());

        unsafe {
            context.disable(glow::SCISSOR_TEST);
            context.disable(glow::BLEND);
        }
        painter.destroy(&context);

        Ok(())
    }
}

#[cfg(feature = "render_element")]
impl RenderElement<Gles2Renderer, Gles2Frame, Gles2Error, Gles2Texture> for EguiFrame {
    fn id(&self) -> usize {
        self.state_id
    }

    fn geometry(&self) -> Rectangle<i32, Logical> {
        Rectangle::<f64, Physical>::from_extemities(
            (self.area.min.x as f64, self.area.min.y as f64),
            (self.area.max.x as f64, self.area.max.y as f64)
        )
        .to_logical(self.scale)
        .to_i32_round()
    }

    fn accumulated_damage(&self, _for_values: Option<(&Space, &WlOutput)>) -> Vec<Rectangle<i32, Logical>> {
        vec![Rectangle::from_loc_and_size((0, 0), (i32::MAX, i32::MAX))]
    }

    fn draw(
        &self,
        _renderer: &mut Gles2Renderer,
        _frame: &mut Gles2Frame,
        _scale: f64,
        _location: Point<i32, Logical>,
        _damage: &[Rectangle<i32, Logical>],
        log: &slog::Logger
    ) -> Result<(), Gles2Error> {
        if let Err(err) = EguiFrame::draw(self) {
            slog::error!(log, "egui rendering error: {}", err);
        }
        Ok(())
    }
}