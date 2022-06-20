use cgmath::Matrix3;
use egui::{
    epaint::{
        image::{ImageData, ImageDelta},
        Mesh, Primitive, Vertex,
    },
    ClippedPrimitive, TextureId,
};
use smithay::{
    backend::renderer::{
        gles2::{ffi, Gles2Error, Gles2Frame, Gles2Renderer},
        Frame,
    },
    utils::{Physical, Point, Rectangle, Scale, Size},
};
use std::{collections::HashMap, ffi::CStr, os::raw::c_char};

mod shaders;

pub struct GlState {
    program: EguiProgram,
    egui_textures: HashMap<u64, (ffi::types::GLuint, [u32; 2])>,
    vertex_buffer: ffi::types::GLuint,
    element_array_buffer: ffi::types::GLuint,
    vertex_array: ffi::types::GLuint,
}

impl GlState {
    pub fn new(renderer: &mut Gles2Renderer, _scale: f64) -> Result<GlState, Gles2Error> {
        renderer
            .with_context(|_, gl| unsafe {
                let ext_ptr = gl.GetString(ffi::EXTENSIONS) as *const c_char;
                if ext_ptr.is_null() {
                    return Err(Gles2Error::GLFunctionLoaderError);
                }

                let exts = {
                    let p = CStr::from_ptr(ext_ptr);
                    let list =
                        String::from_utf8(p.to_bytes().to_vec()).unwrap_or_else(|_| String::new());
                    list.split(' ').map(|e| e.to_string()).collect::<Vec<_>>()
                };

                let mut version = 1;
                gl.GetIntegerv(ffi::MAJOR_VERSION, &mut version as *mut _);

                // required for egui
                if version < 3 && !exts.iter().any(|ext| ext == "GL_EXT_sRGB") {
                    return Err(Gles2Error::GLExtensionNotSupported(&["GL_EXT_sRGB"]));
                }
                // required for simplified implementation.
                // Could be worked around, if deemed necessary.
                if !exts.iter().any(|ext| ext == "GL_OES_vertex_array_object") {
                    return Err(Gles2Error::GLExtensionNotSupported(&[
                        "GL_OES_vertex_array_object",
                    ]));
                }

                let program = program(gl)?;

                let mut buffers = [0u32; 2];
                let mut vertex_array = 0u32;
                gl.GenBuffers(2, &mut buffers as *mut _);
                gl.GenVertexArrays(1, &mut vertex_array as *mut _);

                gl.BindBuffer(ffi::ARRAY_BUFFER, buffers[0]);
                gl.BindVertexArray(vertex_array);

                let stride = std::mem::size_of::<Vertex>() as i32;
                gl.VertexAttribPointer(
                    program.a_pos as u32,
                    2,
                    ffi::FLOAT,
                    false as u8,
                    stride,
                    memoffset::offset_of!(Vertex, pos) as *const _,
                );
                gl.EnableVertexAttribArray(program.a_pos as u32);
                gl.VertexAttribPointer(
                    program.a_tc as u32,
                    2,
                    ffi::FLOAT,
                    false as u8,
                    stride,
                    memoffset::offset_of!(Vertex, uv) as *const _,
                );
                gl.EnableVertexAttribArray(program.a_tc as u32);
                gl.VertexAttribPointer(
                    program.a_srgba as u32,
                    4,
                    ffi::UNSIGNED_BYTE,
                    false as u8,
                    stride,
                    memoffset::offset_of!(Vertex, color) as *const _,
                );
                gl.EnableVertexAttribArray(program.a_srgba as u32);

                Ok(GlState {
                    program,
                    vertex_buffer: buffers[0],
                    element_array_buffer: buffers[1],
                    vertex_array,
                    egui_textures: HashMap::new(),
                })
            })
            .and_then(std::convert::identity)
    }

    pub unsafe fn upload_textures<'a>(
        &mut self,
        gl: &ffi::Gles2,
        new_textures: impl Iterator<Item = (&'a TextureId, &'a ImageDelta)>,
    ) -> Result<(), Gles2Error> {
        for (id, delta) in new_textures {
            if let TextureId::Managed(id) = id {
                let pixels: Vec<u8> = match delta.image {
                    ImageData::Color(ref image) => image
                        .pixels
                        .iter()
                        .flat_map(|a| Vec::from(a.to_array()))
                        .collect(),
                    ImageData::Font(ref image) => image
                        .srgba_pixels(1.0)
                        .flat_map(|a| Vec::from(a.to_array()))
                        .collect(),
                };

                let (t_x, t_y, t_w, t_h) = (
                    delta.pos.map(|[x, _]| x as u32).unwrap_or(0),
                    delta.pos.map(|[_, y]| y as u32).unwrap_or(0),
                    delta.image.width() as u32,
                    delta.image.height() as u32,
                );
                let tex = match self.egui_textures.get(&id) {
                    Some((tex, [w, h])) if *w >= t_x + t_w && *h >= t_y + t_h => *tex,
                    x => {
                        if let Some((tex, _)) = x {
                            gl.DeleteTextures(1, tex);
                        }
                        let mut tex = 0;
                        gl.GenTextures(1, &mut tex);
                        gl.BindTexture(ffi::TEXTURE_2D, tex);
                        gl.TexParameteri(
                            ffi::TEXTURE_2D,
                            ffi::TEXTURE_MAG_FILTER,
                            ffi::LINEAR as i32,
                        );
                        gl.TexParameteri(
                            ffi::TEXTURE_2D,
                            ffi::TEXTURE_MIN_FILTER,
                            ffi::LINEAR as i32,
                        );
                        gl.TexParameteri(
                            ffi::TEXTURE_2D,
                            ffi::TEXTURE_WRAP_S,
                            ffi::CLAMP_TO_EDGE as i32,
                        );
                        gl.TexParameteri(
                            ffi::TEXTURE_2D,
                            ffi::TEXTURE_WRAP_T,
                            ffi::CLAMP_TO_EDGE as i32,
                        );
                        gl.TexStorage2D(
                            ffi::TEXTURE_2D,
                            1,
                            ffi::SRGB8_ALPHA8,
                            delta.image.width() as i32,
                            delta.image.height() as i32,
                        );
                        tex
                    }
                };
                gl.TexSubImage2D(
                    ffi::TEXTURE_2D,
                    0,
                    t_x as i32,
                    t_y as i32,
                    t_w as i32,
                    t_h as i32,
                    ffi::RGBA,
                    ffi::UNSIGNED_BYTE,
                    pixels.as_ptr() as *const _,
                );
                self.egui_textures.insert(*id, (tex, [t_w, t_h]));
            }
        }
        Ok(())
    }

    pub unsafe fn free_textures(
        &mut self,
        gl: &ffi::Gles2,
        free_textures: impl Iterator<Item = TextureId>,
    ) -> Result<(), Gles2Error> {
        for id in free_textures {
            if let TextureId::Managed(id) = id {
                if let Some((tex, _)) = self.egui_textures.remove(&id) {
                    gl.DeleteTextures(1, &tex);
                }
            }
        }
        Ok(())
    }

    pub unsafe fn paint_meshes(
        &self,
        frame: &Gles2Frame,
        gl: &ffi::Gles2,
        location: Point<i32, Physical>,
        area: Size<i32, Physical>,
        scale: Scale<f64>,
        damage: &[Rectangle<i32, Physical>],
        clipped_meshes: impl Iterator<Item = ClippedPrimitive>,
        alpha: f32,
    ) -> Result<(), Gles2Error> {
        gl.Enable(ffi::SCISSOR_TEST);
        gl.Disable(ffi::CULL_FACE);

        gl.Enable(ffi::BLEND);
        gl.BlendEquation(ffi::FUNC_ADD);
        gl.BlendFuncSeparate(
            ffi::ONE,
            ffi::ONE_MINUS_SRC_ALPHA,
            ffi::ONE_MINUS_DST_ALPHA,
            ffi::ONE,
        );

        gl.UseProgram(self.program.program);

        let projection = Into::<&'_ Matrix3<f32>>::into(frame.projection());
        let matrix: Matrix3<f32> = projection
            * Matrix3::from_translation([location.x as f32, location.y as f32].into())
            * Matrix3::from_nonuniform_scale(scale.x as f32, scale.y as f32);
        let matrix_ref: &[f32; 9] = matrix.as_ref();
        gl.UniformMatrix3fv(self.program.u_matrix, 1, ffi::FALSE, matrix_ref.as_ptr());
        gl.Uniform1f(self.program.u_alpha, alpha);
        gl.Uniform1i(self.program.u_sampler, 0);
        gl.ActiveTexture(ffi::TEXTURE0);
        gl.BindVertexArray(self.vertex_array);

        // merge overlapping rectangles
        let damage: Vec<Rectangle<i32, Physical>> =
            damage
                .into_iter()
                .cloned()
                .fold(Vec::new(), |new_damage, mut rect| {
                    // replace with drain_filter, when that becomes stable to reuse the original Vec's memory
                    let (overlapping, mut new_damage): (Vec<_>, Vec<_>) = new_damage
                        .into_iter()
                        .partition(|other| other.overlaps(rect));

                    for overlap in overlapping {
                        rect = rect.merge(overlap);
                    }
                    new_damage.push(rect);
                    new_damage
                });

        for ClippedPrimitive {
            clip_rect,
            primitive,
        } in clipped_meshes
        {
            let clip_rectangle = Rectangle::<f64, Physical>::from_extemities(
                (clip_rect.min.x as f64, clip_rect.min.y as f64),
                (clip_rect.max.x as f64, clip_rect.max.y as f64),
            )
            .to_f64();

            for damage in damage
                .iter()
                .map(|d| d.to_f64())
                .filter(|d| d.overlaps(clip_rectangle))
            {
                let mut scissor_box: Rectangle<i32, Physical> = clip_rectangle
                    .intersection(damage)
                    .unwrap()
                    .to_logical(1.0)
                    .to_physical(scale)
                    .to_i32_round();
                scissor_box = frame.transformation().transform_rect_in(scissor_box, &area);
                scissor_box.loc += location;

                gl.Scissor(
                    std::cmp::max(scissor_box.loc.x, 0),
                    std::cmp::max(scissor_box.loc.y, 0),
                    std::cmp::max(scissor_box.size.w, 0),
                    std::cmp::max(scissor_box.size.h, 0),
                );
                match primitive {
                    Primitive::Mesh(ref mesh) => self.paint_mesh(gl, mesh)?,
                    Primitive::Callback(_callback) => {
                        unimplemented!()
                        /*
                        callback.call(&PaintCallbackInfo {
                            viewport: Rect {
                                min: Pos2 { x: location.x as f32, y: location.y as f32 },
                                max: Pos2 { x: (location.x + area.w) as f32, y: (location.y + area.h) as f32 },
                            },
                            clip_rect,
                            pixels_per_point: self.scale as f32,
                            screen_size_px: [area.w as u32, area.h as u32],
                        }, gl as &mut dyn std::any::Any)
                        */
                    }
                }
            }
        }

        gl.BindVertexArray(0);
        gl.BindBuffer(ffi::ELEMENT_ARRAY_BUFFER, 0);
        gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
        gl.Disable(ffi::SCISSOR_TEST);

        Ok(())
    }

    unsafe fn paint_mesh(&self, gl: &ffi::Gles2, mesh: &Mesh) -> Result<(), Gles2Error> {
        let texture = match mesh.texture_id {
            TextureId::Managed(id) => {
                self.egui_textures
                    .get(&id)
                    .expect("Unknown managed texture id")
                    .0
            }
            TextureId::User(tex) => tex as u32,
        };
        gl.BindTexture(ffi::TEXTURE_2D, texture);

        gl.BindBuffer(ffi::ARRAY_BUFFER, self.vertex_buffer);
        gl.BufferData(
            ffi::ARRAY_BUFFER,
            (mesh.vertices.len() * std::mem::size_of::<Vertex>()) as isize,
            mesh.vertices.as_ptr() as *const _,
            ffi::STREAM_DRAW,
        );

        gl.BindBuffer(ffi::ELEMENT_ARRAY_BUFFER, self.element_array_buffer);
        gl.BufferData(
            ffi::ELEMENT_ARRAY_BUFFER,
            (mesh.indices.len() * std::mem::size_of::<u32>()) as isize,
            mesh.indices.as_ptr() as *const _,
            ffi::STREAM_DRAW,
        );
        gl.DrawElements(
            ffi::TRIANGLES,
            mesh.indices.len() as i32,
            ffi::UNSIGNED_INT,
            std::ptr::null(),
        );

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct EguiProgram {
    program: ffi::types::GLuint,
    u_matrix: ffi::types::GLint,
    u_sampler: ffi::types::GLint,
    u_alpha: ffi::types::GLint,
    a_pos: ffi::types::GLint,
    a_tc: ffi::types::GLint,
    a_srgba: ffi::types::GLint,
}

unsafe fn compile_shader(
    gl: &ffi::Gles2,
    variant: ffi::types::GLuint,
    src: &'static str,
) -> Result<ffi::types::GLuint, Gles2Error> {
    let shader = gl.CreateShader(variant);
    gl.ShaderSource(
        shader,
        1,
        &src.as_ptr() as *const *const u8 as *const *const ffi::types::GLchar,
        &(src.len() as i32) as *const _,
    );
    gl.CompileShader(shader);

    let mut status = ffi::FALSE as i32;
    gl.GetShaderiv(shader, ffi::COMPILE_STATUS, &mut status as *mut _);
    if status == ffi::FALSE as i32 {
        gl.DeleteShader(shader);
        return Err(Gles2Error::ShaderCompileError(src));
    }

    Ok(shader)
}

unsafe fn link_program(
    gl: &ffi::Gles2,
    vert_src: &'static str,
    frag_src: &'static str,
) -> Result<ffi::types::GLuint, Gles2Error> {
    let vert = compile_shader(gl, ffi::VERTEX_SHADER, vert_src)?;
    let frag = compile_shader(gl, ffi::FRAGMENT_SHADER, frag_src)?;
    let program = gl.CreateProgram();
    gl.AttachShader(program, vert);
    gl.AttachShader(program, frag);
    gl.LinkProgram(program);
    gl.DetachShader(program, vert);
    gl.DetachShader(program, frag);
    gl.DeleteShader(vert);
    gl.DeleteShader(frag);

    let mut status = ffi::FALSE as i32;
    gl.GetProgramiv(program, ffi::LINK_STATUS, &mut status as *mut _);
    if status == ffi::FALSE as i32 {
        gl.DeleteProgram(program);
        return Err(Gles2Error::ProgramLinkError);
    }

    Ok(program)
}

unsafe fn program(gl: &ffi::Gles2) -> Result<EguiProgram, Gles2Error> {
    let program = link_program(gl, shaders::VERTEX_SHADER, shaders::FRAGMENT_SHADER)?;

    let u_matrix = CStr::from_bytes_with_nul(b"u_matrix\0").expect("NULL terminated");
    let u_sampler = CStr::from_bytes_with_nul(b"u_sampler\0").expect("NULL terminated");
    let u_alpha = CStr::from_bytes_with_nul(b"u_alpha\0").expect("NULL terminated");
    let a_pos = CStr::from_bytes_with_nul(b"a_pos\0").expect("NULL terminated");
    let a_tc = CStr::from_bytes_with_nul(b"a_tc\0").expect("NULL terminated");
    let a_srgba = CStr::from_bytes_with_nul(b"a_srgba\0").expect("NULL terminated");

    Ok(EguiProgram {
        program,
        u_matrix: gl.GetUniformLocation(program, u_matrix.as_ptr() as *const ffi::types::GLchar),
        u_sampler: gl.GetUniformLocation(program, u_sampler.as_ptr() as *const ffi::types::GLchar),
        u_alpha: gl.GetUniformLocation(program, u_alpha.as_ptr() as *const ffi::types::GLchar),
        a_pos: gl.GetAttribLocation(program, a_pos.as_ptr() as *const ffi::types::GLchar),
        a_tc: gl.GetAttribLocation(program, a_tc.as_ptr() as *const ffi::types::GLchar),
        a_srgba: gl.GetAttribLocation(program, a_srgba.as_ptr() as *const ffi::types::GLchar),
    })
}
