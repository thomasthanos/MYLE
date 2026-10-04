//! Reading a site's 2FA QR code from a picture: as grey levels, from a
//! clipboard bitmap or whatever decoded the picture (Windows' WIC in the
//! Windows app).

/// A picture as grey levels, one byte per pixel, row by row.
pub struct Grey {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

/// Larger pictures are halved first: a QR code in them is big enough, and
/// the search is much quicker.
const MAX_SIDE: usize = 2400;
/// Larger than any screen's picture: refused instead of read.
pub const MAX_PIXELS: usize = 64 * 1024 * 1024;

/// The 2FA link in the picture's QR code.
pub fn otpauth_in(image: &Grey) -> Result<String, String> {
    let mut smaller: Option<Grey> = None;
    loop {
        let current = smaller.as_ref().unwrap_or(image);
        if current.width.max(current.height) <= MAX_SIDE {
            break;
        }
        smaller = Some(halve(current));
    }
    let image = smaller.as_ref().unwrap_or(image);
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(image.width, image.height, |x, y| {
        image.pixels[y * image.width + x]
    });
    let mut other = false;
    for grid in prepared.detect_grids() {
        if let Ok((_, content)) = grid.decode() {
            if content.trim_start().to_ascii_lowercase().starts_with("otpauth://") {
                return Ok(content.trim().to_string());
            }
            other = true;
        }
    }
    Err(if other {
        "That QR code is not for 2FA codes (it has no otpauth:// link).".into()
    } else {
        "No QR code was found in that picture. Snip it closer around the code and try again.".into()
    })
}

fn halve(image: &Grey) -> Grey {
    let (width, height) = (image.width / 2, image.height / 2);
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let at = |dx: usize, dy: usize| u32::from(image.pixels[(y * 2 + dy) * image.width + x * 2 + dx]);
            pixels.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1)) / 4) as u8);
        }
    }
    Grey { width, height, pixels }
}

/// A clipboard bitmap (`CF_DIB`: a BITMAPINFOHEADER, then the pixels), in
/// the 24- and 32-bit forms screenshots come in.
pub fn grey_from_dib(dib: &[u8]) -> Option<Grey> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(dib.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(dib.get(at..at + 4)?.try_into().ok()?));
    let header = u32_at(0)? as usize;
    let width = u32_at(4)? as i32;
    let height = u32_at(8)? as i32;
    let bits = usize::from(u16_at(14)?);
    let compression = u32_at(16)?;
    let colors = u32_at(32)? as usize;
    // BI_RGB, or BI_BITFIELDS with the usual masks.
    if !matches!(bits, 24 | 32) || !matches!(compression, 0 | 3) || header < 40 {
        return None;
    }
    let (w, h) = (width.unsigned_abs() as usize, height.unsigned_abs() as usize);
    if w == 0 || h == 0 || w.checked_mul(h)? > MAX_PIXELS {
        return None;
    }
    // A plain header is followed by the three masks of BI_BITFIELDS.
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let start = header + masks + colors * 4;
    let stride = (w * bits / 8 + 3) & !3;
    let data = dib.get(start..start.checked_add(stride.checked_mul(h)?)?)?;
    let step = bits / 8;
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        // Rows run bottom-up unless the height is negative.
        let row = if height > 0 { h - 1 - y } else { y };
        let line = &data[row * stride..row * stride + w * step];
        pixels.extend(line.chunks_exact(step).map(|p| luma(p[2], p[1], p[0])));
    }
    Some(Grey { width: w, height: h, pixels })
}

fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "otpauth://totp/MYLE:test@example.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=MYLE";

    /// A QR code of `text`, `scale` pixels a module, with a white border.
    fn qr_picture(text: &str, scale: usize) -> Grey {
        let code = qrcode::QrCode::new(text.as_bytes()).unwrap();
        let modules = code.width();
        let colors = code.to_colors();
        let side = (modules + 8) * scale;
        let mut pixels = vec![255u8; side * side];
        for y in 0..side {
            for x in 0..side {
                let (mx, my) = ((x / scale).wrapping_sub(4), (y / scale).wrapping_sub(4));
                if mx < modules && my < modules && colors[my * modules + mx] == qrcode::Color::Dark {
                    pixels[y * side + x] = 0;
                }
            }
        }
        Grey { width: side, height: side, pixels }
    }

    #[test]
    fn a_2fa_qr_code_is_read_and_another_is_named_as_such() {
        assert_eq!(otpauth_in(&qr_picture(LINK, 6)).unwrap(), LINK);
        assert!(otpauth_in(&qr_picture("https://example.com", 6)).unwrap_err().contains("not for 2FA"));
        let blank = Grey { width: 200, height: 200, pixels: vec![255; 40_000] };
        assert!(otpauth_in(&blank).unwrap_err().contains("No QR code"));
    }

    #[test]
    fn a_large_screenshot_is_searched_too() {
        let qr = qr_picture(LINK, 8);
        let (width, height) = (3840, 2160);
        let mut pixels = vec![230u8; width * height];
        for y in 0..qr.height {
            let at = (700 + y) * width + 2500;
            pixels[at..at + qr.width].copy_from_slice(&qr.pixels[y * qr.width..(y + 1) * qr.width]);
        }
        assert_eq!(otpauth_in(&Grey { width, height, pixels }).unwrap(), LINK);
    }

    /// What Win+Shift+S leaves on the clipboard: 32-bit, rows bottom-up.
    #[test]
    fn a_clipboard_bitmap_is_read() {
        let qr = qr_picture(LINK, 5);
        let mut dib = Vec::new();
        dib.extend(40u32.to_le_bytes());
        dib.extend((qr.width as i32).to_le_bytes());
        dib.extend((qr.height as i32).to_le_bytes());
        dib.extend(1u16.to_le_bytes());
        dib.extend(32u16.to_le_bytes());
        dib.extend(0u32.to_le_bytes());
        dib.extend([0u8; 20]);
        for y in (0..qr.height).rev() {
            for x in 0..qr.width {
                let v = qr.pixels[y * qr.width + x];
                dib.extend([v, v, v, 255]);
            }
        }
        let grey = grey_from_dib(&dib).unwrap();
        assert_eq!(grey.pixels, qr.pixels, "same picture, top row first");
        assert_eq!(otpauth_in(&grey).unwrap(), LINK);
        assert!(grey_from_dib(&dib[..30]).is_none());
    }
}
