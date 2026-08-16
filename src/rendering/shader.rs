use anyhow::{Result, anyhow};
use glow::HasContext;

pub(super) unsafe fn compile_program(
    gl: &glow::Context,
    vertex_source: &str,
    fragment_source: &str,
    name: &str,
) -> Result<glow::Program> {
    let program = unsafe { gl.create_program() }
        .map_err(|error| anyhow!("create {name} shader program: {error}"))?;
    let mut shaders = Vec::with_capacity(2);
    for (kind, source, label) in [
        (glow::VERTEX_SHADER, vertex_source, "vertex"),
        (glow::FRAGMENT_SHADER, fragment_source, "fragment"),
    ] {
        let shader = unsafe { gl.create_shader(kind) }
            .map_err(|error| anyhow!("create {name} {label} shader: {error}"))?;
        unsafe {
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
        }
        if !unsafe { gl.get_shader_compile_status(shader) } {
            let log = unsafe { gl.get_shader_info_log(shader) };
            unsafe {
                gl.delete_shader(shader);
                for shader in shaders {
                    gl.delete_shader(shader);
                }
                gl.delete_program(program);
            }
            return Err(anyhow!("compile {name} {label} shader: {log}"));
        }
        unsafe { gl.attach_shader(program, shader) };
        shaders.push(shader);
    }
    unsafe { gl.link_program(program) };
    if !unsafe { gl.get_program_link_status(program) } {
        let log = unsafe { gl.get_program_info_log(program) };
        unsafe {
            for shader in shaders {
                gl.delete_shader(shader);
            }
            gl.delete_program(program);
        }
        return Err(anyhow!("link {name} shader program: {log}"));
    }
    unsafe {
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
    }
    Ok(program)
}
