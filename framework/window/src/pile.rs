use std::fmt::Write as _;
use std::io::Cursor;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gpui::{DragPicture, Pixels, RenderImage, Rgba, SvgRenderer, point};
use image::{ImageFormat, RgbaImage};
use nova_files_api::{IconFormat, ThemeIcon};

use crate::theme::{Colors, pile, radius, size, text};
use crate::theme_icons::RENDERER_SCALE;
use nova_files_shell::assets::Icon;

const BYTES_PER_PIXEL: usize = 4;
const HALF: f32 = 0.5;
const BOTH_SIDES: f32 = 2.0;
const RED: usize = 0;
const BLUE: usize = 2;
const CHANNEL_MAX: f32 = 255.0;

pub enum Face {
    Thumbnail(Arc<RenderImage>),
    Theme(Arc<ThemeIcon>),
    Glyph { glyph: Icon, tint: Rgba },
}

pub enum Label {
    Count(String),
    Name(String),
}

pub struct Pile {
    pub faces: Vec<Face>,
    pub label: Label,
    pub label_width: Pixels,
    pub colors: Colors,
}

struct Layout {
    width: f32,
    height: f32,
    front_x: f32,
    front_y: f32,
    label_x: f32,
    label_y: f32,
    label_width: f32,
}

#[derive(Clone)]
pub struct PileDrawing {
    pub pile: Arc<Pile>,
    pub renderer: SvgRenderer,
    pub scale: u32,
}

impl std::fmt::Debug for PileDrawing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PileDrawing")
            .field("cards", &self.pile.faces.len())
            .field("scale", &self.scale)
            .finish_non_exhaustive()
    }
}

impl PileDrawing {
    pub fn draw(&self) -> Option<DragPicture> {
        self.pile.draw(&self.renderer, self.scale)
    }
}

impl Pile {
    pub fn draw(&self, renderer: &SvgRenderer, scale: u32) -> Option<DragPicture> {
        let svg = self.svg();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window's scale is a single-digit whole number"
        )]
        let wanted = scale as f32 / RENDERER_SCALE;
        let image = renderer
            .render_single_frame(svg.as_bytes(), wanted)
            .inspect_err(|error| log::warn!("the drag pile didn't draw: {error}"))
            .ok()?;
        let offset = f32::from(pile::FROM_POINTER);
        Some(DragPicture {
            image,
            scale,
            offset: point(offset.into(), offset.into()),
        })
    }

    fn layout(&self) -> Layout {
        let margin = f32::from(pile::MARGIN);
        let card = f32::from(pile::CARD);
        let behind = self.faces.len().saturating_sub(1).min(pile::MOST - 1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "at most a few cards sit behind the front one"
        )]
        let behind = behind as f32;
        let front_x = margin;
        let front_y = behind.mul_add(f32::from(pile::STEP_Y), margin);
        let pile_right = behind.mul_add(f32::from(pile::STEP_X), front_x + card);
        let chip = f32::from(pile::CHIP_HEIGHT);
        let label_width =
            BOTH_SIDES.mul_add(f32::from(size::DRAG_CHIP_PAD), f32::from(self.label_width));
        let (label_x, label_y) = match self.label {
            Label::Count(_) => (
                front_x + card + f32::from(pile::CHIP_OVERHANG) - label_width,
                f32::from(pile::CHIP_OVERHANG).mul_add(HALF, front_y + card - chip),
            ),
            Label::Name(_) => (
                (card - label_width).mul_add(HALF, front_x),
                front_y + card + f32::from(pile::NAME_GAP),
            ),
        };
        let left_shift = (-label_x + margin).max(0.0);
        Layout {
            width: pile_right.max(label_x + label_width) + margin + left_shift,
            height: (front_y + card).max(label_y + chip) + margin,
            front_x: front_x + left_shift,
            front_y,
            label_x: label_x + left_shift,
            label_y,
            label_width,
        }
    }

    pub fn svg(&self) -> String {
        let layout = self.layout();
        let colors = &self.colors;
        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><defs><filter id="shadow" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="0" dy="{dy}" stdDeviation="{blur}" flood-color="{shade}" flood-opacity="{strength}"/></filter></defs>"#,
            w = layout.width,
            h = layout.height,
            dy = f32::from(pile::SHADOW_Y),
            blur = f32::from(pile::SHADOW_BLUR) * HALF,
            shade = hex(colors.shadow),
            strength = colors.shadow.a,
        );
        let shown = self.faces.iter().take(pile::MOST).enumerate().rev();
        for (depth, face) in shown {
            self.card(&mut svg, &layout, depth, face);
        }
        self.label(&mut svg, &layout);
        svg.push_str("</svg>");
        svg
    }

    fn card(&self, svg: &mut String, layout: &Layout, depth: usize, face: &Face) {
        let colors = &self.colors;
        let card = f32::from(pile::CARD);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a card's depth is below the pile's few cards"
        )]
        let depth = depth as f32;
        let x = depth.mul_add(f32::from(pile::STEP_X), layout.front_x);
        let y = (-depth).mul_add(f32::from(pile::STEP_Y), layout.front_y);
        let tilt = depth * pile::TILT_DEGREES;
        let centre_x = card.mul_add(HALF, x);
        let centre_y = card.mul_add(HALF, y);
        let corner = f32::from(radius::SMALL);
        let clip = format!("card{depth}");
        write!(
            svg,
            r#"<g opacity="{see}" transform="rotate({tilt} {centre_x} {centre_y})"><clipPath id="{clip}"><rect x="{x}" y="{y}" width="{card}" height="{card}" rx="{corner}"/></clipPath><rect x="{x}" y="{y}" width="{card}" height="{card}" rx="{corner}" fill="{fill}" filter="url(#shadow)"/>"#,
            see = pile::SEE_THROUGH,
            fill = hex(colors.lifted),
        )
        .ok();
        let pad = f32::from(pile::ICON_PAD);
        let inner = (-BOTH_SIDES).mul_add(pad, card);
        match face {
            Face::Thumbnail(image) => {
                if let Some(data) = png_data(image) {
                    write!(
                        svg,
                        r#"<image clip-path="url(#{clip})" x="{x}" y="{y}" width="{card}" height="{card}" preserveAspectRatio="xMidYMid slice" xlink:href="{data}"/>"#,
                    )
                    .ok();
                }
            }
            Face::Theme(icon) => {
                write!(
                    svg,
                    r#"<image x="{ix}" y="{iy}" width="{inner}" height="{inner}" xlink:href="{data}"/>"#,
                    ix = x + pad,
                    iy = y + pad,
                    data = theme_data(icon),
                )
                .ok();
            }
            Face::Glyph { glyph, tint } => {
                svg.push_str(&glyph_svg(*glyph, *tint, x + pad, y + pad, inner));
            }
        }
        let edge = f32::from(pile::EDGE);
        let ring = match face {
            Face::Thumbnail(_) => colors.ring,
            Face::Theme(_) | Face::Glyph { .. } => colors.line,
        };
        write!(
            svg,
            r#"<rect x="{x}" y="{y}" width="{card}" height="{card}" rx="{corner}" fill="none" stroke="{stroke}" stroke-opacity="{opacity}" stroke-width="{edge}"/></g>"#,
            stroke = hex(ring),
            opacity = ring.a,
        )
        .ok();
    }

    fn label(&self, svg: &mut String, layout: &Layout) {
        let colors = &self.colors;
        let words = match &self.label {
            Label::Count(words) | Label::Name(words) => words,
        };
        let chip = f32::from(pile::CHIP_HEIGHT);
        write!(
            svg,
            r#"<rect x="{x}" y="{y}" width="{w}" height="{chip}" rx="{corner}" fill="{fill}" stroke="{line}" stroke-opacity="{line_opacity}" stroke-width="{edge}" filter="url(#shadow)"/><text x="{tx}" y="{ty}" text-anchor="middle" dominant-baseline="central" textLength="{fit}" lengthAdjust="spacingAndGlyphs" font-family="{family}" font-size="{font}" font-weight="{weight}" fill="{ink}">{words}</text>"#,
            x = layout.label_x,
            y = layout.label_y,
            w = layout.label_width,
            corner = f32::from(radius::CONTROL),
            fill = hex(colors.lifted),
            line = hex(colors.line),
            line_opacity = colors.line.a,
            edge = f32::from(size::LINE),
            fit = f32::from(self.label_width),
            tx = layout.label_width.mul_add(HALF, layout.label_x),
            ty = chip.mul_add(HALF, layout.label_y),
            family = text::FAMILY,
            font = f32::from(text::BODY),
            weight = text::BODY_WEIGHT.0,
            ink = hex(colors.text),
            words = escaped(words),
        )
        .ok();
    }
}

fn glyph_svg(glyph: Icon, tint: Rgba, x: f32, y: f32, side: f32) -> String {
    let source = String::from_utf8_lossy(glyph.bytes());
    let placed = format!(r#"<svg x="{x}" y="{y}" width="{side}" height="{side}" "#);
    let tinted = format!(r#"fill="{}" fill-opacity="{}""#, hex(tint), tint.a);
    source
        .replacen("<svg ", &placed, 1)
        .replacen(r##"fill="#000""##, &tinted, 1)
}

fn hex(color: Rgba) -> String {
    let channel = |value: f32| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a colour channel is clamped to 0..=255 first"
        )]
        let byte = (value.clamp(0.0, 1.0) * CHANNEL_MAX).round() as u8;
        byte
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

fn escaped(words: &str) -> String {
    words
        .chars()
        .map(|letter| match letter {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            letter if letter.is_control() => char::REPLACEMENT_CHARACTER.to_string(),
            letter => letter.to_string(),
        })
        .collect()
}

fn theme_data(icon: &ThemeIcon) -> String {
    let mime = match icon.format {
        IconFormat::Png => "image/png",
        IconFormat::Svg => "image/svg+xml",
    };
    format!("data:{mime};base64,{}", STANDARD.encode(&icon.bytes))
}

fn png_data(image: &RenderImage) -> Option<String> {
    let size = image.size(0);
    let width = u32::try_from(size.width.0).ok()?;
    let height = u32::try_from(size.height.0).ok()?;
    let mut rgba = image.as_bytes(0)?.to_vec();
    for pixel in rgba.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
        pixel.swap(RED, BLUE);
    }
    let picture = RgbaImage::from_raw(width, height, rgba)?;
    let mut png = Vec::new();
    picture
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

#[cfg(test)]
mod tests;
