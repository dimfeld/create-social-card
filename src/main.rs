use anyhow::{anyhow, Context, Result};
use glyph_brush_layout::{
    ab_glyph::{Font, FontRef, PxScale},
    FontId, GlyphPositioner, Layout, SectionGeometry, SectionGlyph, SectionText,
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
    max_size: Option<f32>,
    min_size: Option<f32>,
    color: String,
}

fn pt_size_to_px_scale<F: Font>(font: &F, pt_size: f32, screen_scale_factor: f32) -> PxScale {
    let px_per_em = pt_size * screen_scale_factor * (96.0 / 72.0);
    let units_per_em = font.units_per_em().unwrap();
    let height = font.height_unscaled();
    PxScale::from(px_per_em * height / units_per_em)
}

fn fit_glyphs(config: &Config, args: &Args, font: &FontRef) -> Result<Vec<SectionGlyph>> {
    let text_width = config.text_rect.right - config.text_rect.left;
    let text_height = config.text_rect.bottom - config.text_rect.top;

    let geometry = SectionGeometry {
        screen_position: (config.text_rect.left, config.text_rect.top),
        bounds: (text_width, text_height),
    };

    let layout = Layout::Wrap {
        line_breaker: glyph_brush_layout::BuiltInLineBreaker::UnicodeLineBreaker,
        h_align: glyph_brush_layout::HorizontalAlign::Left,
        v_align: glyph_brush_layout::VerticalAlign::Top,
    };

    let max_size = config.max_size.unwrap_or(64.0);
    let min_size = config.min_size.unwrap_or(6.0);
    let mut font_size = max_size;

    while font_size > min_size {
        // println!("Trying font size {font_size}", font_size = font_size);
        let section_text = SectionText {
            text: &args.text,
            font_id: FontId(0),
            scale: pt_size_to_px_scale(font, font_size, 1.0),
        };

        let glyphs = layout.calculate_glyphs(&[&font], &geometry, &[section_text]);

        let last_glyph = glyphs.last().unwrap();
        println!("size {}, {:?}", font_size, last_glyph);
        let text_bottom = last_glyph.glyph.position.y + last_glyph.glyph.scale.y;
        if text_bottom > config.text_rect.bottom {
            font_size -= 4.0;
        } else {
            println!("Chose font size {}", font_size);
            return Ok(glyphs);
        }
    }

    Err(anyhow!("Could not fit text in rectangle"))
}

fn blend(a: u8, b: u8, alpha: f32) -> u8 {
    let a = (a as f32) * (1.0 - alpha);
    let b = (b as f32) * alpha;
    (a + b) as u8
}

fn main() -> Result<()> {
    let args = Args::from_args();

    let config: Config = {
        let config_contents =
            std::fs::read_to_string(&args.config).context("Opening config file")?;
        toml::from_str(&config_contents).context("Parsing config file")?
    };

    let mut bg = image::open(&config.background)
        .context("Opening background image")?
        .to_rgba8();
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

    let font_data = std::fs::read(&config.font).context("Opening font file")?;
    let font = FontRef::try_from_slice(&font_data).context("Loading font")?;

    let glyphs = fit_glyphs(&config, &args, &font)?;

    let red: u8 = 200;
    let green: u8 = 100;
    let blue: u8 = 100;

    for glyph in glyphs {
        println!("{:?}", glyph);
        if let Some(g) = font.outline_glyph(glyph.glyph) {
            println!("{:?}", g.px_bounds());
            let r = g.px_bounds();
            g.draw(|x, y, c| {
                // TODO Blend using the c value
                // println!("{x}, {y}, {c}", x = x, y = y, c = c);
                let pixel = bg.get_pixel_mut(r.min.x as u32 + x, r.min.y as u32 + y);
                let image::Rgba(bg_pixel) = *pixel;
                *pixel = image::Rgba([
                    blend(bg_pixel[0], red, c),
                    blend(bg_pixel[1], green, c),
                    blend(bg_pixel[2], blue, c),
                    bg_pixel[3],
                ]);
            })
        }
    }

    bg.save(&args.output)?;

    Ok(())
}
