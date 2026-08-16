use std::ffi::{CStr, c_void};

use anyhow::{Context, Result, anyhow};
use glow::HasContext;

use crate::scroll::PreviewPatch;

use super::shader::compile_program;

const PREFERRED_TILE_ROWS: u32 = 2048;

pub(crate) struct ScrollPreviewRenderer {
    gl: glow::Context,
    program: glow::Program,
    vertices: glow::Buffer,
    vertex_array: glow::VertexArray,
    width: u32,
    document_height: u32,
    tile_rows: u32,
    tiles: Vec<TextureTile>,
}

impl ScrollPreviewRenderer {
    pub unsafe fn new(width: u32, mut load: impl FnMut(&CStr) -> *const c_void) -> Result<Self> {
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| load(name)) };
        let maximum_texture_size = unsafe { gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE) };
        anyhow::ensure!(
            maximum_texture_size > 0,
            "OpenGL reported an invalid texture limit"
        );
        anyhow::ensure!(
            width <= maximum_texture_size as u32,
            "scroll preview width {width} exceeds OpenGL texture limit {maximum_texture_size}"
        );
        let tile_rows = PREFERRED_TILE_ROWS.min(maximum_texture_size as u32);
        let program =
            unsafe { compile_program(&gl, VERTEX_SHADER, FRAGMENT_SHADER, "scroll preview")? };
        let vertex_array = unsafe { gl.create_vertex_array() }
            .map_err(|error| anyhow!("create scroll preview vertex array: {error}"))?;
        let vertices = unsafe { gl.create_buffer() }
            .map_err(|error| anyhow!("create scroll preview vertex buffer: {error}"))?;

        unsafe {
            gl.bind_vertex_array(Some(vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertices));
            gl.buffer_data_size(glow::ARRAY_BUFFER, 64, glow::DYNAMIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);
            gl.use_program(Some(program));
            gl.uniform_1_i32(
                gl.get_uniform_location(program, "image_texture").as_ref(),
                0,
            );
            gl.use_program(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);
        }

        Ok(Self {
            gl,
            program,
            vertices,
            vertex_array,
            width,
            document_height: 0,
            tile_rows,
            tiles: Vec::new(),
        })
    }

    pub fn update(&mut self, patch: PreviewPatch<'_>) -> Result<()> {
        anyhow::ensure!(
            patch.document_width == self.width,
            "scroll preview width changed from {} to {}",
            self.width,
            patch.document_width
        );
        let patch_bottom = patch
            .region
            .top
            .checked_add(patch.region.height)
            .context("scroll preview patch range overflow")?;
        anyhow::ensure!(
            patch_bottom <= patch.document_height,
            "scroll preview patch exceeds document"
        );
        let row_bytes = self.width as usize * 4;
        let expected = patch.region.height as usize * row_bytes;
        anyhow::ensure!(
            patch.rgba.len() == expected,
            "scroll preview patch has {} bytes, expected {expected}",
            patch.rgba.len()
        );
        self.ensure_tiles(patch.document_height)?;

        let first_tile = patch.region.top / self.tile_rows;
        let last_tile = (patch_bottom.saturating_sub(1)) / self.tile_rows;
        for tile_index in first_tile..=last_tile {
            let tile_start = tile_index * self.tile_rows;
            let upload_start = patch.region.top.max(tile_start);
            let upload_end = patch_bottom.min(tile_start + self.tile_rows);
            let upload_rows = upload_end - upload_start;
            let source_start = (upload_start - patch.region.top) as usize * row_bytes;
            let source_end = source_start + upload_rows as usize * row_bytes;
            let tile = &mut self.tiles[tile_index as usize];
            unsafe {
                self.gl.bind_texture(glow::TEXTURE_2D, Some(tile.texture));
                self.gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    (upload_start - tile_start) as i32,
                    self.width as i32,
                    upload_rows as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&patch.rgba[source_start..source_end])),
                );
            }
            tile.used_rows = tile.used_rows.max(upload_end - tile_start);
        }
        unsafe { self.gl.bind_texture(glow::TEXTURE_2D, None) };
        self.document_height = patch.document_height;
        Ok(())
    }

    pub fn render(&self, viewport_width: u32, viewport_height: u32) {
        let viewport_width = viewport_width.max(1);
        let viewport_height = viewport_height.max(1);
        unsafe {
            self.gl
                .viewport(0, 0, viewport_width as i32, viewport_height as i32);
            self.gl.clear_color(0.08, 0.09, 0.1, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        if self.document_height == 0 {
            return;
        }

        let image_aspect = self.width as f32 / self.document_height as f32;
        let viewport_aspect = viewport_width as f32 / viewport_height as f32;
        let (half_width, half_height) = if image_aspect > viewport_aspect {
            (1.0, viewport_aspect / image_aspect)
        } else {
            (image_aspect / viewport_aspect, 1.0)
        };

        unsafe {
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.disable(glow::STENCIL_TEST);
            self.gl.disable(glow::BLEND);
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.use_program(Some(self.program));
            self.gl.bind_vertex_array(Some(self.vertex_array));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vertices));
        }
        for (index, tile) in self.tiles.iter().enumerate() {
            let top_row = index as u32 * self.tile_rows;
            if top_row >= self.document_height {
                break;
            }
            let bottom_row = (top_row + tile.used_rows).min(self.document_height);
            let top =
                half_height - 2.0 * half_height * top_row as f32 / self.document_height as f32;
            let bottom =
                half_height - 2.0 * half_height * bottom_row as f32 / self.document_height as f32;
            let texture_bottom = tile.used_rows as f32 / self.tile_rows as f32;
            let quad: [f32; 16] = [
                -half_width,
                top,
                0.0,
                0.0,
                -half_width,
                bottom,
                0.0,
                texture_bottom,
                half_width,
                top,
                1.0,
                0.0,
                half_width,
                bottom,
                1.0,
                texture_bottom,
            ];
            unsafe {
                self.gl.buffer_sub_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    0,
                    bytemuck::cast_slice(&quad),
                );
                self.gl.bind_texture(glow::TEXTURE_2D, Some(tile.texture));
                self.gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            }
        }
        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D, None);
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
            self.gl.bind_vertex_array(None);
            self.gl.use_program(None);
        }
    }

    fn ensure_tiles(&mut self, document_height: u32) -> Result<()> {
        let required = document_height.div_ceil(self.tile_rows) as usize;
        while self.tiles.len() < required {
            let texture = unsafe { self.gl.create_texture() }
                .map_err(|error| anyhow!("create scroll preview texture: {error}"))?;
            unsafe {
                self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
                self.gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                self.gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    self.width as i32,
                    self.tile_rows as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
            }
            self.tiles.push(TextureTile {
                texture,
                used_rows: 0,
            });
        }
        unsafe { self.gl.bind_texture(glow::TEXTURE_2D, None) };
        Ok(())
    }
}

impl Drop for ScrollPreviewRenderer {
    fn drop(&mut self) {
        unsafe {
            for tile in self.tiles.drain(..) {
                self.gl.delete_texture(tile.texture);
            }
            self.gl.delete_buffer(self.vertices);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_program(self.program);
        }
    }
}

struct TextureTile {
    texture: glow::Texture,
    used_rows: u32,
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
