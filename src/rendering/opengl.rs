use std::ffi::{CStr, c_void};
use std::path::Path as FilePath;

use anyhow::{Context, Result, anyhow};
use femtovg::renderer::OpenGl;
use femtovg::{
    Align, Baseline, Canvas, Color, FontId, ImageFlags, ImageId, ImageInfo, LineCap, Paint, Path,
    PixelFormat, RenderTarget,
};
use glow::HasContext;

use super::emoji::EmojiRenderer;
use super::icons::ToolbarIcons;
use super::shader::compile_program as compile_shader_program;
use crate::model::{
    Annotation, AnnotationKind, DesktopFrame, EMOTIONS, Editor, ExportRegion, MOSAIC_BLOCK_SIZES,
    OptionsLayout, OverlayAction, OverlayLayout, OverlayOption, OverlaySession, Rect, Rgba,
    RgbaFrame, STROKE_WIDTHS, ScrollAction, ScrollLayout, TEXT_SIZES, TOOLBAR_COLORS, Tool,
    handle_points,
};

pub struct OverlayRenderer {
    document: DocumentPass,
    canvas: Canvas<OpenGl>,
    fonts: Vec<FontId>,
    emojis: EmojiRenderer,
    toolbar_icons: ToolbarIcons,
    export_target: Option<ExportTarget>,
    blank_editor: Editor,
}

impl OverlayRenderer {
    /// Creates all long-lived GPU resources and uploads the frozen desktop once.
    /// The supplied OpenGL context must remain current for this renderer's life.
    pub unsafe fn new(
        frame: &DesktopFrame,
        mut load: impl FnMut(&CStr) -> *const c_void,
    ) -> Result<Self> {
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| load(name)) };
        let document = unsafe {
            DocumentPass::new(
                gl,
                frame.bounds.width(),
                frame.bounds.height(),
                &frame.pixels,
                SourcePixelFormat::Bgra,
            )
        }
        .context("failed to create document OpenGL pass")?;
        let vector = unsafe { OpenGl::new_from_function_cstr(load) }
            .map_err(|error| anyhow!("failed to create FemtoVG renderer: {error:?}"))?;
        let mut canvas = Canvas::new(vector)
            .map_err(|error| anyhow!("failed to create FemtoVG canvas: {error:?}"))?;
        let toolbar_icons =
            ToolbarIcons::new(&mut canvas).context("failed to initialize toolbar icons")?;
        Ok(Self {
            document,
            canvas,
            fonts: Vec::new(),
            emojis: EmojiRenderer::new(),
            toolbar_icons,
            export_target: None,
            blank_editor: Editor::new(),
        })
    }

    pub fn load_font(&mut self, path: &FilePath) {
        self.emojis.try_load_font(path);
        if let Ok(font) = self.canvas.add_font(path) {
            self.fonts.push(font);
        }
    }

    pub fn render(
        &mut self,
        width: u32,
        height: u32,
        dpi_scale: f32,
        frame: &DesktopFrame,
        session: &OverlaySession,
    ) {
        unsafe {
            self.document.draw(
                ScenePass {
                    target: None,
                    width,
                    height,
                    source: frame.bounds.local_bounds(),
                    transform: DocumentTransform::from_rect(
                        frame.bounds.local_bounds(),
                        Rect::new(0.0, 0.0, width as f32, height as f32),
                    ),
                    include_draft: true,
                    clip: session.selection().rect(),
                },
                session.editor(),
            )
        };
        self.canvas.set_size(width, height, 1.0);
        paint_overlay(
            &mut self.canvas,
            Rect::new(0.0, 0.0, width as f32, height as f32),
            frame,
            session,
            &self.fonts,
            &mut self.emojis,
            &self.toolbar_icons,
            dpi_scale,
        );
        self.canvas.flush();
    }

    pub fn render_scroll(
        &mut self,
        width: u32,
        height: u32,
        selection: Rect,
        hovered: Option<ScrollAction>,
        pressed: Option<ScrollAction>,
        dpi_scale: f32,
    ) {
        let surface = Rect::new(0.0, 0.0, width as f32, height as f32);
        unsafe {
            self.document.draw(
                ScenePass {
                    target: None,
                    width,
                    height,
                    source: surface,
                    transform: DocumentTransform::from_rect(surface, surface),
                    include_draft: false,
                    clip: None,
                },
                &self.blank_editor,
            )
        };
        self.canvas.set_size(width, height, 1.0);
        paint_mask(
            &mut self.canvas,
            surface,
            Some(selection),
            Color::rgba(0, 0, 0, 110),
        );
        stroke_rect(&mut self.canvas, selection, Color::rgb(0, 120, 212), 1.5);
        paint_scroll_action_bar(
            &mut self.canvas,
            ScrollLayout::new_scaled(selection, surface, dpi_scale),
            hovered,
            pressed,
            &self.toolbar_icons,
            dpi_scale,
        );
        self.canvas.flush();
    }

    /// Composes the selected document into one reusable offscreen target and performs one readback.
    pub fn export(&mut self, frame: &DesktopFrame, session: &OverlaySession) -> Result<RgbaFrame> {
        let selection = session
            .selection()
            .rect()
            .ok_or_else(|| anyhow!("cannot export without a selection"))?;
        let region = ExportRegion::from_selection(selection, frame.bounds)?;
        let local = region.local_rect();
        let width = region.local().width();
        let height = region.local().height();
        self.ensure_export_target(width, height)?;
        let target = self
            .export_target
            .as_ref()
            .expect("export target was just created");
        let framebuffer = target.gpu.framebuffer;
        let image = target.image;

        unsafe {
            self.document.draw(
                ScenePass {
                    target: Some(framebuffer),
                    width,
                    height,
                    source: local,
                    transform: DocumentTransform::from_rect(
                        local,
                        Rect::new(0.0, 0.0, width as f32, height as f32),
                    ),
                    include_draft: false,
                    clip: None,
                },
                session.editor(),
            );
        }

        self.canvas.set_size(width, height, 1.0);
        self.canvas.set_render_target(RenderTarget::Image(image));
        self.canvas.save();
        self.canvas.translate(-local.left, -local.top);
        paint_document(
            &mut self.canvas,
            session.editor(),
            &self.fonts,
            &mut self.emojis,
        );
        self.canvas.restore();
        self.canvas.flush();

        let pixels = unsafe { self.document.read_rgba(framebuffer, width, height)? };

        self.canvas.set_render_target(RenderTarget::Screen);
        self.canvas.flush();
        RgbaFrame::new(region.desktop(), pixels).map_err(Into::into)
    }

    fn ensure_export_target(&mut self, width: u32, height: u32) -> Result<()> {
        if self
            .export_target
            .as_ref()
            .is_some_and(|target| target.gpu.width == width && target.gpu.height == height)
        {
            return Ok(());
        }
        self.delete_export_target();

        let gpu = unsafe { self.document.create_export_target(width, height)? };
        let info = ImageInfo::new(
            ImageFlags::PREMULTIPLIED,
            width as usize,
            height as usize,
            PixelFormat::Rgba8,
        );
        let image = match self
            .canvas
            .create_image_from_native_texture(gpu.texture, info)
        {
            Ok(image) => image,
            Err(error) => {
                unsafe { self.document.delete_export_target(gpu) };
                return Err(anyhow!("register FemtoVG export texture: {error:?}"));
            }
        };
        self.export_target = Some(ExportTarget { gpu, image });
        Ok(())
    }

    fn delete_export_target(&mut self) {
        if let Some(target) = self.export_target.take() {
            self.canvas.delete_image(target.image);
            unsafe { self.document.delete_export_target(target.gpu) };
        }
    }
}

impl Drop for OverlayRenderer {
    fn drop(&mut self) {
        self.delete_export_target();
    }
}

struct ExportTarget {
    gpu: GpuExportTarget,
    image: ImageId,
}

#[derive(Clone, Copy)]
pub(super) struct GpuExportTarget {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) texture: glow::Texture,
    pub(super) framebuffer: glow::Framebuffer,
}

pub(super) struct DocumentPass {
    gl: glow::Context,
    program: glow::Program,
    mosaic_program: glow::Program,
    vertices: glow::Buffer,
    vertex_array: glow::VertexArray,
    mosaic_vertices: glow::Buffer,
    mosaic_vertex_array: glow::VertexArray,
    texture: glow::Texture,
    texture_width: u32,
    texture_height: u32,
    mosaic_cache_key: Option<MosaicCacheKey>,
    mosaic_batches: Vec<MosaicBatch>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct MosaicCacheKey {
    generation: u64,
    includes_draft: bool,
}

struct MosaicBatch {
    block_size: u32,
    first_vertex: i32,
    vertex_count: i32,
}

struct MosaicMesh {
    vertices: Vec<f32>,
    batches: Vec<MosaicBatch>,
}

#[derive(Clone, Copy)]
pub(super) struct ScenePass {
    pub(super) target: Option<glow::Framebuffer>,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) source: Rect,
    pub(super) transform: DocumentTransform,
    pub(super) include_draft: bool,
    pub(super) clip: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourcePixelFormat {
    Rgba,
    Bgra,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DocumentTransform {
    origin: crate::model::Point,
    x_axis: crate::model::Point,
    y_axis: crate::model::Point,
}

impl DocumentTransform {
    pub(super) fn from_rect(source: Rect, target: Rect) -> Self {
        let x_scale = target.width() / source.width();
        let y_scale = target.height() / source.height();
        Self {
            origin: crate::model::Point::new(
                target.left - source.left * x_scale,
                target.top - source.top * y_scale,
            ),
            x_axis: crate::model::Point::new(x_scale, 0.0),
            y_axis: crate::model::Point::new(0.0, y_scale),
        }
    }

    pub(super) const fn from_basis(
        origin: crate::model::Point,
        x_axis: crate::model::Point,
        y_axis: crate::model::Point,
    ) -> Self {
        Self {
            origin,
            x_axis,
            y_axis,
        }
    }

    pub(super) fn map(self, point: crate::model::Point) -> crate::model::Point {
        self.origin + self.x_axis * point.x + self.y_axis * point.y
    }
}

impl DocumentPass {
    pub(super) unsafe fn new(
        gl: glow::Context,
        width: u32,
        height: u32,
        pixels: &[u8],
        source_format: SourcePixelFormat,
    ) -> Result<Self> {
        let program = unsafe { compile_program(&gl)? };
        let mosaic_program = unsafe { compile_mosaic_program(&gl)? };
        let vertex_array = unsafe { gl.create_vertex_array() }
            .map_err(|error| anyhow!("create screenshot vertex array: {error}"))?;
        let vertices = unsafe { gl.create_buffer() }
            .map_err(|error| anyhow!("create screenshot vertex buffer: {error}"))?;
        let texture = unsafe { gl.create_texture() }
            .map_err(|error| anyhow!("create screenshot texture: {error}"))?;
        let mosaic_vertex_array = unsafe { gl.create_vertex_array() }
            .map_err(|error| anyhow!("create mosaic vertex array: {error}"))?;
        let mosaic_vertices = unsafe { gl.create_buffer() }
            .map_err(|error| anyhow!("create mosaic vertex buffer: {error}"))?;

        let quad = [0.0_f32; 16];

        unsafe {
            gl.bind_vertex_array(Some(vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertices));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&quad),
                glow::DYNAMIC_DRAW,
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
                width as i32,
                height as i32,
                0,
                match source_format {
                    SourcePixelFormat::Rgba => glow::RGBA,
                    SourcePixelFormat::Bgra => glow::BGRA,
                },
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(pixels)),
            );
            gl.use_program(Some(program));
            gl.uniform_1_i32(gl.get_uniform_location(program, "desktop").as_ref(), 0);
            gl.use_program(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);
            gl.bind_texture(glow::TEXTURE_2D, None);

            gl.bind_vertex_array(Some(mosaic_vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(mosaic_vertices));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);

            gl.use_program(Some(mosaic_program));
            gl.uniform_1_i32(
                gl.get_uniform_location(mosaic_program, "desktop").as_ref(),
                0,
            );
            gl.use_program(None);
        }

        Ok(Self {
            gl,
            program,
            mosaic_program,
            vertices,
            vertex_array,
            mosaic_vertices,
            mosaic_vertex_array,
            texture,
            texture_width: width,
            texture_height: height,
            mosaic_cache_key: None,
            mosaic_batches: Vec::new(),
        })
    }

    pub(super) unsafe fn draw(&mut self, pass: ScenePass, editor: &Editor) {
        unsafe { self.update_mosaic_mesh(editor, pass.include_draft) };
        let texture_width = self.texture_width as f32;
        let texture_height = self.texture_height as f32;
        let to_clip = |point: crate::model::Point| {
            [
                point.x / pass.width as f32 * 2.0 - 1.0,
                1.0 - point.y / pass.height as f32 * 2.0,
            ]
        };
        let corners = [
            (
                pass.transform
                    .map(crate::model::Point::new(pass.source.left, pass.source.top)),
                [
                    pass.source.left / texture_width,
                    pass.source.top / texture_height,
                ],
            ),
            (
                pass.transform.map(crate::model::Point::new(
                    pass.source.left,
                    pass.source.bottom,
                )),
                [
                    pass.source.left / texture_width,
                    pass.source.bottom / texture_height,
                ],
            ),
            (
                pass.transform
                    .map(crate::model::Point::new(pass.source.right, pass.source.top)),
                [
                    pass.source.right / texture_width,
                    pass.source.top / texture_height,
                ],
            ),
            (
                pass.transform.map(crate::model::Point::new(
                    pass.source.right,
                    pass.source.bottom,
                )),
                [
                    pass.source.right / texture_width,
                    pass.source.bottom / texture_height,
                ],
            ),
        ];
        let mut quad = [0.0_f32; 16];
        for (index, (position, uv)) in corners.into_iter().enumerate() {
            let clip = to_clip(position);
            quad[index * 4..index * 4 + 4].copy_from_slice(&[clip[0], clip[1], uv[0], uv[1]]);
        }
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, pass.target);
            self.gl
                .viewport(0, 0, pass.width as i32, pass.height as i32);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.disable(glow::STENCIL_TEST);
            self.gl.disable(glow::BLEND);
            self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            self.gl.use_program(Some(self.program));
            self.gl.bind_vertex_array(Some(self.vertex_array));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vertices));
            self.gl
                .buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytemuck::cast_slice(&quad));
            self.gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
            self.gl.bind_vertex_array(None);
            self.gl.use_program(None);

            if !self.mosaic_batches.is_empty() {
                if let Some(clip) = pass.clip {
                    let [x, y, clip_width, clip_height] =
                        output_scissor(clip, pass.transform, pass.width, pass.height);
                    self.gl.enable(glow::SCISSOR_TEST);
                    self.gl.scissor(x, y, clip_width, clip_height);
                }
                self.gl.use_program(Some(self.mosaic_program));
                self.gl.uniform_2_f32(
                    self.gl
                        .get_uniform_location(self.mosaic_program, "target_origin")
                        .as_ref(),
                    pass.transform.origin.x,
                    pass.transform.origin.y,
                );
                self.gl.uniform_2_f32(
                    self.gl
                        .get_uniform_location(self.mosaic_program, "target_x_axis")
                        .as_ref(),
                    pass.transform.x_axis.x,
                    pass.transform.x_axis.y,
                );
                self.gl.uniform_2_f32(
                    self.gl
                        .get_uniform_location(self.mosaic_program, "target_y_axis")
                        .as_ref(),
                    pass.transform.y_axis.x,
                    pass.transform.y_axis.y,
                );
                self.gl.uniform_2_f32(
                    self.gl
                        .get_uniform_location(self.mosaic_program, "target_size")
                        .as_ref(),
                    pass.width as f32,
                    pass.height as f32,
                );
                self.gl.uniform_2_f32(
                    self.gl
                        .get_uniform_location(self.mosaic_program, "desktop_size")
                        .as_ref(),
                    texture_width,
                    texture_height,
                );
                self.gl.bind_vertex_array(Some(self.mosaic_vertex_array));
                for batch in &self.mosaic_batches {
                    self.gl.uniform_1_f32(
                        self.gl
                            .get_uniform_location(self.mosaic_program, "block_size")
                            .as_ref(),
                        batch.block_size as f32,
                    );
                    self.gl
                        .draw_arrays(glow::TRIANGLES, batch.first_vertex, batch.vertex_count);
                }
                self.gl.bind_vertex_array(None);
                self.gl.use_program(None);
                self.gl.disable(glow::SCISSOR_TEST);
            }
            self.gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    unsafe fn update_mosaic_mesh(&mut self, editor: &Editor, include_draft: bool) {
        let includes_draft = include_draft
            && editor
                .draft()
                .is_some_and(|draft| matches!(draft.kind, AnnotationKind::Mosaic { .. }));
        let key = MosaicCacheKey {
            generation: editor.mosaic_generation(),
            includes_draft,
        };
        if self.mosaic_cache_key == Some(key) {
            return;
        }

        let mesh = build_mosaic_mesh(editor, includes_draft);
        unsafe {
            self.gl
                .bind_buffer(glow::ARRAY_BUFFER, Some(self.mosaic_vertices));
            self.gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&mesh.vertices),
                glow::DYNAMIC_DRAW,
            );
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        self.mosaic_batches = mesh.batches;
        self.mosaic_cache_key = Some(key);
    }

    pub(super) unsafe fn create_export_target(
        &self,
        width: u32,
        height: u32,
    ) -> Result<GpuExportTarget> {
        anyhow::ensure!(width > 0 && height > 0, "export target must not be empty");
        let texture = unsafe { self.gl.create_texture() }
            .map_err(|error| anyhow!("create export texture: {error}"))?;
        let framebuffer = match unsafe { self.gl.create_framebuffer() } {
            Ok(framebuffer) => framebuffer,
            Err(error) => {
                unsafe { self.gl.delete_texture(texture) };
                return Err(anyhow!("create export framebuffer: {error}"));
            }
        };

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
            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            self.gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            self.gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            let status = self.gl.check_framebuffer_status(glow::FRAMEBUFFER);
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.bind_texture(glow::TEXTURE_2D, None);
            if status != glow::FRAMEBUFFER_COMPLETE {
                self.gl.delete_framebuffer(framebuffer);
                self.gl.delete_texture(texture);
                return Err(anyhow!("export framebuffer is incomplete: {status:#x}"));
            }
        }

        Ok(GpuExportTarget {
            width,
            height,
            texture,
            framebuffer,
        })
    }

    pub(super) unsafe fn delete_export_target(&self, target: GpuExportTarget) {
        unsafe {
            self.gl.delete_framebuffer(target.framebuffer);
            self.gl.delete_texture(target.texture);
        }
    }

    pub(super) unsafe fn read_rgba(
        &self,
        framebuffer: glow::Framebuffer,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        let row_bytes = (width as usize)
            .checked_mul(4)
            .ok_or_else(|| anyhow!("export row length overflow"))?;
        let byte_count = row_bytes
            .checked_mul(height as usize)
            .ok_or_else(|| anyhow!("export pixel length overflow"))?;
        let mut pixels = vec![0; byte_count];
        unsafe {
            self.gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            self.gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 4);
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        flip_rows_in_place(&mut pixels, row_bytes, height as usize);
        Ok(pixels)
    }
}

impl Drop for DocumentPass {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_texture(self.texture);
            self.gl.delete_buffer(self.mosaic_vertices);
            self.gl.delete_vertex_array(self.mosaic_vertex_array);
            self.gl.delete_program(self.mosaic_program);
            self.gl.delete_buffer(self.vertices);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_program(self.program);
        }
    }
}

unsafe fn compile_program(gl: &glow::Context) -> Result<glow::Program> {
    const VERTEX: &str = r#"#version 330 core
layout(location = 0) in vec2 position;
layout(location = 1) in vec2 texture_coordinate;
out vec2 uv;
void main() {
    uv = texture_coordinate;
    gl_Position = vec4(position, 0.0, 1.0);
}
"#;
    const FRAGMENT: &str = r#"#version 330 core
uniform sampler2D desktop;
in vec2 uv;
out vec4 color;
void main() {
    color = vec4(texture(desktop, uv).rgb, 1.0);
}
"#;

    unsafe { compile_shader_program(gl, VERTEX, FRAGMENT, "screenshot") }
}

unsafe fn compile_mosaic_program(gl: &glow::Context) -> Result<glow::Program> {
    const VERTEX: &str = r#"#version 330 core
layout(location = 0) in vec2 position;
uniform vec2 target_origin;
uniform vec2 target_x_axis;
uniform vec2 target_y_axis;
uniform vec2 target_size;
out vec2 desktop_position;
void main() {
    vec2 target_position = target_origin
        + target_x_axis * position.x
        + target_y_axis * position.y;
    desktop_position = position;
    gl_Position = vec4(
        target_position.x / target_size.x * 2.0 - 1.0,
        1.0 - target_position.y / target_size.y * 2.0,
        0.0,
        1.0
    );
}
"#;
    const FRAGMENT: &str = r#"#version 330 core
uniform sampler2D desktop;
uniform float block_size;
uniform vec2 desktop_size;
in vec2 desktop_position;
out vec4 color;
void main() {
    vec2 block_center = (floor(desktop_position / block_size) + 0.5) * block_size;
    ivec2 sample_pixel = ivec2(clamp(floor(block_center), vec2(0.0), desktop_size - 1.0));
    color = vec4(texelFetch(desktop, sample_pixel, 0).rgb, 1.0);
}
"#;

    unsafe { compile_shader_program(gl, VERTEX, FRAGMENT, "mosaic") }
}

fn build_mosaic_mesh(editor: &Editor, include_draft: bool) -> MosaicMesh {
    struct PendingBatch {
        block_size: u32,
        vertices: Vec<f32>,
    }

    let mut pending: Vec<PendingBatch> = Vec::new();
    let mut append_annotation = |annotation: &Annotation| {
        let AnnotationKind::Mosaic { points, block_size } = &annotation.kind else {
            return;
        };
        let batch = if let Some(index) = pending
            .iter()
            .position(|batch| batch.block_size == *block_size)
        {
            &mut pending[index]
        } else {
            pending.push(PendingBatch {
                block_size: *block_size,
                vertices: Vec::new(),
            });
            pending.last_mut().expect("batch was just inserted")
        };
        append_mosaic_mesh(&mut batch.vertices, points, *block_size as f32 * 0.5);
    };

    for annotation in editor.annotations().items() {
        append_annotation(annotation);
    }
    if include_draft && let Some(draft) = editor.draft() {
        append_annotation(draft);
    }

    let mut vertices = Vec::new();
    let mut batches = Vec::new();
    for batch in pending
        .into_iter()
        .filter(|batch| !batch.vertices.is_empty())
    {
        let first_vertex =
            i32::try_from(vertices.len() / 2).expect("mosaic mesh exceeds GL limits");
        let vertex_count =
            i32::try_from(batch.vertices.len() / 2).expect("mosaic mesh exceeds GL limits");
        vertices.extend_from_slice(&batch.vertices);
        batches.push(MosaicBatch {
            block_size: batch.block_size,
            first_vertex,
            vertex_count,
        });
    }
    MosaicMesh { vertices, batches }
}

fn output_scissor(clip: Rect, transform: DocumentTransform, width: u32, height: u32) -> [i32; 4] {
    let corners = [
        transform.map(crate::model::Point::new(clip.left, clip.top)),
        transform.map(crate::model::Point::new(clip.right, clip.top)),
        transform.map(crate::model::Point::new(clip.left, clip.bottom)),
        transform.map(crate::model::Point::new(clip.right, clip.bottom)),
    ];
    let left = corners
        .iter()
        .map(|point| point.x)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .clamp(0.0, width as f32) as i32;
    let right = corners
        .iter()
        .map(|point| point.x)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .clamp(0.0, width as f32) as i32;
    let top = corners
        .iter()
        .map(|point| point.y)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .clamp(0.0, height as f32) as i32;
    let bottom = corners
        .iter()
        .map(|point| point.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .clamp(0.0, height as f32) as i32;
    [
        left,
        height as i32 - bottom,
        (right - left).max(0),
        (bottom - top).max(0),
    ]
}

fn flip_rows_in_place(pixels: &mut [u8], row_bytes: usize, height: usize) {
    for top_row in 0..height / 2 {
        let bottom_row = height - 1 - top_row;
        let top_start = top_row * row_bytes;
        let bottom_start = bottom_row * row_bytes;
        let (head, tail) = pixels.split_at_mut(bottom_start);
        head[top_start..top_start + row_bytes].swap_with_slice(&mut tail[..row_bytes]);
    }
}

fn append_mosaic_mesh(vertices: &mut Vec<f32>, points: &[crate::model::Point], radius: f32) {
    const CAP_SEGMENTS: usize = 12;
    for point in points {
        for segment in 0..CAP_SEGMENTS {
            let first = segment as f32 / CAP_SEGMENTS as f32 * std::f32::consts::TAU;
            let second = (segment + 1) as f32 / CAP_SEGMENTS as f32 * std::f32::consts::TAU;
            push_triangle(
                vertices,
                *point,
                crate::model::Point::new(
                    point.x + first.cos() * radius,
                    point.y + first.sin() * radius,
                ),
                crate::model::Point::new(
                    point.x + second.cos() * radius,
                    point.y + second.sin() * radius,
                ),
            );
        }
    }

    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let length = dx.hypot(dy);
        if length <= f32::EPSILON {
            continue;
        }
        let normal = crate::model::Point::new(-dy / length * radius, dx / length * radius);
        let a = crate::model::Point::new(start.x + normal.x, start.y + normal.y);
        let b = crate::model::Point::new(start.x - normal.x, start.y - normal.y);
        let c = crate::model::Point::new(end.x + normal.x, end.y + normal.y);
        let d = crate::model::Point::new(end.x - normal.x, end.y - normal.y);
        push_triangle(vertices, a, b, c);
        push_triangle(vertices, c, b, d);
    }
}

fn push_triangle(
    vertices: &mut Vec<f32>,
    a: crate::model::Point,
    b: crate::model::Point,
    c: crate::model::Point,
) {
    vertices.extend_from_slice(&[a.x, a.y, b.x, b.y, c.x, c.y]);
}

fn paint_overlay(
    canvas: &mut Canvas<OpenGl>,
    surface: Rect,
    frame: &DesktopFrame,
    session: &OverlaySession,
    fonts: &[FontId],
    emojis: &mut EmojiRenderer,
    toolbar_icons: &ToolbarIcons,
    dpi_scale: f32,
) {
    let Some(region) = session.selection().rect() else {
        paint_mask(
            canvas,
            surface,
            session.highlight(),
            Color::rgba(0, 0, 0, 70),
        );
        if let Some(highlight) = session.highlight() {
            stroke_rect(canvas, highlight, Color::rgb(0, 120, 212), 2.0);
            paint_size_badge(canvas, highlight, surface, fonts.first().copied());
        }
        if let Some(cursor) = session.cursor() {
            paint_crosshair(canvas, cursor, surface);
            paint_loupe(canvas, frame, cursor, surface, fonts.first().copied());
        }
        return;
    };

    paint_mask(canvas, surface, Some(region), Color::rgba(0, 0, 0, 110));

    if session.selection_locked() {
        paint_annotations(
            canvas,
            session.editor(),
            region,
            fonts,
            emojis,
            session.cursor(),
        );
    }

    stroke_rect(canvas, region, Color::rgb(0, 120, 212), 1.5);
    if !session.selection_locked() {
        for (_, point) in handle_points(region) {
            paint_grip(canvas, point, false);
        }
    }

    paint_size_badge(canvas, region, surface, fonts.first().copied());
    paint_action_bar(
        canvas,
        OverlayLayout::for_tool_scaled(region, surface, session.active_tool(), dpi_scale),
        session,
        fonts,
        emojis,
        toolbar_icons,
        dpi_scale,
    );
}

fn paint_mask(canvas: &mut Canvas<OpenGl>, surface: Rect, hole: Option<Rect>, shade: Color) {
    let Some(region) = hole.filter(|region| region.width() > 0.0 && region.height() > 0.0) else {
        fill_rect(canvas, surface, shade);
        return;
    };
    fill_rect(
        canvas,
        Rect::new(surface.left, surface.top, surface.right, region.top),
        shade,
    );
    fill_rect(
        canvas,
        Rect::new(surface.left, region.bottom, surface.right, surface.bottom),
        shade,
    );
    fill_rect(
        canvas,
        Rect::new(surface.left, region.top, region.left, region.bottom),
        shade,
    );
    fill_rect(
        canvas,
        Rect::new(region.right, region.top, surface.right, region.bottom),
        shade,
    );
}

fn paint_crosshair(canvas: &mut Canvas<OpenGl>, cursor: crate::model::Point, surface: Rect) {
    let paint = Paint::color(Color::rgba(0, 120, 212, 180)).with_line_width(1.0);
    let mut path = Path::new();
    path.move_to(surface.left, cursor.y);
    path.line_to(surface.right, cursor.y);
    path.move_to(cursor.x, surface.top);
    path.line_to(cursor.x, surface.bottom);
    canvas.stroke_path(&path, &paint);
}

fn paint_size_badge(
    canvas: &mut Canvas<OpenGl>,
    region: Rect,
    surface: Rect,
    font: Option<FontId>,
) {
    let text = format!("{} x {}", region.width().round(), region.height().round());
    let y = if region.top >= 34.0 {
        region.top - 30.0
    } else {
        region.top + 8.0
    };
    paint_badge(canvas, &text, region.left, y, surface, font);
}

fn paint_loupe(
    canvas: &mut Canvas<OpenGl>,
    frame: &DesktopFrame,
    cursor: crate::model::Point,
    surface: Rect,
    font: Option<FontId>,
) {
    const SAMPLE_COUNT: i32 = 15;
    const SIZE: f32 = 128.0;
    const GAP: f32 = 24.0;

    let x = if cursor.x + GAP + SIZE <= surface.right {
        cursor.x + GAP
    } else {
        cursor.x - GAP - SIZE
    };
    let y = if cursor.y + GAP + SIZE <= surface.bottom {
        cursor.y + GAP
    } else {
        cursor.y - GAP - SIZE
    };
    let box_ = clamp_rect(Rect::new(x, y, x + SIZE, y + SIZE), surface);
    let cell = SIZE / SAMPLE_COUNT as f32;
    let center_x = cursor.x.floor() as i32;
    let center_y = cursor.y.floor() as i32;
    let radius = SAMPLE_COUNT / 2;
    let mut center_color = [0u8; 4];

    for row in 0..SAMPLE_COUNT {
        for column in 0..SAMPLE_COUNT {
            let pixel_x = (center_x + column - radius).clamp(0, frame.bounds.width() as i32 - 1);
            let pixel_y = (center_y + row - radius).clamp(0, frame.bounds.height() as i32 - 1);
            let bgra = frame.pixel_at_local(pixel_x, pixel_y).unwrap_or_default();
            if row == radius && column == radius {
                center_color = bgra;
            }
            fill_rect(
                canvas,
                Rect::new(
                    box_.left + column as f32 * cell,
                    box_.top + row as f32 * cell,
                    box_.left + (column + 1) as f32 * cell,
                    box_.top + (row + 1) as f32 * cell,
                ),
                Color::rgb(bgra[2], bgra[1], bgra[0]),
            );
        }
    }

    let grid_paint = Paint::color(Color::rgba(255, 255, 255, 36)).with_line_width(1.0);
    let mut grid = Path::new();
    for index in 1..SAMPLE_COUNT {
        let offset = index as f32 * cell;
        grid.move_to(box_.left, box_.top + offset);
        grid.line_to(box_.right, box_.top + offset);
        grid.move_to(box_.left + offset, box_.top);
        grid.line_to(box_.left + offset, box_.bottom);
    }
    canvas.stroke_path(&grid, &grid_paint);
    stroke_rect(canvas, box_, Color::rgba(255, 255, 255, 180), 1.0);
    stroke_rect(
        canvas,
        Rect::new(
            box_.left + radius as f32 * cell,
            box_.top + radius as f32 * cell,
            box_.left + (radius + 1) as f32 * cell,
            box_.top + (radius + 1) as f32 * cell,
        ),
        Color::rgb(0, 120, 212),
        1.5,
    );

    let text = format!(
        "{}, {}  #{:02X}{:02X}{:02X}",
        center_x, center_y, center_color[2], center_color[1], center_color[0]
    );
    let badge_y = if box_.bottom + 30.0 <= surface.bottom {
        box_.bottom + 8.0
    } else {
        box_.top - 30.0
    };
    paint_badge(canvas, &text, box_.left, badge_y, surface, font);
}

fn paint_badge(
    canvas: &mut Canvas<OpenGl>,
    text: &str,
    x: f32,
    y: f32,
    surface: Rect,
    font: Option<FontId>,
) {
    let width = (text.chars().count() as f32 * 7.2 + 12.0).max(36.0);
    let height = 24.0;
    let bounds = clamp_rect(Rect::new(x, y, x + width, y + height), surface);
    let mut path = Path::new();
    path.rounded_rect(
        bounds.left,
        bounds.top,
        bounds.width(),
        bounds.height(),
        4.0,
    );
    canvas.fill_path(&path, &Paint::color(Color::rgba(0, 0, 0, 210)));
    if let Some(font) = font {
        let paint = Paint::color(Color::rgb(255, 255, 255))
            .with_font(&[font])
            .with_font_size(12.0)
            .with_text_align(Align::Left)
            .with_text_baseline(Baseline::Middle);
        let _ = canvas.fill_text(bounds.left + 6.0, bounds.top + height * 0.5, text, &paint);
    }
}

fn clamp_rect(rect: Rect, bounds: Rect) -> Rect {
    let width = rect.width().min(bounds.width());
    let height = rect.height().min(bounds.height());
    let left = rect.left.clamp(bounds.left, bounds.right - width);
    let top = rect.top.clamp(bounds.top, bounds.bottom - height);
    Rect::new(left, top, left + width, top + height)
}

pub(super) fn paint_annotations(
    canvas: &mut Canvas<OpenGl>,
    editor: &Editor,
    region: Rect,
    fonts: &[FontId],
    emojis: &mut EmojiRenderer,
    cursor: Option<crate::model::Point>,
) {
    canvas.save();
    canvas.scissor(region.left, region.top, region.width(), region.height());
    paint_document(canvas, editor, fonts, emojis);
    if let Some(draft) = editor.draft() {
        paint_annotation(canvas, draft, fonts, emojis, 0.72);
    }

    if let Some(selected) = editor.selected_annotation() {
        let bounds = selected.visual_bounds();
        let editing = editor
            .caret()
            .is_some_and(|caret| caret.annotation == selected.id);
        if matches!(selected.kind, AnnotationKind::Text { .. }) && !editing {
            stroke_rect(canvas, bounds, Color::rgb(232, 17, 35), 1.0);
        } else {
            match &selected.kind {
                AnnotationKind::Arrow { from, to } => {
                    stroke_dashed_line(canvas, *from, *to, Color::rgba(128, 128, 128, 220));
                }
                _ => {
                    stroke_dashed_rect(canvas, bounds, Color::rgba(128, 128, 128, 220));
                }
            }
            let hovered = cursor.and_then(|point| {
                selected
                    .handles()
                    .iter()
                    .copied()
                    .find(|handle| selected.handle_position(*handle).distance(point) <= 8.0)
            });
            for handle in selected.handles() {
                paint_grip(
                    canvas,
                    selected.handle_position(*handle),
                    hovered == Some(*handle),
                );
            }
        }
    } else if let Some(hovered) = editor.hovered_annotation()
        && let Some(annotation) = editor.annotations().get(hovered)
        && matches!(
            annotation.kind,
            AnnotationKind::Rectangle { .. } | AnnotationKind::Text { .. }
        )
    {
        stroke_rect(
            canvas,
            annotation.visual_bounds(),
            Color::rgb(232, 17, 35),
            1.0,
        );
    }

    if let Some(caret) = editor.caret()
        && let Some(annotation) = editor.annotations().get(caret.annotation)
    {
        paint_caret(canvas, annotation, caret.index);
    }

    if editor.tool() == Tool::Mosaic
        && let Some(cursor) = cursor
    {
        let mut preview = Path::new();
        preview.circle(cursor.x, cursor.y, editor.mosaic_block_size() as f32 * 0.5);
        canvas.fill_path(&preview, &Paint::color(Color::rgba(96, 96, 96, 100)));
        canvas.stroke_path(
            &preview,
            &Paint::color(Color::rgba(255, 255, 255, 180)).with_line_width(1.0),
        );
    }
    canvas.restore();
}

pub(super) fn paint_document(
    canvas: &mut Canvas<OpenGl>,
    editor: &Editor,
    fonts: &[FontId],
    emojis: &mut EmojiRenderer,
) {
    for annotation in editor.annotations().items() {
        paint_annotation(canvas, annotation, fonts, emojis, 1.0);
    }
}

fn paint_annotation(
    canvas: &mut Canvas<OpenGl>,
    annotation: &Annotation,
    fonts: &[FontId],
    emojis: &mut EmojiRenderer,
    opacity: f32,
) {
    let stroke_color = model_color(annotation.stroke.color, opacity);
    let mut stroke = Paint::color(stroke_color).with_line_width(annotation.stroke.width.max(1.0));
    stroke.set_line_cap(LineCap::Round);
    match &annotation.kind {
        AnnotationKind::Rectangle { rect } => {
            let rect = rect.normalized();
            let mut path = Path::new();
            path.rect(rect.left, rect.top, rect.width(), rect.height());
            if let Some(fill) = annotation.stroke.fill {
                canvas.fill_path(&path, &Paint::color(model_color(fill, opacity)));
            }
            canvas.stroke_path(&path, &stroke);
        }
        AnnotationKind::Circle { rect } => {
            let rect = rect.normalized();
            let mut path = Path::new();
            path.ellipse(
                rect.center().x,
                rect.center().y,
                rect.width() * 0.5,
                rect.height() * 0.5,
            );
            if let Some(fill) = annotation.stroke.fill {
                canvas.fill_path(&path, &Paint::color(model_color(fill, opacity)));
            }
            canvas.stroke_path(&path, &stroke);
        }
        AnnotationKind::Arrow { from, to } => {
            let mut path = Path::new();
            path.move_to(from.x, from.y);
            path.line_to(to.x, to.y);
            let angle = (to.y - from.y).atan2(to.x - from.x);
            let head = (annotation.stroke.width * 3.0 + 8.0).clamp(10.0, 24.0);
            for offset in [-0.65_f32, 0.65_f32] {
                path.move_to(to.x, to.y);
                path.line_to(
                    to.x - head * (angle + offset).cos(),
                    to.y - head * (angle + offset).sin(),
                );
            }
            canvas.stroke_path(&path, &stroke);
        }
        AnnotationKind::Pen { points } => {
            if let Some(first) = points.first() {
                let mut path = Path::new();
                path.move_to(first.x, first.y);
                for point in &points[1..] {
                    path.line_to(point.x, point.y);
                }
                canvas.stroke_path(&path, &stroke);
            }
        }
        AnnotationKind::Mosaic { .. } => {}
        AnnotationKind::Text {
            origin,
            content,
            style,
        } => {
            if fonts.is_empty() {
                return;
            }
            let paint = Paint::color(model_color(style.color, opacity))
                .with_font(fonts)
                .with_font_size(style.size)
                .with_text_align(Align::Left)
                .with_text_baseline(Baseline::Top);
            for (line, text) in content.split('\n').enumerate() {
                let _ = canvas.fill_text(
                    origin.x,
                    origin.y + line as f32 * style.size * 1.2,
                    text,
                    &paint,
                );
            }
        }
        AnnotationKind::Emotion {
            center,
            glyph,
            size,
        } => {
            let bounds = Rect::new(
                center.x - size * 0.5,
                center.y - size * 0.5,
                center.x + size * 0.5,
                center.y + size * 0.5,
            );
            if emojis.paint(canvas, glyph, bounds, opacity) {
                return;
            }
            if fonts.is_empty() {
                return;
            }
            let paint = Paint::color(model_color(annotation.stroke.color, opacity))
                .with_font(fonts)
                .with_font_size(*size)
                .with_text_align(Align::Center)
                .with_text_baseline(Baseline::Middle);
            let _ = canvas.fill_text(center.x, center.y, glyph, &paint);
        }
    }
}

fn paint_caret(canvas: &mut Canvas<OpenGl>, annotation: &Annotation, index: usize) {
    let AnnotationKind::Text {
        origin,
        content,
        style,
    } = &annotation.kind
    else {
        return;
    };
    let before: String = content.chars().take(index).collect();
    let line = before
        .chars()
        .filter(|character| *character == '\n')
        .count();
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |value| value.chars().count());
    let x = origin.x + column as f32 * style.size * 0.6;
    let y = origin.y + line as f32 * style.size * 1.2;
    let mut path = Path::new();
    path.move_to(x, y);
    path.line_to(x, y + style.size * 1.1);
    canvas.stroke_path(
        &path,
        &Paint::color(Color::rgb(32, 176, 80)).with_line_width(1.5),
    );
}

fn paint_grip(canvas: &mut Canvas<OpenGl>, point: crate::model::Point, hovered: bool) {
    let mut grip = Path::new();
    grip.circle(point.x, point.y, 3.0);
    canvas.fill_path(
        &grip,
        &Paint::color(if hovered {
            Color::rgb(191, 191, 191)
        } else {
            Color::rgb(255, 255, 255)
        }),
    );
    canvas.stroke_path(
        &grip,
        &Paint::color(Color::rgb(0, 0, 0)).with_line_width(1.0),
    );
}

fn stroke_dashed_rect(canvas: &mut Canvas<OpenGl>, rect: Rect, color: Color) {
    let rect = rect.normalized();
    for (from, to) in [
        (
            crate::model::Point::new(rect.left, rect.top),
            crate::model::Point::new(rect.right, rect.top),
        ),
        (
            crate::model::Point::new(rect.right, rect.top),
            crate::model::Point::new(rect.right, rect.bottom),
        ),
        (
            crate::model::Point::new(rect.right, rect.bottom),
            crate::model::Point::new(rect.left, rect.bottom),
        ),
        (
            crate::model::Point::new(rect.left, rect.bottom),
            crate::model::Point::new(rect.left, rect.top),
        ),
    ] {
        stroke_dashed_line(canvas, from, to, color);
    }
}

fn stroke_dashed_line(
    canvas: &mut Canvas<OpenGl>,
    from: crate::model::Point,
    to: crate::model::Point,
    color: Color,
) {
    let mut path = Path::new();
    path.move_to(from.x, from.y);
    path.line_to(to.x, to.y);
    canvas.stroke_path(
        &path,
        &Paint::color(color)
            .with_line_width(1.0)
            .with_line_dash(&[4.0, 2.0]),
    );
}

fn model_color(color: Rgba, opacity: f32) -> Color {
    Color::rgba(
        color.r,
        color.g,
        color.b,
        (color.a as f32 * opacity.clamp(0.0, 1.0)).round() as u8,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolbarVisualState {
    Disabled,
    Pressed,
    Hovered,
    Active,
    Idle,
}

fn toolbar_visual_state(
    enabled: bool,
    active: bool,
    hovered: bool,
    pressed: bool,
) -> ToolbarVisualState {
    if !enabled {
        ToolbarVisualState::Disabled
    } else if pressed {
        ToolbarVisualState::Pressed
    } else if hovered {
        ToolbarVisualState::Hovered
    } else if active {
        ToolbarVisualState::Active
    } else {
        ToolbarVisualState::Idle
    }
}

fn paint_action_bar(
    canvas: &mut Canvas<OpenGl>,
    layout: OverlayLayout,
    session: &OverlaySession,
    fonts: &[FontId],
    emojis: &mut EmojiRenderer,
    toolbar_icons: &ToolbarIcons,
    dpi_scale: f32,
) {
    let mut bar = Path::new();
    bar.rounded_rect(
        layout.bar.left,
        layout.bar.top,
        layout.bar.width(),
        layout.bar.height(),
        10.0_f32.min(layout.bar.height() * 0.25),
    );
    canvas.fill_path(&bar, &Paint::color(Color::rgb(240, 240, 240)));

    for button in layout.buttons {
        let enabled = session.action_enabled(button.action);
        let active =
            matches!(button.action, OverlayAction::Tool(tool) if tool == session.active_tool());
        let state = toolbar_visual_state(
            enabled,
            active,
            session.hovered_action() == Some(button.action),
            session.pressed_action() == Some(button.action),
        );
        let background_color = match state {
            ToolbarVisualState::Pressed => Some(Color::rgb(174, 174, 174)),
            ToolbarVisualState::Hovered => Some(Color::rgb(191, 191, 191)),
            ToolbarVisualState::Active => Some(Color::rgba(0, 120, 212, 36)),
            ToolbarVisualState::Disabled | ToolbarVisualState::Idle => None,
        };
        if let Some(color) = background_color {
            let mut background = Path::new();
            background.rounded_rect(
                button.bounds.left,
                button.bounds.top,
                button.bounds.width(),
                button.bounds.height(),
                6.0_f32.min(button.bounds.width() * 0.2),
            );
            canvas.fill_path(&background, &Paint::color(color));
        }

        let glyph = if state == ToolbarVisualState::Disabled {
            Color::rgb(158, 158, 158)
        } else if active {
            Color::rgb(0, 120, 212)
        } else {
            match button.action {
                OverlayAction::Confirm => Color::rgb(0, 195, 117),
                OverlayAction::Cancel => Color::rgb(250, 81, 81),
                _ => Color::rgb(0, 0, 0),
            }
        };
        toolbar_icons.paint(canvas, button.action, button.bounds, glyph, dpi_scale);
    }
    if let Some(options) = layout.options {
        paint_options(
            canvas,
            options,
            session,
            fonts,
            emojis,
            toolbar_icons,
            dpi_scale,
        );
    }
}

fn paint_scroll_action_bar(
    canvas: &mut Canvas<OpenGl>,
    layout: ScrollLayout,
    hovered: Option<ScrollAction>,
    pressed: Option<ScrollAction>,
    toolbar_icons: &ToolbarIcons,
    dpi_scale: f32,
) {
    let mut bar = Path::new();
    bar.rounded_rect(
        layout.bar.left,
        layout.bar.top,
        layout.bar.width(),
        layout.bar.height(),
        10.0_f32.min(layout.bar.height() * 0.25),
    );
    canvas.fill_path(&bar, &Paint::color(Color::rgb(240, 240, 240)));

    for button in layout.buttons {
        if hovered == Some(button.action) || pressed == Some(button.action) {
            let mut background = Path::new();
            background.rounded_rect(
                button.bounds.left,
                button.bounds.top,
                button.bounds.width(),
                button.bounds.height(),
                6.0_f32.min(button.bounds.width() * 0.2),
            );
            let color = if pressed == Some(button.action) {
                Color::rgb(174, 174, 174)
            } else {
                Color::rgb(191, 191, 191)
            };
            canvas.fill_path(&background, &Paint::color(color));
        }
        let (icon, color) = match button.action {
            ScrollAction::Edit => (OverlayAction::Tool(Tool::Select), Color::rgb(0, 0, 0)),
            ScrollAction::Save => (OverlayAction::Save, Color::rgb(0, 0, 0)),
            ScrollAction::Cancel => (OverlayAction::Cancel, Color::rgb(250, 81, 81)),
            ScrollAction::Confirm => (OverlayAction::Confirm, Color::rgb(0, 195, 117)),
        };
        toolbar_icons.paint(canvas, icon, button.bounds, color, dpi_scale);
    }
}

fn paint_options(
    canvas: &mut Canvas<OpenGl>,
    layout: OptionsLayout,
    session: &OverlaySession,
    fonts: &[FontId],
    emojis: &mut EmojiRenderer,
    toolbar_icons: &ToolbarIcons,
    dpi_scale: f32,
) {
    let mut bar = Path::new();
    bar.rounded_rect(
        layout.bar.left,
        layout.bar.top,
        layout.bar.width(),
        layout.bar.height(),
        8.0_f32.min(layout.bar.height() * 0.2),
    );
    canvas.fill_path(&bar, &Paint::color(Color::rgb(240, 240, 240)));

    for button in layout.buttons() {
        let OverlayAction::Option(option) = button.action else {
            continue;
        };
        let hovered = session.hovered_action() == Some(button.action);
        let pressed = session.pressed_action() == Some(button.action);
        let active = session.option_active(option);
        let background = if pressed {
            Some(Color::rgb(174, 174, 174))
        } else if hovered {
            Some(Color::rgb(191, 191, 191))
        } else if active {
            Some(Color::rgba(0, 120, 212, 36))
        } else {
            None
        };
        if let Some(background) = background {
            let mut path = Path::new();
            path.rounded_rect(
                button.bounds.left,
                button.bounds.top,
                button.bounds.width(),
                button.bounds.height(),
                4.0_f32.min(button.bounds.width() * 0.2),
            );
            canvas.fill_path(&path, &Paint::color(background));
        }
        paint_option_icon(
            canvas,
            option,
            button.bounds,
            active,
            fonts,
            emojis,
            toolbar_icons,
            dpi_scale,
        );
    }
}

fn paint_option_icon(
    canvas: &mut Canvas<OpenGl>,
    option: OverlayOption,
    bounds: Rect,
    active: bool,
    fonts: &[FontId],
    emojis: &mut EmojiRenderer,
    toolbar_icons: &ToolbarIcons,
    dpi_scale: f32,
) {
    let center = bounds.center();
    let foreground = if active {
        Color::rgb(0, 120, 212)
    } else {
        Color::rgb(20, 20, 20)
    };
    match option {
        OverlayOption::StrokeWidth(index) => {
            let Some(width) = STROKE_WIDTHS.get(index as usize) else {
                return;
            };
            let mut dot = Path::new();
            dot.circle(center.x, center.y, width * dpi_scale * 0.5);
            canvas.fill_path(&dot, &Paint::color(foreground));
        }
        OverlayOption::TextSize(index) => {
            let Some(size) = TEXT_SIZES.get(index as usize) else {
                return;
            };
            if fonts.is_empty() {
                return;
            }
            let display_size =
                ((10.0 + size * 0.3) * dpi_scale).min(bounds.height() - 4.0 * dpi_scale);
            let paint = Paint::color(foreground)
                .with_font(fonts)
                .with_font_size(display_size)
                .with_text_align(Align::Center)
                .with_text_baseline(Baseline::Middle);
            let _ = canvas.fill_text(center.x, center.y, "A", &paint);
        }
        OverlayOption::ToggleFill => {
            toolbar_icons.paint_fill(canvas, bounds, foreground, dpi_scale);
        }
        OverlayOption::Color(index) => {
            let Some(color) = TOOLBAR_COLORS.get(index as usize) else {
                return;
            };
            let size = (16.0 * dpi_scale)
                .min(bounds.width() - 4.0 * dpi_scale)
                .max(1.0);
            let mut swatch = Path::new();
            swatch.rounded_rect(
                center.x - size * 0.5,
                center.y - size * 0.5,
                size,
                size,
                2.0 * dpi_scale,
            );
            canvas.fill_path(&swatch, &Paint::color(model_color(*color, 1.0)));
            let border = if *color == Rgba::WHITE {
                Color::rgb(110, 110, 110)
            } else if active {
                Color::rgb(0, 120, 212)
            } else {
                Color::rgba(0, 0, 0, 80)
            };
            canvas.stroke_path(
                &swatch,
                &Paint::color(border).with_line_width(dpi_scale.max(1.0)),
            );
        }
        OverlayOption::MosaicBlock(index) => {
            let Some(size) = MOSAIC_BLOCK_SIZES.get(index as usize) else {
                return;
            };
            let radius = (*size as f32 * dpi_scale * 0.25).min(bounds.width() * 0.4);
            let mut preview = Path::new();
            preview.circle(center.x, center.y, radius);
            canvas.fill_path(&preview, &Paint::color(Color::rgba(96, 96, 96, 180)));
        }
        OverlayOption::Emotion(index) => {
            let Some(emotion) = EMOTIONS.get(index as usize) else {
                return;
            };
            let glyph_bounds = bounds.inflated(-1.5 * dpi_scale);
            if emojis.paint(canvas, emotion, glyph_bounds, 1.0) {
                return;
            }
            if fonts.is_empty() {
                return;
            }
            let paint = Paint::color(foreground)
                .with_font(fonts)
                .with_font_size((bounds.height() - 3.0 * dpi_scale).max(1.0))
                .with_text_align(Align::Center)
                .with_text_baseline(Baseline::Middle);
            let _ = canvas.fill_text(center.x, center.y, emotion, &paint);
        }
    }
}

fn fill_rect(canvas: &mut Canvas<OpenGl>, rect: Rect, color: Color) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let mut path = Path::new();
    path.rect(rect.left, rect.top, rect.width(), rect.height());
    canvas.fill_path(&path, &Paint::color(color));
}

fn stroke_rect(canvas: &mut Canvas<OpenGl>, rect: Rect, color: Color, width: f32) {
    let mut path = Path::new();
    path.rect(rect.left, rect.top, rect.width(), rect.height());
    canvas.stroke_path(&path, &Paint::color(color).with_line_width(width));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Point;

    const REGION: Rect = Rect::new(0.0, 0.0, 800.0, 600.0);

    #[test]
    fn mosaic_mesh_is_a_capsule_for_each_sampled_segment() {
        let mut vertices = Vec::new();
        append_mosaic_mesh(
            &mut vertices,
            &[Point::new(100.0, 100.0), Point::new(200.0, 100.0)],
            12.0,
        );

        let cap_floats = 2 * 12 * 3 * 2;
        let segment_floats = 2 * 3 * 2;
        assert_eq!(vertices.len(), cap_floats + segment_floats);
        assert!(vertices.iter().all(|value| value.is_finite()));
        assert!(vertices.chunks_exact(2).all(|point| {
            point[0] >= 88.0 && point[0] <= 212.0 && point[1] >= 88.0 && point[1] <= 112.0
        }));
    }

    #[test]
    fn batches_are_built_only_for_mosaic_annotations() {
        let mut editor = Editor::new();
        editor.set_tool(Tool::Rectangle);
        editor.press(Point::new(10.0, 10.0), REGION);
        editor.pointer_move(Point::new(100.0, 100.0), REGION);
        editor.release();
        assert!(build_mosaic_mesh(&editor, false).batches.is_empty());

        editor.set_tool(Tool::Mosaic);
        editor.press(Point::new(20.0, 20.0), REGION);
        editor.pointer_move(Point::new(200.0, 120.0), REGION);
        editor.release();
        let mesh = build_mosaic_mesh(&editor, false);
        assert_eq!(mesh.batches.len(), 1);
        assert_eq!(mesh.batches[0].block_size, 16);
        assert!(!mesh.vertices.is_empty());
    }

    #[test]
    fn live_mosaic_draft_is_included_before_release() {
        let mut editor = Editor::new();
        editor.set_tool(Tool::Mosaic);
        editor.press(Point::new(20.0, 20.0), REGION);
        editor.pointer_move(Point::new(100.0, 80.0), REGION);

        assert!(build_mosaic_mesh(&editor, false).batches.is_empty());
        let mesh = build_mosaic_mesh(&editor, true);
        assert_eq!(mesh.batches.len(), 1);
        assert!(!mesh.vertices.is_empty());
    }

    #[test]
    fn mosaic_brush_radius_is_half_of_the_selected_block_size() {
        let mut editor = Editor::new();
        assert!(editor.set_mosaic_block_size(10));
        editor.set_tool(Tool::Mosaic);
        editor.press(Point::new(50.0, 40.0), REGION);

        let mesh = build_mosaic_mesh(&editor, true);
        let points: Vec<_> = mesh.vertices.chunks_exact(2).collect();
        let left = points
            .iter()
            .map(|point| point[0])
            .fold(f32::INFINITY, f32::min);
        let right = points
            .iter()
            .map(|point| point[0])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((left - 45.0).abs() < 0.001);
        assert!((right - 55.0).abs() < 0.001);
    }

    #[test]
    fn crop_scissor_is_mapped_to_bottom_left_gl_coordinates() {
        let source = Rect::new(100.0, 50.0, 500.0, 250.0);
        let clip = Rect::new(200.0, 100.0, 400.0, 200.0);
        let transform = DocumentTransform::from_rect(source, Rect::new(0.0, 0.0, 800.0, 400.0));
        assert_eq!(
            output_scissor(clip, transform, 800, 400),
            [200, 100, 400, 200]
        );
    }

    #[test]
    fn readback_rows_are_flipped_without_changing_pixels_inside_rows() {
        let mut pixels = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        flip_rows_in_place(&mut pixels, 4, 3);
        assert_eq!(pixels, vec![9, 10, 11, 12, 5, 6, 7, 8, 1, 2, 3, 4]);
    }

    #[test]
    fn disabled_toolbar_actions_never_get_interactive_visuals() {
        for (active, hovered, pressed) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (true, true, true),
        ] {
            assert_eq!(
                toolbar_visual_state(false, active, hovered, pressed),
                ToolbarVisualState::Disabled
            );
        }
    }

    #[test]
    fn enabled_toolbar_visual_priority_matches_pointer_state() {
        assert_eq!(
            toolbar_visual_state(true, true, true, true),
            ToolbarVisualState::Pressed
        );
        assert_eq!(
            toolbar_visual_state(true, true, true, false),
            ToolbarVisualState::Hovered
        );
        assert_eq!(
            toolbar_visual_state(true, true, false, false),
            ToolbarVisualState::Active
        );
        assert_eq!(
            toolbar_visual_state(true, false, false, false),
            ToolbarVisualState::Idle
        );
    }
}
