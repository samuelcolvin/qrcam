use image::GrayImage;
use std::{
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use zxingcpp::{Barcode, BarcodeFormat, Position};

#[derive(Debug)]
pub struct QRCode {
    text: String,
    position: Position,
}

impl fmt::Display for QRCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}/{}",
            self.text, self.position.top_left, self.position.bottom_right
        )
    }
}

impl Into<QRCode> for &Barcode {
    fn into(self) -> QRCode {
        QRCode {
            text: self.text(),
            position: self.position(),
        }
    }
}

pub fn decode_qr(
    grey_img_mutex: Arc<Mutex<Option<GrayImage>>>,
    qrcodes: Arc<Mutex<Option<Vec<QRCode>>>>,
    stop: Arc<AtomicBool>,
) {
    let barcode_reader = zxingcpp::read().formats(BarcodeFormat::QRCode).try_invert(false);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(51));
        let grey_img_opt = { grey_img_mutex.lock().ok().and_then(|mut img| img.take()) };
        if let Some(grey_img) = grey_img_opt {
            let barcodes = barcode_reader.from(&grey_img).unwrap();
            if let Ok(mut qrcodes) = qrcodes.lock() {
                *qrcodes = Some(barcodes.iter().map(Into::into).collect());
            }
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
    }
}
