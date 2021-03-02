use anyhow::{anyhow, Context, Result};
use glyph_brush_layout::{
    ab_glyph::{Font, FontRef, PxScale},
    FontId, GlyphPositioner, Layout, LineBreaker, SectionGeometry, SectionGlyph, SectionText,
};
use image::{GenericImageView, ImageBuffer, Rgba};
use serde_derive::Deserialize;
use std::borrow::Cow;
use std::convert::TryFrom;

type Pixel = image::Rgba<u8>;

const fn pixel(red: u8, green: u8, blue: u8, alpha: u8) -> Pixel {
    Rgba([red, green, blue, alpha])
}

#[derive(Clone, Debug, Deserialize)]
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
pub struct FontDef<'a> {
    pub name: Cow<'a, str>,
    pub font: FontRef<'a>,
}

#[derive(Debug)]
pub struct OverlayOptions<'a> {
    pub background: image::DynamicImage,
    pub blocks: Vec<Block<'a>>,
    pub fonts: Vec<FontDef<'a>>,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
    pub shadow: Option<Shadow<'a>>,
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
    pub padding: Option<Rect>,
    // /// Wrap the text. Defaults to true
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

#[derive(Clone, Debug, Deserialize)]
pub struct Text<'a> {
    pub font: Cow<'a, str>,
    pub text: Cow<'a, str>,
    pub color: Option<Color<'a>>,
}

#[derive(Debug, Deserialize)]
pub struct Shadow<'a> {
    pub x: u32,
    pub y: u32,
    pub blur: Option<f32>,
    pub color: Option<Color<'a>>,
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

fn fit_glyphs<'a>(
    fonts: &[FontDef],
    rect: &Rect,
    options: &'a Block,
) -> Result<Vec<(Vec<Cow<'a, Text<'a>>>, Vec<SectionGlyph>)>> {
    println!("Rect {:?}", rect);
    let text_width = rect.right - rect.left;
    let text_height = rect.bottom - rect.top;

    let geometry = SectionGeometry {
        screen_position: (rect.left as f32, rect.top as f32),
        bounds: (text_width as f32, text_height as f32),
    };

    let (layout, lines, mut font_size) = if options.wrap {
        let layout = Layout::Wrap {
            line_breaker: glyph_brush_layout::BuiltInLineBreaker::UnicodeLineBreaker,
            h_align: options.h_align.into(),
            v_align: glyph_brush_layout::VerticalAlign::Top,
        };

        // Just a single line here and the layout algorithm will handle the wrapping.
        let text = options.text.iter().map(Cow::Borrowed).collect::<Vec<_>>();
        let lines = vec![text];

        (layout, lines, options.max_size)
    } else {
        let line_breaker = glyph_brush_layout::BuiltInLineBreaker::UnicodeLineBreaker;
        let layout = Layout::SingleLine {
            line_breaker,
            h_align: options.h_align.into(),
            v_align: glyph_brush_layout::VerticalAlign::Top,
        };

        // In non-wrapping mode we need to manually calculate how many lines can fit in the vertical
        // space.

        let mut lines = vec![];
        let mut current_line = Vec::new();
        for text in &options.text {
            let mut last_index = 0;
            println!("Text {}", text.text);
            for index in line_breaker.line_breaks(&text.text) {
                if let glyph_brush_layout::LineBreak::Hard(offset) = index {
                    println!("Break at offset {}", offset);
                    let t = text.text[last_index..offset].trim_matches('\n');

                    if !t.is_empty() {
                        current_line.push(Cow::Owned(Text {
                            text: Cow::from(t),
                            font: text.font.clone(),
                            color: text.color.clone(),
                        }));
                    }
                    lines.push(current_line);
                    current_line = Vec::new();
                    last_index = offset;
                }
            }

            if last_index == 0 {
                current_line.push(Cow::Borrowed(text));
            } else if last_index < text.text.len() {
                let t = text.text[last_index..].trim_matches('\n');
                if !t.is_empty() {
                    current_line.push(Cow::Owned(Text {
                        text: Cow::from(t),
                        font: text.font.clone(),
                        color: text.color.clone(),
                    }));
                }
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        let mut font_size = options.max_size;

        let lines_len_f32 = lines.len() as f32;
        let text_height_f32 = text_height as f32;
        // We assume that the first font in this block is representative of the height of all the fonts
        let sizing_font = fonts
            .iter()
            .find(|f| f.name == options.text[0].font)
            .ok_or_else(|| anyhow!("Could not find font named {}", options.text[0].font))?;
        while font_size >= options.min_size
            && pt_size_to_px_scale(&sizing_font.font, font_size, 1.0).y * lines_len_f32
                >= text_height_f32
        {
            font_size -= 4.0;
        }

        if font_size < options.min_size {
            return Err(anyhow!("Could not fit text in rectangle"));
        }

        (layout, lines, font_size)
    };

    let mut line_sections = lines
        .iter()
        .map(|line| {
            let sections = line
                .iter()
                .map(|t| {
                    Ok(SectionText {
                        text: &t.text,
                        font_id: fonts
                            .iter()
                            .position(|f| f.name == t.font)
                            .map(|index| FontId(index))
                            .ok_or_else(|| anyhow!("Could not find font named {}", t.font))?,
                        scale: PxScale::from(0.0), // This will be filled in below
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            Ok(sections)
        })
        .collect::<Result<Vec<_>>>()?;
    println!("Sections {:?}", line_sections);

    let font_refs = fonts.iter().map(|f| &f.font).collect::<Vec<_>>();
    for sections in line_sections.as_mut_slice().iter_mut() {
        if sections.is_empty() {
            // This happens with a pair of newlines. We keep the empty
            // section so that line position calculations work right, but there's
            // nothing to do here for that case.
            continue;
        }

        let text_length = sections
            .iter()
            .fold(0, |acc, section| acc + section.text.len());
        let last_section_byte_index = sections.last().unwrap().text.len() - 1;
        while font_size >= options.min_size {
            // println!("Trying font size {font_size}", font_size = font_size);
            for i in sections.iter_mut() {
                i.scale = pt_size_to_px_scale(&font_refs.as_slice()[i.font_id], font_size, 1.0);
            }

            let glyphs = layout.calculate_glyphs(font_refs.as_slice(), &geometry, &sections);

            let fits = if options.wrap {
                // When wrapping, the text fits if it doesn't exceed the vertical size available.
                // calculate_glyphs handles fitting the text horizontally.
                let last_glyph = glyphs.last().unwrap();
                println!(
                    "size {}, {} sections, {:?}",
                    font_size,
                    sections.len(),
                    last_glyph
                );
                let text_bottom = last_glyph.glyph.position.y;
                last_glyph.section_index == sections.len() - 1
                    && last_glyph.byte_index == last_section_byte_index
                    && text_bottom < rect.bottom as f32
            } else {
                // In non-wrapping mode, a line fits if we can render all of its glyphs.
                println!(
                    "size {} rendered {} glyphs out of {}",
                    font_size,
                    glyphs.len(),
                    text_length
                );
                glyphs.len() == text_length
            };

            if fits {
                println!("Chose font size {}", font_size);
                break;
            } else {
                font_size -= 4.0;
            }
        }
    }

    if font_size < options.min_size {
        return Err(anyhow!("Could not fit text in rectangle"));
    }

    // Go back through and render all the lines with the chosen font size.
    let sizing_font_id = line_sections[0][0].font_id;
    let sizing_font = &font_refs.as_slice()[sizing_font_id];
    let line_height = pt_size_to_px_scale(&sizing_font, font_size, 1.0);
    let result_glyphs = line_sections
        .into_iter()
        .enumerate()
        .map(|(line_index, mut sections)| {
            for i in sections.iter_mut() {
                i.scale = line_height;
            }

            let mut glyphs = layout.calculate_glyphs(font_refs.as_slice(), &geometry, &sections);
            let baseline = line_index as f32 * line_height.y;
            for glyph in glyphs.iter_mut() {
                glyph.glyph.position.y += baseline;
            }

            glyphs
        })
        .collect::<Vec<_>>();

    // And return each line's glyphs with the line that configured it.
    let result = lines
        .into_iter()
        .zip(result_glyphs.into_iter())
        .collect::<Vec<_>>();

    Ok(result)
}

fn blend(dest: Pixel, src: Pixel, src_alpha: f32) -> Pixel {
    if src_alpha >= 1.0 {
        return src;
    }

    pixel(
        ((dest[0] as f32) * (1.0 - src_alpha) + (src[0] as f32) * src_alpha) as u8,
        ((dest[1] as f32) * (1.0 - src_alpha) + (src[1] as f32) * src_alpha) as u8,
        ((dest[2] as f32) * (1.0 - src_alpha) + (src[2] as f32) * src_alpha) as u8,
        ((dest[3] as f32) * (1.0 - src_alpha) + (src_alpha * 255.0)) as u8,
    )
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

    if hex.len() == 6 || hex.len() == 8 {
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

    let font_refs = options.fonts.iter().map(|f| &f.font).collect::<Vec<_>>();
    const DEFAULT_SHADOW_COLOR: Pixel = pixel(0, 0, 0, 25);
    const TRANSPARENT: Pixel = pixel(0, 0, 0, 0);

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

        let shadow_color = block
            .shadow
            .as_ref()
            .and_then(|s| s.color.as_ref())
            .map(|s| Pixel::try_from(s))
            .transpose()?
            .unwrap_or(DEFAULT_SHADOW_COLOR);

        let border_pixel = block
            .border
            .as_ref()
            .map(|b| Pixel::try_from(&b.color))
            .transpose()?
            .unwrap_or_else(|| pixel(0, 0, 0, 255));
        let border_width = block.border.as_ref().map(|b| b.width).unwrap_or(0);

        if let Some(s) = block.border.as_ref().and_then(|b| b.shadow.as_ref()) {
            let shadow_bg_pixel = s
                .color
                .as_ref()
                .map(Pixel::try_from)
                .transpose()?
                .unwrap_or(DEFAULT_SHADOW_COLOR);
            let shadow_top = rect.top + s.y;
            let shadow_bottom = rect.bottom + s.y;
            let shadow_left = rect.left + s.x;
            let shadow_right = rect.right + s.x;

            let shadow_bg_image = image::RgbaImage::from_fn(width, height, |x, y| {
                if y >= shadow_top && y <= shadow_bottom && x >= shadow_left && x <= shadow_right {
                    shadow_bg_pixel
                } else {
                    TRANSPARENT
                }
            });

            let mut shadow_bg_image = s
                .blur
                .map(|blur_sigma| image::imageops::blur(&shadow_bg_image, blur_sigma))
                .unwrap_or(shadow_bg_image);

            // The shadow should not show through if the block is transparent, so clear out all the pixels for the
            // block's rect.
            let bg_placeholder = image::RgbaImage::from_pixel(
                rect.right - rect.left + 1,
                rect.bottom - rect.top + 1,
                TRANSPARENT,
            );
            image::imageops::replace(&mut shadow_bg_image, &bg_placeholder, rect.left, rect.top);

            image::imageops::overlay(&mut bg, &shadow_bg_image, 0, 0);
        }

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
                TRANSPARENT
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

        if let Some(padding) = block.padding.as_ref() {
            rect.left += padding.left;
            rect.right -= padding.right;
            rect.top += padding.top;
            rect.bottom -= padding.bottom;
        }

        let lines = fit_glyphs(&options.fonts, &rect, block)?;
        if lines.is_empty() {
            continue;
        }

        let lines_bottom = lines
            .last()
            .unwrap()
            .1
            .last()
            .map(|g| g.glyph.position.y)
            .unwrap_or(rect.bottom as f32);
        let start_y = match block.v_align {
            VAlign::Top => 0,
            VAlign::Center => {
                let first_glyph = &lines[0].1[0];
                let rect_height = rect.bottom - rect.top;
                let lines_top = first_glyph.glyph.position.y - first_glyph.glyph.scale.y;
                (rect_height / 2) - (((lines_bottom - lines_top - 1.0) / 2.0) as u32)
            }
            VAlign::Bottom => rect.bottom - (lines_bottom as u32),
        };
        println!("start_y: {}", start_y);

        for (texts, glyphs) in lines {
            for glyph in glyphs {
                // println!("{:?}", glyph);
                let run = &texts[glyph.section_index];
                let color = Pixel::try_from(run.color.as_ref().unwrap_or(&block.color))?;
                let glyph_font = &font_refs.as_slice()[glyph.font_id];
                if let Some(g) = glyph_font.outline_glyph(glyph.glyph) {
                    // println!("{:?}", g.px_bounds());
                    let r = g.px_bounds();
                    let x_base = r.min.x as u32;
                    let y_base = start_y + r.min.y as u32;
                    g.draw(|x, y, c| {
                        // println!("{x}, {y}, {c}", x = x, y = y, c = c);
                        let pixel = if c < 1.0 {
                            let mut p = color.clone();
                            p[3] = ((p[3] as f32) * c) as u8;
                            blend(bg_pixel, p, c)
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
