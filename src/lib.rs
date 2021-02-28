use anyhow::{anyhow, Context, Result};
use glyph_brush_layout::{
    ab_glyph::{Font, FontRef, PxScale},
    FontId, GlyphPositioner, Layout, SectionGeometry, SectionGlyph, SectionText,
};
use image::{GenericImageView, ImageBuffer, Rgba};
use serde_derive::Deserialize;
use std::borrow::Cow;
use std::convert::{TryFrom, TryInto};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Color<'a> {
    Rgb(u8, u8, u8),
    Rgba(u8, u8, u8, u8),
    RgbString(Cow<'a, str>),
}

impl<'a> Default for Color<'a> {
    fn default() -> Color<'a> {
        Color::Rgb(0, 0, 0)
    }
}

impl<'a> TryFrom<Color<'a>> for Rgba<u8> {
    type Error = anyhow::Error;

    fn try_from(val: Color) -> Result<Rgba<u8>> {
        match val {
            Color::Rgb(r, g, b) => Ok(Rgba([r, g, b, 255])),
            Color::Rgba(r, g, b, a) => Ok(Rgba([r, g, b, a])),
            Color::RgbString(s) => parse_color(&s),
        }
    }
}

#[derive(Debug)]
pub struct OverlayOptions<'a> {
    pub background: image::DynamicImage,
    pub paragraphs: Vec<Paragraph<'a>>,
    pub fonts: Vec<FontRef<'a>>,
}

#[derive(Copy, Clone, Debug, Deserialize)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

impl Default for HAlign {
    fn default() -> HAlign {
        HAlign::Left
    }
}

impl From<HAlign> for glyph_brush_layout::HorizontalAlign {
    fn from(v: HAlign) -> glyph_brush_layout::HorizontalAlign {
        match v {
            HAlign::Left => glyph_brush_layout::HorizontalAlign::Left,
            HAlign::Center => glyph_brush_layout::HorizontalAlign::Center,
            HAlign::Right => glyph_brush_layout::HorizontalAlign::Right,
        }
    }
}

#[derive(Copy, Clone, Debug, Deserialize)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

impl Default for VAlign {
    fn default() -> VAlign {
        VAlign::Top
    }
}

impl From<VAlign> for glyph_brush_layout::VerticalAlign {
    fn from(v: VAlign) -> glyph_brush_layout::VerticalAlign {
        match v {
            VAlign::Top => glyph_brush_layout::VerticalAlign::Top,
            VAlign::Center => glyph_brush_layout::VerticalAlign::Center,
            VAlign::Bottom => glyph_brush_layout::VerticalAlign::Bottom,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ParagraphBorder<'a> {
    #[serde(default)]
    pub width: usize,
    #[serde(default)]
    pub color: Color<'a>,
    pub shadow: Option<Shadow<'a>>,
}

#[derive(Debug, Deserialize)]
pub struct Paragraph<'a> {
    pub min_size: f32,
    pub max_size: f32,
    pub text: Vec<Text<'a>>,
    pub rect: Rect,
    pub shadow: Option<Shadow<'a>>,
    pub background: Option<(u8, u8, u8, u8)>,
    pub border: Option<ParagraphBorder<'a>>,
    #[serde(default)]
    pub wrap: bool,
    #[serde(default)]
    pub h_align: HAlign,
    #[serde(default)]
    pub v_align: VAlign,

    /// Text blocks in a paragraph that do not have their own color will inherit it from this color.
    #[serde(default)]
    pub color: Color<'a>,
}

#[derive(Debug, Deserialize)]
pub struct Text<'a> {
    pub font_index: usize,
    pub text: Cow<'a, str>,
    #[serde(default)]
    pub color: Color<'a>,
}

#[derive(Debug, Deserialize)]
pub struct Shadow<'a> {
    pub x: u32,
    pub y: u32,
    pub blur: Option<f32>,
    #[serde(default)]
    pub color: Color<'a>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Rect {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

fn pt_size_to_px_scale<F: Font>(font: &F, pt_size: f32, screen_scale_factor: f32) -> PxScale {
    let px_per_em = pt_size * screen_scale_factor * (96.0 / 72.0);
    let units_per_em = font.units_per_em().unwrap();
    let height = font.height_unscaled();
    PxScale::from(px_per_em * height / units_per_em)
}

fn fit_glyphs(fonts: &[FontRef], options: &Paragraph) -> Result<Vec<SectionGlyph>> {
    let text_width = options.rect.right - options.rect.left;
    let text_height = options.rect.bottom - options.rect.top;

    let geometry = SectionGeometry {
        screen_position: (options.rect.left, options.rect.top),
        bounds: (text_width, text_height),
    };

    let layout = if options.wrap {
        Layout::Wrap {
            line_breaker: glyph_brush_layout::BuiltInLineBreaker::UnicodeLineBreaker,
            h_align: options.h_align.into(),
            v_align: options.v_align.into(),
        }
    } else {
        Layout::SingleLine {
            line_breaker: glyph_brush_layout::BuiltInLineBreaker::UnicodeLineBreaker,
            h_align: options.h_align.into(),
            v_align: options.v_align.into(),
        }
    };

    let mut font_size = options.max_size;

    let mut sections = options
        .text
        .iter()
        .map(|t| SectionText {
            text: &t.text,
            font_id: FontId(t.font_index),
            scale: PxScale::from(0.0), // This will be filled in later
        })
        .collect::<Vec<_>>();

    while font_size > options.min_size {
        // println!("Trying font size {font_size}", font_size = font_size);
        for i in sections.iter_mut() {
            i.scale = pt_size_to_px_scale(&fonts[i.font_id], font_size, 1.0);
        }

        let glyphs = layout.calculate_glyphs(fonts, &geometry, &sections);

        let last_glyph = glyphs.last().unwrap();
        println!("size {}, {:?}", font_size, last_glyph);
        let text_bottom = last_glyph.glyph.position.y + last_glyph.glyph.scale.y;
        if text_bottom > options.rect.bottom {
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

fn parse_color(color: &str) -> Result<Rgba<u8>> {
    let hex = if color.starts_with('#') {
        &color[1..]
    } else {
        color
    };

    let mut color = u32::from_str_radix(color, 16).context("color")?;
    let mut alpha: u8 = 255;
    if hex.len() == 8 {
        alpha = (color & 0xFF) as u8;
        color = color >> 8;
    }

    if hex.len() == 6 {
        let red: u8 = ((color >> 16) & 0xFF) as u8;
        let green: u8 = ((color >> 8) & 0xFF) as u8;
        let blue: u8 = ((color) & 0xFF) as u8;

        Ok(Rgba([red, green, blue, alpha]))
    } else {
        Err(anyhow!("Color must be 6 or 8 hex digits"))
    }
}

// TODO Proper library errors instead of anyhow
pub fn overlay_text(options: &OverlayOptions) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let mut bg = options.background.to_rgba8();
    let (width, height) = bg.dimensions();
    let width_f32 = width as f32;
    let height_f32 = height as f32;

    for paragraph in &options.paragraphs {
        if paragraph.rect.left > width_f32
            || paragraph.rect.right > width_f32
            || paragraph.rect.top > height_f32
            || paragraph.rect.bottom > height_f32
        {
            return Err(anyhow!(
                "Text rect {rect:?} does not fit in image of size {width}x{height}",
                rect = paragraph.rect,
                width = width,
                height = height
            ));
        } else if paragraph.rect.left >= paragraph.rect.right
            || paragraph.rect.top > paragraph.rect.bottom
        {
            return Err(anyhow!("rect must not have a negative size"));
        }

        let glyphs = fit_glyphs(&options.fonts, paragraph)?;
        // TODO Make all this actually work on a per-paragraph basis

        let shadow_color = paragraph
            .shadow
            .map(|s| s.color.try_into())
            .transpose()?
            .unwrap_or_else(|| Rgba([128, 128, 128, 255]));

        let mut text_image = image::RgbaImage::new(width, height);
        let mut shadow_image = paragraph
            .shadow
            .map(|s| (s, image::RgbaImage::new(width, height)));

        for glyph in glyphs {
            // println!("{:?}", glyph);
            let glyph_font = &options.fonts.as_slice()[glyph.font_id];
            if let Some(g) = glyph_font.outline_glyph(glyph.glyph) {
                // println!("{:?}", g.px_bounds());
                let r = g.px_bounds();
                let x_base = r.min.x as u32;
                let y_base = r.min.y as u32;
                g.draw(|x, y, c| {
                    let alpha = (c * 255.0) as u8;
                    // println!("{x}, {y}, {c}", x = x, y = y, c = c);
                    let mut pixel = image::Rgba([red, green, blue, alpha]);
                    text_image.put_pixel(x_base + x, y_base + y, pixel);

                    if let Some((s, i)) = shadow_image.as_mut() {
                        let shadow_x = x_base + x + s.x;
                        let shadow_y = y_base + y + s.y;
                        if i.in_bounds(shadow_x, shadow_y) {
                            let mut pixel = shadow_color.clone();
                            if c < 1.0 {
                                pixel[3] = ((pixel[3] as f32) * c) as u8;
                            }

                            i.put_pixel(shadow_x, shadow_y, pixel);
                        }
                    }
                })
            }
        }

        if let Some((s, i)) = shadow_image {
            let i = s
                .blur
                .map(|blur_sigma| image::imageops::blur(&i, blur_sigma))
                .unwrap_or(i);
            image::imageops::overlay(&mut bg, &i, 0, 0);
        }

        image::imageops::overlay(&mut bg, &text_image, 0, 0);
    }

    Ok(bg)
}
