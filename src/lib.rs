use anyhow::{anyhow, Result};
use glyph_brush_layout::{
    ab_glyph::{Font, FontRef, PxScale},
    FontId, GlyphPositioner, Layout, SectionGeometry, SectionGlyph, SectionText,
};
use image::{ImageBuffer, Rgba};
use serde_derive::Deserialize;

#[derive(Debug)]
pub struct OverlayOptions<'a> {
    pub text: &'a str,
    pub background: image::DynamicImage,
    pub font: &'a FontRef<'a>,
    pub text_rect: &'a Rect,
    pub min_size: f32,
    pub max_size: f32,
    pub color: &'a str,
}

#[derive(Debug, Deserialize)]
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

fn fit_glyphs(options: &OverlayOptions) -> Result<Vec<SectionGlyph>> {
    let text_width = options.text_rect.right - options.text_rect.left;
    let text_height = options.text_rect.bottom - options.text_rect.top;

    let geometry = SectionGeometry {
        screen_position: (options.text_rect.left, options.text_rect.top),
        bounds: (text_width, text_height),
    };

    let layout = Layout::Wrap {
        line_breaker: glyph_brush_layout::BuiltInLineBreaker::UnicodeLineBreaker,
        h_align: glyph_brush_layout::HorizontalAlign::Left,
        v_align: glyph_brush_layout::VerticalAlign::Top,
    };

    let mut font_size = options.max_size;

    while font_size > options.min_size {
        // println!("Trying font size {font_size}", font_size = font_size);
        let section_text = SectionText {
            text: options.text,
            font_id: FontId(0),
            scale: pt_size_to_px_scale(options.font, font_size, 1.0),
        };

        let glyphs = layout.calculate_glyphs(&[options.font], &geometry, &[section_text]);

        let last_glyph = glyphs.last().unwrap();
        println!("size {}, {:?}", font_size, last_glyph);
        let text_bottom = last_glyph.glyph.position.y + last_glyph.glyph.scale.y;
        if text_bottom > options.text_rect.bottom {
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

// TODO Proper library errors instead of anyhow
pub fn overlay_text(options: &OverlayOptions) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let mut bg = options.background.to_rgba8();
    let (width, height) = bg.dimensions();
    let width = width as f32;
    let height = height as f32;
    if options.text_rect.left > width
        || options.text_rect.right > width
        || options.text_rect.top > height
        || options.text_rect.bottom > height
    {
        return Err(anyhow!(
            "Text rect {rect:?} does not fit in image of size {width}x{height}",
            rect = options.text_rect,
            width = width,
            height = height
        ));
    } else if options.text_rect.left >= options.text_rect.right
        || options.text_rect.top > options.text_rect.bottom
    {
        return Err(anyhow!("text_rect must not have a negative size"));
    }

    let glyphs = fit_glyphs(options)?;

    // TODO Use the color
    let red: u8 = 200;
    let green: u8 = 100;
    let blue: u8 = 100;

    for glyph in glyphs {
        println!("{:?}", glyph);
        if let Some(g) = options.font.outline_glyph(glyph.glyph) {
            println!("{:?}", g.px_bounds());
            let r = g.px_bounds();
            g.draw(|x, y, c| {
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

    Ok(bg)
}
