mod app;
mod model;
mod ocr;
mod platform;
mod rendering;
mod scroll;
mod settings;
mod ui;

fn main() -> anyhow::Result<()> {
    app::run()
}
