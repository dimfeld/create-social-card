use anyhow::{Context, Result};
use glyph_brush_layout::ab_glyph::FontRef;
use serde_derive::Deserialize;
use std::path::PathBuf;
use structopt::StructOpt;

mod lib;
use lib::{overlay_text, OverlayOptions, Rect};

#[derive(Debug, StructOpt)]
struct Args {
    #[structopt(long = "config", short = "c", help = "configuration file")]
    config: PathBuf,

    #[structopt(long = "output", short = "o", help = "output path")]
    output: PathBuf,

    #[structopt(long = "text", short = "t", help = "the text to render")]
    text: String,
}

#[derive(Deserialize)]
struct Config {
    background: PathBuf,
    font: PathBuf,
    text_rect: Rect,
    max_size: Option<f32>,
    min_size: Option<f32>,
    color: String,
}

fn main() -> Result<()> {
    let args = Args::from_args();

    let config: Config = {
        let config_contents =
            std::fs::read_to_string(&args.config).context("Opening config file")?;
        toml::from_str(&config_contents).context("Parsing config file")?
    };

    let bg = image::open(&config.background).context("Opening background image")?;

    let max_size = config.max_size.unwrap_or(64.0);
    let min_size = config.min_size.unwrap_or(6.0);

    let font_data = std::fs::read(&config.font).context("Opening font file")?;
    let font = FontRef::try_from_slice(&font_data).context("Loading font")?;

    let options = OverlayOptions {
        text: &args.text,
        background: bg,
        font: &font,
        text_rect: &config.text_rect,
        min_size,
        max_size,
        color: &config.color,
    };

    let result = overlay_text(&options)?;
    result.save(&args.output)?;

    Ok(())
}
