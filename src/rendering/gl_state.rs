use glow::HasContext;

pub(crate) struct OpenGlStateSnapshot {
    draw_framebuffer: Option<glow::Framebuffer>,
    read_framebuffer: Option<glow::Framebuffer>,
    renderbuffer: Option<glow::Renderbuffer>,
    program: Option<glow::Program>,
    vertex_array: Option<glow::VertexArray>,
    array_buffer: Option<glow::Buffer>,
    viewport: [i32; 4],
    scissor_box: [i32; 4],
    active_texture: u32,
    texture_zero: TextureUnitState,
    texture_one: TextureUnitState,
    active_texture_state: TextureUnitState,
    blend_enabled: bool,
    depth_enabled: bool,
    stencil_enabled: bool,
    cull_enabled: bool,
    scissor_enabled: bool,
    blend_equation_rgb: u32,
    blend_equation_alpha: u32,
    blend_source_rgb: u32,
    blend_destination_rgb: u32,
    blend_source_alpha: u32,
    blend_destination_alpha: u32,
    blend_color: [f32; 4],
    color_mask: [bool; 4],
    depth_mask: bool,
    depth_function: u32,
    stencil_front: StencilState,
    stencil_back: StencilState,
    cull_face: u32,
    front_face: u32,
    pack_alignment: i32,
    unpack_alignment: i32,
    unpack_row_length: i32,
    unpack_skip_pixels: i32,
    unpack_skip_rows: i32,
    clear_color: [f32; 4],
}

struct TextureUnitState {
    unit: u32,
    texture: Option<glow::Texture>,
    sampler: Option<glow::Sampler>,
}

struct StencilState {
    function: u32,
    reference: i32,
    value_mask: u32,
    write_mask: u32,
    fail: u32,
    depth_fail: u32,
    depth_pass: u32,
}

impl OpenGlStateSnapshot {
    pub(crate) unsafe fn capture(gl: &glow::Context) -> Self {
        let active_texture = unsafe { gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32 };
        let texture_zero = unsafe { TextureUnitState::capture(gl, glow::TEXTURE0) };
        let texture_one = unsafe { TextureUnitState::capture(gl, glow::TEXTURE0 + 1) };
        let active_texture_state = unsafe { TextureUnitState::capture(gl, active_texture) };
        unsafe { gl.active_texture(active_texture) };

        Self {
            draw_framebuffer: unsafe {
                gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING)
            },
            read_framebuffer: unsafe {
                gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING)
            },
            renderbuffer: unsafe { gl.get_parameter_renderbuffer(glow::RENDERBUFFER_BINDING) },
            program: unsafe { gl.get_parameter_program(glow::CURRENT_PROGRAM) },
            vertex_array: unsafe { gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING) },
            array_buffer: unsafe { gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING) },
            viewport: unsafe { parameter_i32_array(gl, glow::VIEWPORT) },
            scissor_box: unsafe { parameter_i32_array(gl, glow::SCISSOR_BOX) },
            active_texture,
            texture_zero,
            texture_one,
            active_texture_state,
            blend_enabled: unsafe { gl.is_enabled(glow::BLEND) },
            depth_enabled: unsafe { gl.is_enabled(glow::DEPTH_TEST) },
            stencil_enabled: unsafe { gl.is_enabled(glow::STENCIL_TEST) },
            cull_enabled: unsafe { gl.is_enabled(glow::CULL_FACE) },
            scissor_enabled: unsafe { gl.is_enabled(glow::SCISSOR_TEST) },
            blend_equation_rgb: unsafe { gl.get_parameter_i32(glow::BLEND_EQUATION_RGB) as u32 },
            blend_equation_alpha: unsafe {
                gl.get_parameter_i32(glow::BLEND_EQUATION_ALPHA) as u32
            },
            blend_source_rgb: unsafe { gl.get_parameter_i32(glow::BLEND_SRC_RGB) as u32 },
            blend_destination_rgb: unsafe { gl.get_parameter_i32(glow::BLEND_DST_RGB) as u32 },
            blend_source_alpha: unsafe { gl.get_parameter_i32(glow::BLEND_SRC_ALPHA) as u32 },
            blend_destination_alpha: unsafe { gl.get_parameter_i32(glow::BLEND_DST_ALPHA) as u32 },
            blend_color: unsafe { parameter_f32_array(gl, glow::BLEND_COLOR) },
            color_mask: unsafe { gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK) },
            depth_mask: unsafe { gl.get_parameter_bool(glow::DEPTH_WRITEMASK) },
            depth_function: unsafe { gl.get_parameter_i32(glow::DEPTH_FUNC) as u32 },
            stencil_front: unsafe { StencilState::capture_front(gl) },
            stencil_back: unsafe { StencilState::capture_back(gl) },
            cull_face: unsafe { gl.get_parameter_i32(glow::CULL_FACE_MODE) as u32 },
            front_face: unsafe { gl.get_parameter_i32(glow::FRONT_FACE) as u32 },
            pack_alignment: unsafe { gl.get_parameter_i32(glow::PACK_ALIGNMENT) },
            unpack_alignment: unsafe { gl.get_parameter_i32(glow::UNPACK_ALIGNMENT) },
            unpack_row_length: unsafe { gl.get_parameter_i32(glow::UNPACK_ROW_LENGTH) },
            unpack_skip_pixels: unsafe { gl.get_parameter_i32(glow::UNPACK_SKIP_PIXELS) },
            unpack_skip_rows: unsafe { gl.get_parameter_i32(glow::UNPACK_SKIP_ROWS) },
            clear_color: unsafe { parameter_f32_array(gl, glow::COLOR_CLEAR_VALUE) },
        }
    }

    pub(crate) fn draw_framebuffer(&self) -> Option<glow::Framebuffer> {
        self.draw_framebuffer
    }

    pub(crate) unsafe fn restore(self, gl: &glow::Context) {
        unsafe {
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.draw_framebuffer);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read_framebuffer);
            gl.bind_renderbuffer(glow::RENDERBUFFER, self.renderbuffer);
            gl.viewport(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            gl.scissor(
                self.scissor_box[0],
                self.scissor_box[1],
                self.scissor_box[2],
                self.scissor_box[3],
            );
            gl.use_program(self.program);
            gl.bind_vertex_array(self.vertex_array);
            gl.bind_buffer(glow::ARRAY_BUFFER, self.array_buffer);

            self.texture_zero.restore(gl);
            self.texture_one.restore(gl);
            self.active_texture_state.restore(gl);
            gl.active_texture(self.active_texture);

            gl.blend_equation_separate(self.blend_equation_rgb, self.blend_equation_alpha);
            gl.blend_func_separate(
                self.blend_source_rgb,
                self.blend_destination_rgb,
                self.blend_source_alpha,
                self.blend_destination_alpha,
            );
            gl.blend_color(
                self.blend_color[0],
                self.blend_color[1],
                self.blend_color[2],
                self.blend_color[3],
            );
            gl.color_mask(
                self.color_mask[0],
                self.color_mask[1],
                self.color_mask[2],
                self.color_mask[3],
            );
            gl.depth_mask(self.depth_mask);
            gl.depth_func(self.depth_function);
            self.stencil_front.restore(gl, glow::FRONT);
            self.stencil_back.restore(gl, glow::BACK);
            gl.cull_face(self.cull_face);
            gl.front_face(self.front_face);

            restore_capability(gl, glow::BLEND, self.blend_enabled);
            restore_capability(gl, glow::DEPTH_TEST, self.depth_enabled);
            restore_capability(gl, glow::STENCIL_TEST, self.stencil_enabled);
            restore_capability(gl, glow::CULL_FACE, self.cull_enabled);
            restore_capability(gl, glow::SCISSOR_TEST, self.scissor_enabled);

            gl.pixel_store_i32(glow::PACK_ALIGNMENT, self.pack_alignment);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, self.unpack_alignment);
            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, self.unpack_row_length);
            gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, self.unpack_skip_pixels);
            gl.pixel_store_i32(glow::UNPACK_SKIP_ROWS, self.unpack_skip_rows);
            gl.clear_color(
                self.clear_color[0],
                self.clear_color[1],
                self.clear_color[2],
                self.clear_color[3],
            );
        }
    }
}

impl TextureUnitState {
    unsafe fn capture(gl: &glow::Context, unit: u32) -> Self {
        unsafe { gl.active_texture(unit) };
        Self {
            unit,
            texture: unsafe { gl.get_parameter_texture(glow::TEXTURE_BINDING_2D) },
            sampler: unsafe { gl.get_parameter_sampler(glow::SAMPLER_BINDING) },
        }
    }

    unsafe fn restore(self, gl: &glow::Context) {
        unsafe {
            gl.active_texture(self.unit);
            gl.bind_texture(glow::TEXTURE_2D, self.texture);
            gl.bind_sampler(self.unit - glow::TEXTURE0, self.sampler);
        }
    }
}

impl StencilState {
    unsafe fn capture_front(gl: &glow::Context) -> Self {
        Self {
            function: unsafe { gl.get_parameter_i32(glow::STENCIL_FUNC) as u32 },
            reference: unsafe { gl.get_parameter_i32(glow::STENCIL_REF) },
            value_mask: unsafe { gl.get_parameter_i32(glow::STENCIL_VALUE_MASK) as u32 },
            write_mask: unsafe { gl.get_parameter_i32(glow::STENCIL_WRITEMASK) as u32 },
            fail: unsafe { gl.get_parameter_i32(glow::STENCIL_FAIL) as u32 },
            depth_fail: unsafe { gl.get_parameter_i32(glow::STENCIL_PASS_DEPTH_FAIL) as u32 },
            depth_pass: unsafe { gl.get_parameter_i32(glow::STENCIL_PASS_DEPTH_PASS) as u32 },
        }
    }

    unsafe fn capture_back(gl: &glow::Context) -> Self {
        Self {
            function: unsafe { gl.get_parameter_i32(glow::STENCIL_BACK_FUNC) as u32 },
            reference: unsafe { gl.get_parameter_i32(glow::STENCIL_BACK_REF) },
            value_mask: unsafe { gl.get_parameter_i32(glow::STENCIL_BACK_VALUE_MASK) as u32 },
            write_mask: unsafe { gl.get_parameter_i32(glow::STENCIL_BACK_WRITEMASK) as u32 },
            fail: unsafe { gl.get_parameter_i32(glow::STENCIL_BACK_FAIL) as u32 },
            depth_fail: unsafe { gl.get_parameter_i32(glow::STENCIL_BACK_PASS_DEPTH_FAIL) as u32 },
            depth_pass: unsafe { gl.get_parameter_i32(glow::STENCIL_BACK_PASS_DEPTH_PASS) as u32 },
        }
    }

    unsafe fn restore(self, gl: &glow::Context, face: u32) {
        unsafe {
            gl.stencil_func_separate(face, self.function, self.reference, self.value_mask);
            gl.stencil_mask_separate(face, self.write_mask);
            gl.stencil_op_separate(face, self.fail, self.depth_fail, self.depth_pass);
        }
    }
}

unsafe fn parameter_i32_array<const N: usize>(gl: &glow::Context, parameter: u32) -> [i32; N] {
    let mut values = [0; N];
    unsafe { gl.get_parameter_i32_slice(parameter, &mut values) };
    values
}

unsafe fn parameter_f32_array<const N: usize>(gl: &glow::Context, parameter: u32) -> [f32; N] {
    let mut values = [0.0; N];
    unsafe { gl.get_parameter_f32_slice(parameter, &mut values) };
    values
}

unsafe fn restore_capability(gl: &glow::Context, capability: u32, enabled: bool) {
    if enabled {
        unsafe { gl.enable(capability) };
    } else {
        unsafe { gl.disable(capability) };
    }
}
