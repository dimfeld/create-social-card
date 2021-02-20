use anyhow::{anyhow, Context, Result};
use glyph_brush_layout::{
    ab_glyph::{FontRef, PxScale},
    FontId, GlyphPositioner, Layout, SectionGeometry, SectionText,
};
use image::{GenericImage, GenericImageView};
use serde_derive::Deserialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Args {
    #[structopt(long = "config", short = "c", help = "configuration file")]
    config: PathBuf,

    #[structopt(long = "output", short = "o", help = "output path")]
    output: PathBuf,

    #[structopt(long = "text", short = "t", help = "the text to render")]
    text: String,
}

#[derive(Debug, Deserialize)]
struct Rect {
    top: f32,
    bottom: f32,
    left: f32,
    right: f32,
}

#[derive(Deserialize)]
struct Config {
    background: PathBuf,
    font: PathBuf,
    text_rect: Rect,
    color: String,
}

fn main() -> Result<()> {
    let args = Args::from_args();

    let config: Config = {
        let mut config_file = std::fs::File::open(args.config).context("Opening config file")?;
        let mut config_contents = String::new();
        config_file.read_to_string(&mut config_contents)?;
        toml::from_str(&config_contents).context("Parsing config file")?
    };

    let mut bg = image::open(config.background).context("Opening background image")?;
    let (width, height) = bg.dimensions();
    let width = width as f32;
    let height = height as f32;

    if config.text_rect.left > width
        || config.text_rect.right > width
        || config.text_rect.top > height
        || config.text_rect.bottom > height
    {
        return Err(anyhow!(
            "Text rect {rect:?} does not fit in image of size {width}x{height}",
            rect = config.text_rect,
            width = width,
            height = height
        ));
    } else if config.text_rect.left >= config.text_rect.right
        || config.text_rect.top > config.text_rect.bottom
    {
        return Err(anyhow!("text_rect must not have a negative size"));
    }

    let text_width = config.text_rect.right - config.text_rect.left;
    let text_height = config.text_rect.bottom - config.text_rect.top;

    let font_data = {
        let mut font_file = std::fs::File::open(&config.font).context("Opening font file")?;
        let mut font_data: Vec<u8> = Vec::new();
        font_file.read_to_end(&mut font_data)?;
        font_data
    };

    let font = FontRef::try_from_slice(&font_data).context("Loading font")?;

    let geometry = SectionGeometry {
        screen_position: (config.text_rect.left, config.text_rect.top),
        bounds: (text_width, text_height),
    };

    let section_text = SectionText {
        text: &args.text,
        font_id: FontId(0),
        scale: PxScale::from(48.0),
    };

    let layout = Layout::Wrap {
        line_breaker: glyph_brush_layout::BuiltInLineBreaker::UnicodeLineBreaker,
        h_align: glyph_brush_layout::HorizontalAlign::Left,
        v_align: glyph_brush_layout::VerticalAlign::Center,
    };
    let glyphs = layout.calculate_glyphs(&[font], &geometry, &[section_text]);

    Ok(())
}
