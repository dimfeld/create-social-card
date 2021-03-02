use anyhow::{Context, Result};
use glyph_brush_layout::ab_glyph::FontRef;
use serde_derive::Deserialize;
use std::borrow::Cow;
use std::path::PathBuf;
use structopt::StructOpt;

mod lib;
use lib::{overlay_text, OverlayOptions};

#[derive(Debug, StructOpt)]
struct Args {
    #[structopt(long = "config", short = "c", help = "configuration file")]
    config: PathBuf,

    #[structopt(long = "output", short = "o", help = "output path")]
    output: PathBuf,
}

#[derive(Deserialize)]
struct FontConfig {
    name: String,
    path: PathBuf,
}

#[derive(Deserialize)]
struct Config<'a> {
    background: PathBuf,
    fonts: Vec<FontConfig>,
    blocks: Vec<lib::Block<'a>>,
}

fn main() -> Result<()> {
    let args = Args::from_args();

    let config: Config = {
        let config_contents =
            std::fs::read_to_string(&args.config).context("Opening config file")?;
        toml::from_str(&config_contents).context("Parsing config file")?
    };

    let bg = image::open(&config.background).context("Opening background image")?;

    let font_data = config
        .fonts
        .into_iter()
        .map(|f| {
            let font_data = std::fs::read(&f.path)
                .with_context(|| format!("Opening font file {:?}", f.path))?;
            Ok((f, font_data))
        })
        .collect::<Result<Vec<_>>>()?;

    let fonts = font_data
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let font = FontRef::try_from_slice_and_index(&f.1, i as u32)
                .with_context(|| format!("Loading font {:?}", f.0.path))?;
            Ok(lib::FontDef {
                name: Cow::from(&f.0.name),
                font,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let options = OverlayOptions {
        background: bg,
        fonts: &fonts,
        blocks: &config.blocks,
    };

    let result = overlay_text(&options)?;
    result.save(&args.output)?;

    Ok(())
}
