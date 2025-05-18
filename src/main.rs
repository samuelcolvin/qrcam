#![allow(dead_code)]
#![allow(unused_imports)]

use std::sync::{Arc, Mutex};

use av_foundation::capture_device::AVCaptureDeviceTypeExternalUnknown;
use av_foundation::{
    capture_device::{
        AVCaptureDevice, AVCaptureDeviceDiscoverySession, AVCaptureDeviceFormat, AVCaptureDevicePositionUnspecified,
        AVCaptureDeviceTypeBuiltInWideAngleCamera, AVCaptureDeviceTypeExternal,
    },
    capture_input::AVCaptureDeviceInput,
    capture_output_base::AVCaptureOutput,
    capture_session::{AVCaptureConnection, AVCaptureSession},
    capture_video_data_output::{AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate},
    media_format::AVMediaTypeVideo,
};
use core_foundation::base::TCFType;
use core_media::{
    format_description::{CMVideoCodecType, CMVideoFormatDescription},
    sample_buffer::{CMSampleBuffer, CMSampleBufferRef},
    time::CMTime,
};
use core_video::pixel_buffer::{
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelFormatType_420YpCbCr8Planar,
    kCVPixelFormatType_422YpCbCr8, kCVPixelFormatType_422YpCbCr8_yuvs, CVPixelBuffer, CVPixelBufferKeys,
};
use dispatch2::{Queue, QueueAttribute};
use image::{Rgb, RgbImage};
use objc2::{
    declare_class, extern_methods, msg_send_id, mutability,
    rc::{Allocated, Id, Retained},
    runtime::ProtocolObject,
    ClassType, DeclaredClass,
};
use objc2_foundation::{NSArray, NSMutableArray, NSMutableDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
use x_media::{
    media_frame::MediaFrame,
    video::{ColorRange, PixelFormat, VideoFormat},
};

fn main() {
    let devices = DeviceInfo::find_all();
    dbg!(&devices);
    let device_info = devices.first().unwrap();
    let handler = Handler::default();

    let mut capture = DeviceCapture::start(&device_info, handler.clone()).unwrap();

    std::thread::sleep(std::time::Duration::from_secs(1));

    capture.stop();

    dbg!(&handler);
    handler.save();
}

#[derive(Clone, Debug)]
struct DeviceInfo {
    id: String,
    name: String,
}

impl DeviceInfo {
    fn find_all() -> Vec<Self> {
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

struct ImageData {
    timestamp: u64,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageData")
            .field("timestamp", &self.timestamp)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("data", &self.data.len())
            .finish()
    }
}

impl ImageData {
    fn save(&self) {
        let mut img = RgbImage::new(self.width / 2, self.height);
        for row in 0..self.height {
            for col in (0..self.width).step_by(2) {
                let index = ((row * self.width + col) as usize) * 2;

                let u = self.data[index];
                let y0 = self.data[index + 1];
                let v = self.data[index + 2];
                let y1 = self.data[index + 3];

                // First pixel
                let rgb0 = yuv_to_rgb(y0 as f32, u as f32, v as f32);
                img.put_pixel(col, row, Rgb([rgb0[0], rgb0[1], rgb0[2]]));

                // Second pixel
                if col + 1 < self.width {
                    let rgb1 = yuv_to_rgb(y1 as f32, u as f32, v as f32);
                    img.put_pixel(col + 1, row, Rgb([rgb1[0], rgb1[1], rgb1[2]]));
                }
            }
        }
        img.save("output.png").unwrap();
    }
}

fn yuv_to_rgb(y: f32, u: f32, v: f32) -> [u8; 3] {
    let c = y - 16.0;
    let d = u - 128.0;
    let e = v - 128.0;

    let r = (298.082 * c + 408.583 * e) / 256.0 + 128.0;
    let g = (298.082 * c - 100.291 * d - 208.120 * e) / 256.0 + 128.0;
    let b = (298.082 * c + 516.412 * d) / 256.0 + 128.0;

    [clamp(r), clamp(g), clamp(b)]
}

fn clamp(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

#[derive(Debug, Default, Clone)]
struct Handler {
    image: Arc<Mutex<Option<ImageData>>>,
}

impl Handler {
    fn handle(&self, frame: MediaFrame) {
        println!("frame desc: {:?}", frame.description());

        let Ok(mapped_guard) = frame.map() else {
            return;
        };
        let Some(planes) = mapped_guard.planes() else {
            return;
        };
        for plane in planes {
            let Some(width) = plane.stride() else {
                continue;
            };
            let Some(height) = plane.height() else {
                continue;
            };
            let Some(data) = plane.data() else {
                continue;
            };

            let mut image = self.image.lock().unwrap();
            *image = Some(ImageData {
                timestamp: frame.timestamp,
                width,
                height,
                data: data.to_vec(),
            });
        }
    }

    fn save(&self) {
        let image = self.image.lock().unwrap();
        if let Some(image) = image.as_ref() {
            image.save();
        }
    }
}

pub struct DeviceCapture {
    info: DeviceInfo,
    session: Id<AVCaptureSession>,
    input: Id<AVCaptureDeviceInput>,
    output: Id<AVCaptureVideoDataOutput>,
    delegate: Id<OutputDelegate>,
    running: bool,
}

impl DeviceCapture {
    fn start(info: &DeviceInfo, handler: Handler) -> Result<DeviceCapture, String> {
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
            info: info.clone(),
            session,
            input,
            output,
            delegate,
            running: true,
        })
    }

    fn stop(&mut self) {
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

            if let Some(mut video_frame) = video_frame {
                if let Some(handler) = self.ivars().handler.as_ref() {
                    video_frame.timestamp = (sample_buffer.get_presentation_time_stamp().get_seconds() * 1000.0) as u64;
                    handler.handle(video_frame);
                }
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
