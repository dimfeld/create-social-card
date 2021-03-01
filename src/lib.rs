use anyhow::{anyhow, Context, Result};
use glyph_brush_layout::{
    ab_glyph::{Font, FontRef, PxScale},
    FontId, GlyphPositioner, Layout, SectionGeometry, SectionGlyph, SectionText,
};
use image::{GenericImageView, ImageBuffer, Rgba};
use serde_derive::Deserialize;
use std::borrow::Cow;
use std::convert::TryFrom;

type Pixel = image::Rgba<u8>;

fn pixel(red: u8, green: u8, blue: u8, alpha: u8) -> Pixel {
    Rgba([red, green, blue, alpha])
}

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

impl<'a> TryFrom<&Color<'a>> for Pixel {
    type Error = anyhow::Error;

    fn try_from(val: &Color) -> Result<Pixel> {
        match val {
            Color::Rgb(r, g, b) => Ok(pixel(*r, *g, *b, 255)),
            Color::Rgba(r, g, b, a) => Ok(pixel(*r, *g, *b, *a)),
            Color::RgbString(s) => parse_color(s),
        }
    }
}

#[derive(Debug)]
pub struct OverlayOptions<'a> {
    pub background: image::DynamicImage,
    pub blocks: Vec<Block<'a>>,
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
pub struct BlockBorder<'a> {
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub color: Color<'a>,
    // pub shadow: Option<Shadow<'a>>,
}

fn bool_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct Block<'a> {
    pub min_size: f32,
    pub max_size: f32,
    pub text: Vec<Text<'a>>,
    pub rect: Rect,
    pub shadow: Option<Shadow<'a>>,
    pub background: Option<Color<'a>>,
    pub border: Option<BlockBorder<'a>>,
    /// Wrap the text. Defaults to true
    #[serde(default = "bool_true")]
    pub wrap: bool,
    #[serde(default)]
    pub h_align: HAlign,
    #[serde(default)]
    pub v_align: VAlign,

    /// Text runs in a block that do not have their own color will inherit it from this color.
    #[serde(default)]
    pub color: Color<'a>,
}

#[derive(Debug, Deserialize)]
pub struct Text<'a> {
    pub font_index: usize,
    pub text: Cow<'a, str>,
    pub color: Option<Color<'a>>,
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
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

fn pt_size_to_px_scale<F: Font>(font: &F, pt_size: f32, screen_scale_factor: f32) -> PxScale {
    let px_per_em = pt_size * screen_scale_factor * (96.0 / 72.0);
    let units_per_em = font.units_per_em().unwrap();
    let height = font.height_unscaled();
    PxScale::from(px_per_em * height / units_per_em)
}

fn fit_glyphs(fonts: &[FontRef], options: &Block) -> Result<Vec<SectionGlyph>> {
    let text_width = options.rect.right - options.rect.left;
    let text_height = options.rect.bottom - options.rect.top;

    let geometry = SectionGeometry {
        screen_position: (options.rect.left as f32, options.rect.top as f32),
        bounds: (text_width as f32, text_height as f32),
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
        if text_bottom > options.rect.bottom as f32 {
            font_size -= 4.0;
        } else {
            println!("Chose font size {}", font_size);
            return Ok(glyphs);
        }
    }

    Err(anyhow!("Could not fit text in rectangle"))
}

fn parse_color(color: &str) -> Result<Pixel> {
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

        Ok(pixel(red, green, blue, alpha))
    } else {
        Err(anyhow!("Color must be 6 or 8 hex digits"))
    }
}

// TODO Proper library errors instead of anyhow
pub fn overlay_text(options: &OverlayOptions) -> Result<ImageBuffer<Pixel, Vec<u8>>> {
    let mut bg = options.background.to_rgba8();
    let (width, height) = bg.dimensions();
    let width_f32 = width as f32;
    let height_f32 = height as f32;

    for block in &options.blocks {
        let mut rect = block.rect.clone();
        if rect.left > width || rect.right > width || rect.top > height || rect.bottom > height {
            return Err(anyhow!(
                "Text rect {rect:?} does not fit in image of size {width}x{height}",
                rect = rect,
                width = width,
                height = height
            ));
        } else if rect.left >= rect.right || rect.top > rect.bottom {
            return Err(anyhow!("rect must not have a negative size"));
        }

        let glyphs = fit_glyphs(&options.fonts, block)?;

        let shadow_color = block
            .shadow
            .as_ref()
            .map(|s| Pixel::try_from(&s.color))
            .transpose()?
            .unwrap_or_else(|| pixel(128, 128, 128, 255));

        let border_pixel = block
            .border
            .as_ref()
            .map(|b| Pixel::try_from(&b.color))
            .transpose()?
            .unwrap_or_else(|| pixel(0, 0, 0, 255));
        let border_width = block.border.as_ref().map(|b| b.width).unwrap_or(0);
        let transparent = pixel(0, 0, 0, ]);

        let bg_pixel = block
            .background
            .as_ref()
            .map(Pixel::try_from)
            .transpose()?
            .unwrap_or_else(|| pixel(0, 0, 0, 0));
        let border_left = rect.left + border_width;
        let border_right = rect.right - border_width;
        let border_top = rect.top + border_width;
        let border_bottom = rect.bottom - border_width;
        let mut text_image = image::RgbaImage::from_fn(width, height, |x, y| {
            if x < rect.left || x > rect.right || y < rect.top || y > rect.bottom {
                transparent
            } else if x < border_left || x > border_right || y < border_top || y > border_bottom {
                border_pixel
            } else {
                bg_pixel
            }
        });
        let mut shadow_image = block
            .shadow
            .as_ref()
            .map(|s| (s, image::RgbaImage::new(width, height)));

        if let Some(border) = block.border.as_ref() {
            rect.left += border.width;
            rect.right -= border.width;
            rect.top += border.width;
            rect.bottom -= border.width;
        }

        for glyph in glyphs {
            // println!("{:?}", glyph);
            let run = &block.text[glyph.section_index];
            let color = Pixel::try_from(run.color.as_ref().unwrap_or(&block.color))?;
            let glyph_font = &options.fonts.as_slice()[glyph.font_id];
            if let Some(g) = glyph_font.outline_glyph(glyph.glyph) {
                // println!("{:?}", g.px_bounds());
                let r = g.px_bounds();
                let x_base = r.min.x as u32;
                let y_base = r.min.y as u32;
                g.draw(|x, y, c| {
                    // println!("{x}, {y}, {c}", x = x, y = y, c = c);
                    let pixel = if c < 1.0 {
                        let mut p = color.clone();
                        p[3] = ((p[3] as f32) * c) as u8;
                        p
                    } else {
                        color
                    };
                    text_image.put_pixel(x_base + x, y_base + y, pixel);

                    if let Some((s, i)) = shadow_image.as_mut() {
                        let shadow_x = x_base + x + s.x;
                        let shadow_y = y_base + y + s.y;
                        if i.in_bounds(shadow_x, shadow_y) {
                            let pixel = if c < 1.0 {
                                let mut p = shadow_color.clone();
                                p[3] = ((p[3] as f32) * c) as u8;
                                p
                            } else {
                                color
                            };

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
