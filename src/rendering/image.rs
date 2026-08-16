use std::ffi::{CStr, c_void};

use anyhow::{Result, anyhow};
use glow::HasContext;

use super::shader::compile_program;
use crate::model::RgbaFrame;

pub(crate) struct PinnedImageRenderer {
    gl: glow::Context,
    program: glow::Program,
    vertices: glow::Buffer,
    vertex_array: glow::VertexArray,
    texture: glow::Texture,
}

impl PinnedImageRenderer {
    pub unsafe fn new(
        image: &RgbaFrame,
        mut load: impl FnMut(&CStr) -> *const c_void,
    ) -> Result<Self> {
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| load(name)) };
        let program =
            unsafe { compile_program(&gl, VERTEX_SHADER, FRAGMENT_SHADER, "pinned image")? };
        let vertex_array = unsafe { gl.create_vertex_array() }
            .map_err(|error| anyhow!("create pinned image vertex array: {error}"))?;
        let vertices = unsafe { gl.create_buffer() }
            .map_err(|error| anyhow!("create pinned image vertex buffer: {error}"))?;
        let texture = unsafe { gl.create_texture() }
            .map_err(|error| anyhow!("create pinned image texture: {error}"))?;
        let quad: [f32; 16] = [
            -1.0, 1.0, 0.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, -1.0, 1.0, 1.0,
        ];

        unsafe {
            gl.bind_vertex_array(Some(vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertices));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&quad),
                glow::STATIC_DRAW,
            );
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);

            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                image.width() as i32,
                image.height() as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(image.pixels())),
            );
            gl.use_program(Some(program));
            gl.uniform_1_i32(
                gl.get_uniform_location(program, "image_texture").as_ref(),
                0,
            );
            gl.use_program(None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);
        }

        Ok(Self {
            gl,
            program,
            vertices,
            vertex_array,
            texture,
        })
    }

    pub fn render(&self, width: u32, height: u32) {
        unsafe {
            self.gl.viewport(0, 0, width as i32, height as i32);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.disable(glow::STENCIL_TEST);
            self.gl.disable(glow::BLEND);
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            self.gl.use_program(Some(self.program));
            self.gl.bind_vertex_array(Some(self.vertex_array));
            self.gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            self.gl.bind_vertex_array(None);
            self.gl.use_program(None);
            self.gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }
}

impl Drop for PinnedImageRenderer {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_texture(self.texture);
            self.gl.delete_buffer(self.vertices);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_program(self.program);
        }
    }
}

const VERTEX_SHADER: &str = r#"#version 330 core
layout(location = 0) in vec2 position;
layout(location = 1) in vec2 texture_coordinate;
out vec2 uv;
void main() {
    uv = texture_coordinate;
    gl_Position = vec4(position, 0.0, 1.0);
}
"#;

const FRAGMENT_SHADER: &str = r#"#version 330 core
uniform sampler2D image_texture;
in vec2 uv;
out vec4 color;
void main() {
    color = texture(image_texture, uv);
}
"#;
