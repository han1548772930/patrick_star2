use std::ffi::{CStr, c_void};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use femtovg::renderer::OpenGl;
use femtovg::{Canvas, FontId, ImageFlags, ImageId, ImageInfo, PixelFormat, RenderTarget};

use super::emoji::EmojiRenderer;
use super::opengl::{
    DocumentPass, DocumentTransform, GpuExportTarget, ScenePass, SourcePixelFormat,
    paint_annotations, paint_document,
};
use crate::model::preview::{PreviewSession, QuarterTurn, ViewTransform};
use crate::model::{Point, RectI, RgbaFrame};

pub(crate) struct PreviewRenderer {
    document: DocumentPass,
    canvas: Canvas<OpenGl>,
    fonts: Vec<FontId>,
    emojis: EmojiRenderer,
    export_target: Option<ExportTarget>,
}

struct ExportTarget {
    gpu: GpuExportTarget,
    image: ImageId,
}

impl PreviewRenderer {
    pub unsafe fn new(
        image: &RgbaFrame,
        mut load: impl FnMut(&CStr) -> *const c_void,
    ) -> Result<Self> {
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| load(name)) };
        let document = unsafe {
            DocumentPass::new(
                gl,
                image.width(),
                image.height(),
                image.pixels(),
                SourcePixelFormat::Rgba,
            )
        }
        .context("create preview document pass")?;
        let vector = unsafe { OpenGl::new_from_function_cstr(load) }
            .map_err(|error| anyhow!("create preview FemtoVG renderer: {error:?}"))?;
        let canvas = Canvas::new(vector)
            .map_err(|error| anyhow!("create preview FemtoVG canvas: {error:?}"))?;
        Ok(Self {
            document,
            canvas,
            fonts: Vec::new(),
            emojis: EmojiRenderer::new(),
            export_target: None,
        })
    }

    pub fn load_font(&mut self, path: &Path) {
        self.emojis.try_load_font(path);
        if let Ok(font) = self.canvas.add_font(path) {
            self.fonts.push(font);
        }
    }

    pub fn render(
        &mut self,
        surface_width: u32,
        surface_height: u32,
        scale_factor: f32,
        canvas_origin: Point,
        session: &PreviewSession,
    ) {
        let scale_factor = scale_factor.max(0.01);
        let transform = scene_transform(session.view(), canvas_origin, scale_factor);
        unsafe {
            self.document.draw(
                ScenePass {
                    target: None,
                    width: surface_width,
                    height: surface_height,
                    source: session.view().document_bounds(),
                    transform,
                    include_draft: true,
                    clip: Some(session.view().document_bounds()),
                },
                session.editor(),
            );
        }

        self.canvas
            .set_size(surface_width, surface_height, scale_factor);
        self.canvas.save();
        self.canvas.translate(canvas_origin.x, canvas_origin.y);
        let canvas_bounds = session.view().canvas_bounds();
        self.canvas.scissor(
            canvas_bounds.left,
            canvas_bounds.top,
            canvas_bounds.width(),
            canvas_bounds.height(),
        );
        apply_view_transform(&mut self.canvas, session.view());
        paint_annotations(
            &mut self.canvas,
            session.editor(),
            session.view().document_bounds(),
            &self.fonts,
            &mut self.emojis,
            None,
        );
        self.canvas.restore();
        self.canvas.flush();
    }

    pub fn export(&mut self, session: &PreviewSession) -> Result<RgbaFrame> {
        let (width, height) = output_size(session);
        self.ensure_export_target(width, height)?;
        let target = self
            .export_target
            .as_ref()
            .expect("export target was just created");
        let framebuffer = target.gpu.framebuffer;
        let image = target.image;
        let transform = export_transform(session);

        unsafe {
            self.document.draw(
                ScenePass {
                    target: Some(framebuffer),
                    width,
                    height,
                    source: session.view().document_bounds(),
                    transform,
                    include_draft: false,
                    clip: None,
                },
                session.editor(),
            );
        }

        self.canvas.set_size(width, height, 1.0);
        self.canvas.set_render_target(RenderTarget::Image(image));
        self.canvas.save();
        apply_export_transform(&mut self.canvas, session);
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
        let source = session.image().bounds();
        RgbaFrame::new(RectI::new(source.left, source.top, width, height), pixels)
            .map_err(Into::into)
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
                return Err(anyhow!("register preview export texture: {error:?}"));
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

impl Drop for PreviewRenderer {
    fn drop(&mut self) {
        self.delete_export_target();
    }
}

fn scene_transform(
    view: ViewTransform,
    canvas_origin: Point,
    scale_factor: f32,
) -> DocumentTransform {
    let map = |point| (view.document_to_canvas(point) + canvas_origin) * scale_factor;
    let origin = map(Point::new(0.0, 0.0));
    DocumentTransform::from_basis(
        origin,
        map(Point::new(1.0, 0.0)) - origin,
        map(Point::new(0.0, 1.0)) - origin,
    )
}

fn export_transform(session: &PreviewSession) -> DocumentTransform {
    let bounds = session.view().document_bounds();
    match session.view().rotation() {
        QuarterTurn::Zero => DocumentTransform::from_basis(
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.0, 1.0),
        ),
        QuarterTurn::Clockwise90 => DocumentTransform::from_basis(
            Point::new(bounds.height(), 0.0),
            Point::new(0.0, 1.0),
            Point::new(-1.0, 0.0),
        ),
        QuarterTurn::Clockwise180 => DocumentTransform::from_basis(
            Point::new(bounds.width(), bounds.height()),
            Point::new(-1.0, 0.0),
            Point::new(0.0, -1.0),
        ),
        QuarterTurn::Clockwise270 => DocumentTransform::from_basis(
            Point::new(0.0, bounds.width()),
            Point::new(0.0, -1.0),
            Point::new(1.0, 0.0),
        ),
    }
}

fn apply_view_transform(canvas: &mut Canvas<OpenGl>, view: ViewTransform) {
    let document = view.document_bounds();
    let viewport = view.canvas_bounds();
    let pan = view.pan();
    canvas.translate(
        viewport.width() * 0.5 + pan.x,
        viewport.height() * 0.5 + pan.y,
    );
    canvas.rotate(view.rotation().angle_radians());
    canvas.scale(view.zoom(), view.zoom());
    canvas.translate(-document.width() * 0.5, -document.height() * 0.5);
}

fn apply_export_transform(canvas: &mut Canvas<OpenGl>, session: &PreviewSession) {
    let bounds = session.view().document_bounds();
    match session.view().rotation() {
        QuarterTurn::Zero => {}
        QuarterTurn::Clockwise90 => {
            canvas.translate(bounds.height(), 0.0);
            canvas.rotate(core::f32::consts::FRAC_PI_2);
        }
        QuarterTurn::Clockwise180 => {
            canvas.translate(bounds.width(), bounds.height());
            canvas.rotate(core::f32::consts::PI);
        }
        QuarterTurn::Clockwise270 => {
            canvas.translate(0.0, bounds.width());
            canvas.rotate(core::f32::consts::PI + core::f32::consts::FRAC_PI_2);
        }
    }
}

fn output_size(session: &PreviewSession) -> (u32, u32) {
    if session.view().rotation().swaps_axes() {
        (session.image().height(), session.image().width())
    } else {
        (session.image().width(), session.image().height())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(width: u32, height: u32) -> PreviewSession {
        PreviewSession::new(
            RgbaFrame::new(
                RectI::new(10, 20, width, height),
                vec![0; width as usize * height as usize * 4],
            )
            .unwrap(),
        )
    }

    #[test]
    fn preview_scene_transform_matches_the_model_at_high_dpi() {
        let mut preview = session(400, 200);
        preview.set_canvas_size(800.0, 600.0);
        preview.rotate_clockwise();
        let origin = Point::new(0.0, 86.0);
        let transform = scene_transform(preview.view(), origin, 1.5);
        let document = Point::new(37.0, 91.0);
        let expected = (preview.view().document_to_canvas(document) + origin) * 1.5;
        assert_eq!(transform.map(document), expected);
    }

    #[test]
    fn quarter_turns_choose_the_correct_export_dimensions() {
        let mut preview = session(400, 200);
        assert_eq!(output_size(&preview), (400, 200));
        preview.rotate_clockwise();
        assert_eq!(output_size(&preview), (200, 400));
        preview.rotate_clockwise();
        assert_eq!(output_size(&preview), (400, 200));
    }
}
