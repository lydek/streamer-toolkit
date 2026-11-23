//! 圖片尺寸: one picture resized to the sizes Twitch asks for (subscriber badges 72/36/18 px, channel points icons
//! 112/56/28 px), squared by padding with transparency or cropping the middle, as optimized PNGs. A badge above
//! 25 KB is reduced to 256 colors; one still above that is flagged. Everything stays on this computer. Admin route;
//! the caller adds the token check.

use std::io::Cursor;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Query, rejection::BytesRejection},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use base64::Engine;
use image::{
    DynamicImage, ImageFormat, ImageReader, Limits, Rgba, RgbaImage,
    imageops::{self, FilterType},
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::admin::{ErrorBody, error};

/// The largest picture accepted, in bytes and in pixels per side.
pub(crate) const MAX_BYTES: usize = 10 * 1024 * 1024;
const MAX_SIDE: u32 = 4096;
/// Twitch's limit for a badge file.
pub(crate) const BADGE_MAX_BYTES: usize = 25 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageKind {
    /// Subscriber badge: 72, 36 and 18 px.
    Badge,
    /// Channel points icon: 112, 56 and 28 px.
    Points,
}

impl ImageKind {
    fn sizes(self) -> [u32; 3] {
        match self {
            Self::Badge => [72, 36, 18],
            Self::Points => [112, 56, 28],
        }
    }
}

/// How a picture that is not square becomes square.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Fit {
    /// Transparent edges.
    Pad,
    /// The middle.
    Crop,
    /// The rectangle the broadcaster picked (`crop_x`, `crop_y`, `crop_w`, `crop_h`), padded to a square.
    Manual,
}

/// A rectangle of the picture, in its own pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Crop {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct SizeQuery {
    kind: ImageKind,
    fit: Fit,
    /// `zip`: every size in one zip file; otherwise JSON with a preview of each.
    #[serde(default)]
    zip: bool,
    /// With `fit=manual`: the rectangle's left edge, top edge, width and height in the picture's pixels; it must lie
    /// inside the picture.
    crop_x: Option<u32>,
    crop_y: Option<u32>,
    crop_w: Option<u32>,
    crop_h: Option<u32>,
    /// The padding's color as six hex digits (`1f2937`); transparent when missing.
    bg: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct SizedImage {
    pub(crate) px: u32,
    pub(crate) bytes: usize,
    /// A `data:image/png;base64,…` URL to preview it.
    pub(crate) data_url: String,
    /// A badge still above 25 KB after reducing its colors.
    pub(crate) too_large: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct SizedImages {
    pub(crate) images: Vec<SizedImage>,
}

pub(crate) fn routes() -> Router {
    Router::new().route(
        "/api/admin/image-sizes",
        post(make_sizes).layer(DefaultBodyLimit::max(MAX_BYTES)),
    )
}

/// Decodes PNG, JPEG, GIF or WebP within the size and pixel caps.
fn decode(bytes: &[u8]) -> Result<DynamicImage, &'static str> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "image_sizes.unsupported")?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP)
    ) {
        return Err("image_sizes.unsupported");
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    reader.limits(limits);
    reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => "image_sizes.too_large",
        _ => "image_sizes.unsupported",
    })
}

/// `rrggbb` as an opaque color.
fn parse_bg(hex: &str) -> Option<Rgba<u8>> {
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Rgba([byte(0)?, byte(2)?, byte(4)?, 255]))
}

/// The picture in the middle of a square whose edges are `bg`, or transparent.
fn padded(rgba: &RgbaImage, bg: Option<Rgba<u8>>) -> RgbaImage {
    let (w, h) = rgba.dimensions();
    let side = w.max(h);
    let mut canvas = RgbaImage::from_pixel(side, side, bg.unwrap_or(Rgba([0, 0, 0, 0])));
    imageops::overlay(
        &mut canvas,
        rgba,
        i64::from((side - w) / 2),
        i64::from((side - h) / 2),
    );
    canvas
}

/// Square, by transparent edges, by the middle, or by the rectangle picked (which must lie inside the picture), padded.
fn square(
    img: &DynamicImage,
    fit: Fit,
    crop: Option<Crop>,
    bg: Option<Rgba<u8>>,
) -> Result<RgbaImage, &'static str> {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(match fit {
        Fit::Pad => padded(&rgba, bg),
        Fit::Crop => {
            let side = w.min(h);
            imageops::crop_imm(&rgba, (w - side) / 2, (h - side) / 2, side, side).to_image()
        }
        Fit::Manual => {
            let c = crop.ok_or("image_sizes.invalid_crop")?;
            let inside = c.w > 0
                && c.h > 0
                && c.x.checked_add(c.w).is_some_and(|r| r <= w)
                && c.y.checked_add(c.h).is_some_and(|b| b <= h);
            if !inside {
                return Err("image_sizes.invalid_crop");
            }
            padded(
                &imageops::crop_imm(&rgba, c.x, c.y, c.w, c.h).to_image(),
                bg,
            )
        }
    })
}

fn png(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .expect("PNG encodes in memory");
    // Lossless; keeps the plain PNG if the optimizer has nothing better.
    oxipng::optimize_from_memory(&out, &oxipng::Options::from_preset(2)).unwrap_or(out)
}

/// The same picture in at most 256 colors (alpha included), which the optimizer stores as a palette.
fn reduced(img: &RgbaImage) -> RgbaImage {
    let quant = color_quant::NeuQuant::new(10, 256, img.as_raw());
    let palette = quant.color_map_rgba();
    let mut out = img.clone();
    for px in out.pixels_mut() {
        let i = quant.index_of(&px.0) * 4;
        px.0.copy_from_slice(&palette[i..i + 4]);
    }
    out
}

/// One size: its pixels a side, the PNG file and whether a badge is still above its limit.
type Sized = (u32, Vec<u8>, bool);

/// Every size as PNG bytes, with whether a badge is still above its limit; padding is transparent.
#[cfg(test)]
fn sized(
    bytes: &[u8],
    kind: ImageKind,
    fit: Fit,
    crop: Option<Crop>,
) -> Result<Vec<Sized>, &'static str> {
    sized_on(bytes, kind, fit, crop, None)
}

/// [`sized`] with the padding in `bg`.
fn sized_on(
    bytes: &[u8],
    kind: ImageKind,
    fit: Fit,
    crop: Option<Crop>,
    bg: Option<Rgba<u8>>,
) -> Result<Vec<Sized>, &'static str> {
    sized_within(bytes, kind, fit, crop, bg, BADGE_MAX_BYTES)
}

/// [`sized_on`] with the badge limit given (a 72 px badge rarely reaches 25 KB, so tests use a smaller one).
fn sized_within(
    bytes: &[u8],
    kind: ImageKind,
    fit: Fit,
    crop: Option<Crop>,
    bg: Option<Rgba<u8>>,
    limit: usize,
) -> Result<Vec<Sized>, &'static str> {
    let source = square(&decode(bytes)?, fit, crop, bg)?;
    Ok(kind
        .sizes()
        .into_iter()
        .map(|px| {
            let img = imageops::resize(&source, px, px, FilterType::Lanczos3);
            let mut file = png(&img);
            if kind == ImageKind::Badge && file.len() > limit {
                // Fewer colors usually packs smaller, but not always: keep whichever is.
                let fewer = png(&reduced(&img));
                if fewer.len() < file.len() {
                    file = fewer;
                }
            }
            let over = kind == ImageKind::Badge && file.len() > limit;
            (px, file, over)
        })
        .collect())
}

fn zip_of(kind: ImageKind, files: &[Sized]) -> Vec<u8> {
    use std::io::Write;
    let name = match kind {
        ImageKind::Badge => "badge",
        ImageKind::Points => "points",
    };
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // PNGs are already compressed.
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (px, file, _) in files {
        out.start_file(format!("{name}-{px}.png"), options)
            .expect("zip in memory");
        out.write_all(file).expect("zip in memory");
    }
    out.finish().expect("zip in memory").into_inner()
}

/// Resizes a PNG, JPEG, GIF or WebP picture (at most 10 MB and 4096 px a side; the body is the file itself) to the
/// sizes of `kind`, as JSON with previews or, with `zip=true`, as one zip of PNG files.
#[utoipa::path(
    post,
    path = "/api/admin/image-sizes",
    tag = "admin",
    params(SizeQuery),
    request_body(content = String, content_type = "application/octet-stream", description = "The picture"),
    responses(
        (status = 200, body = SizedImages, description = "Or, with `zip=true`, an `application/zip` file"),
        (status = 400, description = "`image_sizes.unsupported`, or `image_sizes.invalid_crop`: a manual rectangle missing or outside the picture, or `image_sizes.invalid_bg`: a padding color that is not six hex digits", body = ErrorBody),
        (status = 413, description = "`image_sizes.too_large`: more than 10 MB or 4096 px a side", body = ErrorBody),
    )
)]
pub(crate) async fn make_sizes(
    Query(q): Query<SizeQuery>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let bytes = match body {
        Ok(b) => b,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return error(StatusCode::PAYLOAD_TOO_LARGE, "image_sizes.too_large");
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "image_sizes.unsupported"),
    };
    let crop = match (q.crop_x, q.crop_y, q.crop_w, q.crop_h) {
        (Some(x), Some(y), Some(w), Some(h)) => Some(Crop { x, y, w, h }),
        _ => None,
    };
    let bg = match q.bg.as_deref() {
        None => None,
        Some(hex) => match parse_bg(hex) {
            Some(color) => Some(color),
            None => return error(StatusCode::BAD_REQUEST, "image_sizes.invalid_bg"),
        },
    };
    let result =
        tokio::task::spawn_blocking(move || sized_on(&bytes, q.kind, q.fit, crop, bg)).await;
    let files = match result {
        Ok(Ok(files)) => files,
        Ok(Err(code @ "image_sizes.too_large")) => {
            return error(StatusCode::PAYLOAD_TOO_LARGE, code);
        }
        Ok(Err(code)) => return error(StatusCode::BAD_REQUEST, code),
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "image_sizes.failed"),
    };
    if q.zip {
        return (
            [(header::CONTENT_TYPE, "application/zip")],
            zip_of(q.kind, &files),
        )
            .into_response();
    }
    let b64 = base64::engine::general_purpose::STANDARD;
    Json(SizedImages {
        images: files
            .into_iter()
            .map(|(px, file, too_large)| SizedImage {
                px,
                bytes: file.len(),
                data_url: format!("data:image/png;base64,{}", b64.encode(&file)),
                too_large,
            })
            .collect(),
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32, noisy: bool) -> Vec<u8> {
        // xorshift: pixels that do not compress, so a badge starts large.
        let mut seed = 0x9E37_79B9_u32;
        let img = RgbaImage::from_fn(w, h, |_, _| {
            if noisy {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let [r, g, b, _] = seed.to_le_bytes();
                image::Rgba([r, g, b, 255])
            } else {
                image::Rgba([200, 30, 30, 255])
            }
        });
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn side(png: &[u8]) -> (u32, u32) {
        image::load_from_memory(png)
            .unwrap()
            .to_rgba8()
            .dimensions()
    }

    #[test]
    fn every_size_comes_out_square() {
        for (kind, sizes) in [
            (ImageKind::Badge, [72, 36, 18]),
            (ImageKind::Points, [112, 56, 28]),
        ] {
            for fit in [Fit::Pad, Fit::Crop] {
                let out = sized(&picture(300, 120, false), kind, fit, None).unwrap();
                assert_eq!(out.iter().map(|o| o.0).collect::<Vec<_>>(), sizes);
                for (px, file, too_large) in out {
                    assert_eq!(side(&file), (px, px));
                    assert!(!too_large);
                }
            }
        }
    }

    #[test]
    fn a_picked_rectangle_is_cut_out_padded_and_must_lie_inside() {
        let pic = picture(300, 120, false);
        let crop = |x, y, w, h| Some(Crop { x, y, w, h });
        let out = sized(&pic, ImageKind::Points, Fit::Manual, crop(10, 20, 100, 100)).unwrap();
        assert!(out.iter().all(|(px, file, _)| side(file) == (*px, *px)));
        // A wide rectangle keeps its shape: transparent above and below it, opaque in the middle.
        let wide = sized(&pic, ImageKind::Points, Fit::Manual, crop(10, 20, 200, 50)).unwrap();
        assert!(wide.iter().all(|(px, file, _)| side(file) == (*px, *px)));
        let img = image::load_from_memory(&wide[0].1).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(56, 0)[3], 0, "padding above the rectangle");
        assert_eq!(img.get_pixel(56, 56)[3], 255);
        // A very narrow one still gives every size.
        let narrow = sized(
            &picture(20, 1000, false),
            ImageKind::Points,
            Fit::Manual,
            crop(0, 0, 20, 1000),
        )
        .unwrap();
        assert!(narrow.iter().all(|(px, file, _)| side(file) == (*px, *px)));
        for bad in [
            None,
            crop(250, 0, 100, 100),
            crop(0, 30, 100, 100),
            crop(0, 0, 0, 10),
            crop(0, 0, 10, 0),
            crop(u32::MAX, 0, 2, 2),
        ] {
            assert_eq!(
                sized(&pic, ImageKind::Points, Fit::Manual, bad).err(),
                Some("image_sizes.invalid_crop")
            );
        }
    }

    #[test]
    fn padding_takes_a_chosen_color_and_refuses_a_bad_one() {
        let pic = picture(300, 100, false);
        let red = parse_bg("ff0000");
        let out = sized_on(&pic, ImageKind::Points, Fit::Pad, None, red).unwrap();
        let img = image::load_from_memory(&out[0].1).unwrap().to_rgba8();
        assert_eq!(
            img.get_pixel(56, 0).0,
            [255, 0, 0, 255],
            "the top edge is red"
        );
        for bad in ["f00", "ff00zz", "#ff0000", ""] {
            assert_eq!(parse_bg(bad), None, "{bad}");
        }
    }

    #[test]
    fn padding_keeps_the_edges_transparent() {
        let out = sized(&picture(300, 100, false), ImageKind::Points, Fit::Pad, None).unwrap();
        let img = image::load_from_memory(&out[0].1).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(56, 0)[3], 0, "the top edge is padding");
        assert_eq!(img.get_pixel(56, 56)[3], 255);
    }

    #[test]
    fn a_heavy_badge_is_reduced_to_256_colors_and_flagged_only_when_still_over() {
        let noisy = picture(72, 72, true);
        let plain =
            sized_within(&noisy, ImageKind::Badge, Fit::Crop, None, None, usize::MAX).unwrap();
        let limit = plain[0].1.len() - 1;
        let out = sized_within(&noisy, ImageKind::Badge, Fit::Crop, None, None, limit).unwrap();
        assert!(
            out[0].1.len() < plain[0].1.len(),
            "reducing the colors made it smaller"
        );
        let colors: std::collections::HashSet<_> = image::load_from_memory(&out[0].1)
            .unwrap()
            .to_rgba8()
            .pixels()
            .map(|p| p.0)
            .collect();
        assert!(colors.len() <= 256);
        assert!(!out[0].2, "under the limit after reducing");
        let still = sized_within(&noisy, ImageKind::Badge, Fit::Crop, None, None, 10).unwrap();
        assert!(still[0].2, "flagged when even that is too big");
        let icon = sized_within(&noisy, ImageKind::Points, Fit::Crop, None, None, 10).unwrap();
        assert!(!icon[0].2, "only badges have the limit");
    }

    #[test]
    fn oversized_and_unknown_input_is_refused() {
        assert_eq!(
            sized(
                &picture(MAX_SIDE + 1, 2, false),
                ImageKind::Badge,
                Fit::Pad,
                None
            )
            .err(),
            Some("image_sizes.too_large")
        );
        assert_eq!(
            sized(
                b"<svg xmlns='http://www.w3.org/2000/svg'/>",
                ImageKind::Badge,
                Fit::Pad,
                None
            )
            .err(),
            Some("image_sizes.unsupported")
        );
    }

    #[test]
    fn the_zip_holds_one_png_per_size() {
        let files = sized(&picture(100, 100, false), ImageKind::Badge, Fit::Pad, None).unwrap();
        let zip = zip_of(ImageKind::Badge, &files);
        let archive = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
        let mut names: Vec<String> = archive
            .file_names()
            .map(|n| n.unwrap().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["badge-18.png", "badge-36.png", "badge-72.png"]);
    }
}
