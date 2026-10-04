//! Reading a site's 2FA QR code from a picture: the clipboard (a snip with
//! Win+Shift+S) or an image file. Windows' own decoders (WIC) read the files,
//! so any format Windows shows works; the search itself is `myle_vault::qr`.

use std::path::Path;

pub use myle_vault::qr::{Grey, grey_from_dib, otpauth_in};
use myle_vault::qr::MAX_PIXELS;

/// An image file, through Windows' decoders.
pub fn grey_from_file(path: &Path) -> Result<Grey, String> {
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Graphics::Imaging::WICDecodeMetadataCacheOnDemand;
    use windows::core::HSTRING;
    decode(|factory| unsafe {
        factory.CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
    })
}

/// A picture in memory (a screenshot of a browser tab), the same way.
pub fn grey_from_bytes(bytes: &[u8]) -> Result<Grey, String> {
    use windows::Win32::Graphics::Imaging::WICDecodeMetadataCacheOnDemand;
    decode(|factory| unsafe {
        let stream = factory.CreateStream()?;
        // `bytes` outlives the stream: both end with this call.
        stream.InitializeFromMemory(bytes)?;
        factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
    })
}

fn decode(
    open: impl FnOnce(
        &windows::Win32::Graphics::Imaging::IWICImagingFactory,
    ) -> windows::core::Result<windows::Win32::Graphics::Imaging::IWICBitmapDecoder>,
) -> Result<Grey, String> {
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat8bppGray, IWICImagingFactory, WICBitmapDitherTypeNone,
        WICBitmapPaletteTypeCustom,
    };
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};

    let unreadable = |_| "That picture could not be read. Use a PNG, JPEG or BMP file.".to_string();
    unsafe {
        // This thread may already have COM set up another way: fine either way.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let decoder = open(&factory).map_err(unreadable)?;
        let frame = decoder.GetFrame(0).map_err(unreadable)?;
        let grey = factory.CreateFormatConverter().map_err(|e| e.to_string())?;
        grey.Initialize(&frame, &GUID_WICPixelFormat8bppGray, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)
            .map_err(unreadable)?;
        let (mut width, mut height) = (0u32, 0u32);
        grey.GetSize(&mut width, &mut height).map_err(unreadable)?;
        let (width, height) = (width as usize, height as usize);
        if width == 0 || height == 0 || width.saturating_mul(height) > MAX_PIXELS {
            return Err("That picture is too large to search for a QR code.".into());
        }
        let mut pixels = vec![0u8; width * height];
        grey.CopyPixels(std::ptr::null(), width as u32, &mut pixels).map_err(unreadable)?;
        Ok(Grey { width, height, pixels })
    }
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
    fn an_image_file_is_read_through_windows() {
        let qr = qr_picture(LINK, 4);
        let path = std::env::temp_dir().join(format!("myle-qr-{}.png", std::process::id()));
        {
            let file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
            let mut encoder = png::Encoder::new(file, qr.width as u32, qr.height as u32);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.write_header().unwrap().write_image_data(&qr.pixels).unwrap();
        }
        let grey = grey_from_file(&path).unwrap();
        assert_eq!(otpauth_in(&grey).unwrap(), LINK);
        // The same picture from memory, as a tab's screenshot comes.
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(otpauth_in(&grey_from_bytes(&bytes).unwrap()).unwrap(), LINK);
        assert!(grey_from_bytes(b"not a picture").is_err());
        let _ = std::fs::remove_file(&path);
        assert!(grey_from_file(&path).is_err());
    }
}
