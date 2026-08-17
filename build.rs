fn main() {
    let libraries = std::collections::HashMap::from([(
        slint_borderless::SLINT_LIBRARY_NAME.to_owned(),
        slint_borderless::slint_library_path(),
    )]);
    let config = slint_build::CompilerConfiguration::new().with_library_paths(libraries);
    slint_build::compile_with_config("ui/app.slint", config).unwrap();
}
