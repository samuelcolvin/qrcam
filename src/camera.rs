use std::fmt;
use std::sync::{Arc, Mutex};

use av_foundation::capture_device::AVCaptureDeviceTypeExternalUnknown;
use av_foundation::{
    capture_device::{
        AVCaptureDevice, AVCaptureDeviceDiscoverySession, AVCaptureDevicePositionUnspecified,
        AVCaptureDeviceTypeBuiltInWideAngleCamera, AVCaptureDeviceTypeExternal,
    },
    capture_input::AVCaptureDeviceInput,
    capture_output_base::AVCaptureOutput,
    capture_session::{AVCaptureConnection, AVCaptureSession},
    capture_video_data_output::{AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate},
    media_format::AVMediaTypeVideo,
};
use core_foundation::base::TCFType;
use core_media::sample_buffer::{CMSampleBuffer, CMSampleBufferRef};
use core_video::pixel_buffer::CVPixelBuffer;
use dispatch2::{Queue, QueueAttribute};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use objc2::{
    declare_class, extern_methods, msg_send_id, mutability,
    rc::{Allocated, Id},
    runtime::ProtocolObject,
    ClassType, DeclaredClass,
};
use objc2_foundation::{NSMutableArray, NSObject, NSObjectProtocol, NSString};
use x_media::media_frame::MediaFrame;
use zxingcpp::{Barcode, BarcodeFormat, BarcodeReader, Position};

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    id: String,
    pub name: String,
}

impl DeviceInfo {
    pub fn find_all() -> Vec<Self> {
        let mut device_types = NSMutableArray::new();
        let session = unsafe {
            device_types.addObject(AVCaptureDeviceTypeBuiltInWideAngleCamera);
            device_types.addObject(AVCaptureDeviceTypeExternal);
            device_types.addObject(AVCaptureDeviceTypeExternalUnknown);
            AVCaptureDeviceDiscoverySession::discovery_session_with_device_types(
                &device_types,
                AVMediaTypeVideo,
                AVCaptureDevicePositionUnspecified,
            )
        };
        session
            .devices()
            .iter()
            .map(|device| DeviceInfo {
                id: device.unique_id().to_string(),
                name: device.localized_name().to_string(),
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct QrCode {
    text: String,
    position: Position,
}
impl fmt::Display for QrCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}/{}",
            self.text, self.position.top_left, self.position.bottom_right
        )
    }
}

impl Into<QrCode> for &Barcode {
    fn into(self) -> QrCode {
        QrCode {
            text: self.text(),
            position: self.position(),
        }
    }
}

#[derive(Debug)]
struct QrImage {
    img: RgbaImage,
    qrcodes: Vec<QrCode>,
}

#[derive(Clone)]
pub struct Handler {
    image: Arc<Mutex<Option<QrImage>>>,
    barcode_reader: Arc<BarcodeReader>,
}

impl Handler {
    pub fn new() -> Self {
        Self {
            image: Arc::new(Mutex::new(None)),
            barcode_reader: Arc::new(zxingcpp::read().formats(BarcodeFormat::QRCode).try_invert(false)),
        }
    }

    pub fn take_img(&self) -> Option<(RgbaImage, Vec<QrCode>)> {
        if let Ok(mut image) = self.image.lock() {
            image.take().map(|qr_image| (qr_image.img, qr_image.qrcodes))
        } else {
            None
        }
    }

    fn handle(&self, frame: MediaFrame) {
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

        let barcodes = self.barcode_reader.from(&grey_img).unwrap();
        let qrcodes = barcodes.iter().map(Into::into).collect();

        let mut image = self.image.lock().unwrap();
        *image = Some(QrImage { img: rgba_img, qrcodes });
    }
}

pub struct DeviceCapture {
    session: Id<AVCaptureSession>,
    input: Id<AVCaptureDeviceInput>,
    output: Id<AVCaptureVideoDataOutput>,
    // we have to keep a reference to the delegate to prevent it from being dropped
    _delegate: Id<OutputDelegate>,
    running: bool,
}

impl DeviceCapture {
    pub fn start(info: &DeviceInfo, handler: Handler) -> Result<DeviceCapture, String> {
        let session = AVCaptureSession::new();
        let id = NSString::from_str(&info.id);
        let device = AVCaptureDevice::device_with_unique_id(&id).ok_or("Device not found")?;
        let output = AVCaptureVideoDataOutput::new();
        let input =
            AVCaptureDeviceInput::from_device(&device).map_err(|err| format!("Failed to create input: {}", err))?;
        let mut delegate = OutputDelegate::new();
        let queue = Queue::new("com.video-capture.output", QueueAttribute::Serial);
        let ivars = delegate.ivars_mut();

        ivars.handler = Some(handler);

        output.set_sample_buffer_delegate(ProtocolObject::from_ref(&*delegate), &queue);
        output.set_always_discards_late_video_frames(true);

        if session.can_add_input(&input) && session.can_add_output(&output) {
            session.add_input(&input);
            session.add_output(&output);
        } else {
            return Err("cannot add input or output".to_string());
        }

        session.begin_configuration();

        session.commit_configuration();
        session.start_running();

        Ok(Self {
            session,
            input,
            output,
            _delegate: delegate,
            running: true,
        })
    }

    pub fn stop(&mut self) {
        if self.running {
            self.session.remove_output(&self.output);
            self.session.stop_running();
            self.session.remove_input(&self.input);
            self.running = false;
        }
    }
}

impl Drop for DeviceCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct OutputDelegateIvars {
    handler: Option<Handler>,
}

declare_class!(
    struct OutputDelegate;

    unsafe impl ClassType for OutputDelegate {
        type Super = NSObject;
        type Mutability = mutability::Mutable;
        const NAME: &'static str = "OutputSampleBufferDelegate";
    }

    impl DeclaredClass for OutputDelegate {
        type Ivars = OutputDelegateIvars;
    }

    unsafe impl NSObjectProtocol for OutputDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for OutputDelegate {
        #[method(captureOutput:didOutputSampleBuffer:fromConnection:)]
        unsafe fn capture_output_did_output_sample_buffer(
            &self,
            _capture_output: &AVCaptureOutput,
            sample_buffer: CMSampleBufferRef,
            _connection: &AVCaptureConnection,
        ) {
            let sample_buffer = CMSampleBuffer::wrap_under_get_rule(sample_buffer);
            let video_frame = sample_buffer
                .get_image_buffer()
                .and_then(|image_buffer| image_buffer.downcast::<CVPixelBuffer>())
                .and_then(|pixel_buffer| MediaFrame::from_pixel_buffer(&pixel_buffer).ok());

            if let Some(video_frame) = video_frame {
                let handler = self.ivars().handler.as_ref().unwrap();
                handler.handle(video_frame);
            }
        }
    }

    unsafe impl OutputDelegate {
        #[method_id(init)]
        fn init(this: Allocated<Self>) -> Option<Id<Self>> {
            let this = this.set_ivars(OutputDelegateIvars::default());
            unsafe { msg_send_id![super(this), init] }
        }
    }
);

extern_methods!(
    unsafe impl OutputDelegate {
        #[method_id(new)]
        pub fn new() -> Id<Self>;
    }
);

fn yuv_to_rgb(y: f32, u: f32, v: f32) -> [u8; 3] {
    let r = y + (1.402 * (v - 128.));
    let g = y - (0.344136 * (u - 128.)) - (0.714136 * (v - 128.));
    let b = y + (1.772 * (u - 128.));

    [clamp(r), clamp(g), clamp(b)]
}

fn clamp(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}
