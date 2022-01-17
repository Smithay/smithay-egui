use smithay::{
    backend::renderer::gles2::{
        ffi,
        Gles2Renderer,
        Gles2Frame,
        Gles2Error,
    },
    utils::{Rectangle, Size, Physical},
};
use egui::{epaint::{FontImage, Rect, Mesh, Vertex}, ClippedMesh, TextureId};
use std::{
    ffi::CStr,
    sync::Arc,
    os::raw::c_char,
};

mod shaders;

pub struct GlState {
    program: EguiProgram,
    egui_texture: ffi::types::GLuint,
    vertex_buffer: ffi::types::GLuint,
    element_array_buffer: ffi::types::GLuint,
    vertex_array: ffi::types::GLuint,
}

impl GlState {
    pub fn new(renderer: &mut Gles2Renderer, font_image: Arc<FontImage>) -> Result<GlState, Gles2Error> {
        renderer.with_context(|_, gl| unsafe {
            let ext_ptr = gl.GetString(ffi::EXTENSIONS) as *const c_char;
            if ext_ptr.is_null() {
                return Err(Gles2Error::GLFunctionLoaderError);
            }

            let exts = {
                let p = CStr::from_ptr(ext_ptr);
                let list = String::from_utf8(p.to_bytes().to_vec()).unwrap_or_else(|_| String::new());
                list.split(' ').map(|e| e.to_string()).collect::<Vec<_>>()
            };
            
            let mut version = 1;
            gl.GetIntegerv(ffi::MAJOR_VERSION, &mut version as *mut _);

            // required for egui
            if version < 3 && !exts.iter().any(|ext| ext == "GL_EXT_sRGB") {
                return Err(Gles2Error::GLExtensionNotSupported(&[
                    "GL_EXT_sRGB",
                ]));
            }
            // required for simplified implementation.
            // Could be worked around, if deemed necessary.
            if !exts.iter().any(|ext| ext == "GL_OES_vertex_array_object") {
                return Err(Gles2Error::GLExtensionNotSupported(&[
                    "GL_OES_vertex_array_object",
                ]))
            }

            let program = program(gl)?;

            let pixels: Vec<u8> = font_image
                .srgba_pixels(1.0)
                .flat_map(|a| Vec::from(a.to_array()))
                .collect();

            let mut tex = 0;
            gl.GenTextures(1, &mut tex);
            gl.BindTexture(ffi::TEXTURE_2D, tex);
            gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MAG_FILTER, ffi::LINEAR as i32);
            gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MIN_FILTER, ffi::LINEAR as i32);
            gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_WRAP_S, ffi::CLAMP_TO_EDGE as i32);
            gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_WRAP_T, ffi::CLAMP_TO_EDGE as i32);
            gl.TexStorage2D(ffi::TEXTURE_2D, 1, ffi::SRGB8_ALPHA8, font_image.width as i32, font_image.height as i32);
            gl.TexSubImage2D(
                ffi::TEXTURE_2D,
                0,
                0,
                0,
                font_image.width as i32,
                font_image.height as i32,
                ffi::RGBA,
                ffi::UNSIGNED_BYTE,
                pixels.as_ptr() as *const _,
            );

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
                egui_texture: tex,
                vertex_buffer: buffers[0],
                element_array_buffer: buffers[1],
                vertex_array,
            })
        }).and_then(std::convert::identity)
    }

    pub unsafe fn paint_meshes(
        &self,
        frame: &Gles2Frame,
        gl: &ffi::Gles2,
        size: Size<i32, Physical>,
        scale: f64,
        clipped_meshes: impl Iterator<Item=ClippedMesh>,
        alpha: f32,
    ) -> Result<(), Gles2Error> {
        gl.Enable(ffi::SCISSOR_TEST);
        gl.Disable(ffi::CULL_FACE);

        gl.Enable(ffi::BLEND);
        gl.BlendEquation(ffi::FUNC_ADD);
        gl.BlendFuncSeparate(
            ffi::ONE, ffi::ONE_MINUS_SRC_ALPHA,
            ffi::ONE_MINUS_DST_ALPHA, ffi::ONE
        );

        gl.UseProgram(self.program.program);
        gl.UniformMatrix3fv(
            self.program.u_matrix,
            1,
            ffi::FALSE,
            frame.projection().as_ptr(),
        );
        gl.Uniform1f(self.program.u_alpha, alpha);
        gl.Uniform1i(self.program.u_sampler, 0);
        gl.ActiveTexture(ffi::TEXTURE0);
        gl.BindVertexArray(self.vertex_array);
        
        for ClippedMesh(clip_rect, mesh) in clipped_meshes {
            self.paint_mesh(gl, &clip_rect, &mesh, size, scale)?;
        }

        gl.BindVertexArray(0);
        gl.BindBuffer(ffi::ELEMENT_ARRAY_BUFFER, 0);
        gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
        gl.Disable(ffi::SCISSOR_TEST);

        Ok(())
    }

    unsafe fn paint_mesh(
        &self,
        gl: &ffi::Gles2,
        clip_rect: &Rect,
        mesh: &Mesh,
        size: Size<i32, Physical>,
        scale: f64,
    ) -> Result<(), Gles2Error> {
        let texture = match mesh.texture_id {
            TextureId::Egui => self.egui_texture,
            TextureId::User(_) =>  unimplemented!(),
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
        
        let screen_space = Rectangle::from_loc_and_size((0, 0), size);
        let clip_rect = Rectangle::from_extemities(
            (clip_rect.min.x, clip_rect.min.y),
            (clip_rect.max.x, clip_rect.max.y),
        );
        let scissor_box = clip_rect.to_f64().to_physical(scale).intersection(screen_space.to_f64()).unwrap().to_i32_round();

        gl.Scissor(scissor_box.loc.x, scissor_box.loc.y, scissor_box.size.w, scissor_box.size.h);
        gl.DrawElements(ffi::TRIANGLES, mesh.indices.len() as i32, ffi::UNSIGNED_INT, std::ptr::null());
    
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

    let u_matrix    = CStr::from_bytes_with_nul(b"u_matrix\0").expect("NULL terminated");
    let u_sampler   = CStr::from_bytes_with_nul(b"u_sampler\0").expect("NULL terminated");
    let u_alpha     = CStr::from_bytes_with_nul(b"u_alpha\0").expect("NULL terminated");
    let a_pos       = CStr::from_bytes_with_nul(b"a_pos\0").expect("NULL terminated");
    let a_tc        = CStr::from_bytes_with_nul(b"a_tc\0").expect("NULL terminated");
    let a_srgba     = CStr::from_bytes_with_nul(b"a_srgba\0").expect("NULL terminated");

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