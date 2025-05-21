use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use image::{GrayImage, Luma, Rgba, RgbaImage};
use x_media::media_frame::MediaFrame;

use crate::qr::{decode_qr, QRCode};

#[derive(Clone)]
pub struct Decoder {
    rgba_image: Arc<Mutex<Option<RgbaImage>>>,
    grey_image: Arc<Mutex<Option<GrayImage>>>,
    qrcodes: Arc<Mutex<Option<Vec<QRCode>>>>,
    stop: Arc<AtomicBool>,
    join_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl Decoder {
    pub fn new() -> Self {
        let grey_image = Arc::new(Mutex::new(None));
        let grey_image_mov = grey_image.clone();
        let qrcodes = Arc::new(Mutex::new(None));
        let qrcodes_mov = qrcodes.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_mov = stop.clone();
        let join_handle = thread::spawn(move || decode_qr(grey_image_mov, qrcodes_mov, stop_mov));
        Self {
            rgba_image: Arc::new(Mutex::new(None)),
            grey_image,
            qrcodes,
            stop,
            join_handle: Arc::new(Mutex::new(Some(join_handle))),
        }
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join_handle.lock().ok().and_then(|mut h| h.take()) {
            handle.join().unwrap();
        }
    }

    pub fn take_img(&self) -> Option<RgbaImage> {
        self.rgba_image.lock().ok().and_then(|mut img| img.take())
    }

    pub fn take_qrcodes(&self) -> Option<Vec<QRCode>> {
        self.qrcodes.lock().ok().and_then(|mut qrcodes| qrcodes.take())
    }

    pub fn decode(&self, frame: MediaFrame) {
        // println!("frame desc: {:?}", frame.description());

        let Ok(mapped_guard) = frame.map() else {
            return;
        };
        let Some(planes) = mapped_guard.planes() else {
            return;
        };
        for plane in planes {
            match (plane.stride(), plane.height(), plane.data()) {
                (Some(stride), Some(height), Some(data)) => self.record_img(stride, height, data),
                _ => (),
            };
        }
    }

    fn record_img(&self, stride: u32, height: u32, data: &[u8]) {
        // For YUV422 format, the actual number of pixels is half the stride width
        let width = stride / 2;
        let mut rgba_img = RgbaImage::new(width, height);
        let mut grey_img = GrayImage::new(width, height);

        for row in 0..height {
            for x in 0..width / 2 {
                // flip the image horizontally
                let x_reverse = width - x - 1;
                // Each 4 bytes represent 2 pixels in UYVY format
                let idx = (row * stride + x_reverse * 4) as usize;

                // Safety check to avoid out of bounds access
                if idx + 3 >= data.len() {
                    continue;
                }

                // Extract UYVY values - note because the image is flipped horizontally
                // we select items in this order, not u, y0, v, y1
                let v = data[idx];
                let y1 = data[idx + 1];
                let u = data[idx + 2];
                let y0 = data[idx + 3];

                // Convert to RGB
                let rgb0 = yuv_to_rgb(y0 as f32, u as f32, v as f32);
                let rgb1 = yuv_to_rgb(y1 as f32, u as f32, v as f32);

                // Place both pixels in the output image
                rgba_img.put_pixel(x * 2, row, Rgba([rgb0[0], rgb0[1], rgb0[2], 255]));
                rgba_img.put_pixel(x * 2 + 1, row, Rgba([rgb1[0], rgb1[1], rgb1[2], 255]));

                grey_img.put_pixel(x * 2, row, Luma([y0]));
                grey_img.put_pixel(x * 2 + 1, row, Luma([y1]));
            }
        }
        if let Ok(mut image) = self.rgba_image.lock() {
            *image = Some(rgba_img);
        }
        if let Ok(mut grey_image) = self.grey_image.lock() {
            *grey_image = Some(grey_img);
        }
    }
}

fn yuv_to_rgb(y: f32, u: f32, v: f32) -> [u8; 3] {
    let r = y + (1.402 * (v - 128.));
    let g = y - (0.344136 * (u - 128.)) - (0.714136 * (v - 128.));
    let b = y + (1.772 * (u - 128.));

    [clamp(r), clamp(g), clamp(b)]
}

fn clamp(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}
