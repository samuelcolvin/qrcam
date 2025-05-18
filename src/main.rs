#![allow(dead_code)]
#![allow(unused_imports)]

use std::sync::Arc;

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

    let _capture = DeviceCapture::start(&device_info, Arc::new(handler)).unwrap();

    std::thread::sleep(std::time::Duration::from_secs(1));
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

#[derive(Debug)]
struct ImageData {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

#[derive(Debug, Default)]
struct Handler {
    image: Option<ImageData>,
}

impl Handler {
    fn handle(&self, frame: MediaFrame) {
        println!("frame desc: {:?}", frame.description());
        println!("frame timestamp: {:?}", frame.timestamp);

        let Ok(mapped_guard) = frame.map() else {
            return;
        };
        let Some(planes) = mapped_guard.planes() else {
            return;
        };
        for plane in planes {
            let Some(plane_width) = plane.stride() else {
                continue;
            };
            let Some(plane_height) = plane.height() else {
                continue;
            };
            let Some(plane_data) = plane.data() else {
                continue;
            };

            dbg!(plane_width, plane_height, plane_data.len());
        }
    }
}

pub struct DeviceCapture {
    info: DeviceInfo,
    // formats: Vec<CameraFormat>,
    // handler: Arc<Handler>,
    session: Id<AVCaptureSession>,
    // device: Id<AVCaptureDevice>,
    input: Id<AVCaptureDeviceInput>,
    output: Id<AVCaptureVideoDataOutput>,
    delegate: Id<OutputDelegate>,
    running: bool,
}

impl DeviceCapture {
    fn start(info: &DeviceInfo, handler: Arc<Handler>) -> Result<DeviceCapture, String> {
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

        // let formats = get_formats(&device);

        session.commit_configuration();
        session.start_running();

        Ok(Self {
            info: info.clone(),
            // formats,
            // handler: handler.clone(),
            session,
            // device,
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
    handler: Option<Arc<Handler>>,
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

// fn get_formats(device: &AVCaptureDevice) -> Vec<CameraFormat> {
//     device
//         .formats()
//         .iter()
//         .filter_map(from_av_capture_device_format)
//         .collect()
// }

// #[derive(Clone, Debug)]
// struct CameraFormat {
//     format: VideoFormat,
//     color_range: ColorRange,
//     width: u32,
//     height: u32,
//     frame_rates: Vec<f32>,
// }

// fn from_av_capture_device_format(format: &AVCaptureDeviceFormat) -> Option<CameraFormat> {
//     if let Some(desc) = format.format_description().downcast_into::<CMVideoFormatDescription>() {
//         let dimensions = desc.get_dimensions();
//         match from_cm_codec_type(desc.get_codec_type()) {
//             Some((video_format, color_range)) => {
//                 let frame_rate_ranges = format.video_supported_frame_rate_ranges();
//                 let frame_rates = frame_rate_ranges
//                     .iter()
//                     .map(|range| range.max_frame_rate() as f32)
//                     .collect();

//                 Some(CameraFormat {
//                     format: video_format,
//                     color_range,
//                     width: dimensions.width as u32,
//                     height: dimensions.height as u32,
//                     frame_rates,
//                 })
//             }
//             None => None,
//         }
//     } else {
//         None
//     }
// }

// fn from_cm_codec_type(codec_type: CMVideoCodecType) -> Option<(VideoFormat, ColorRange)> {
//     #[allow(non_upper_case_globals)]
//     match codec_type {
//         kCVPixelFormatType_420YpCbCr8Planar => Some((VideoFormat::Pixel(PixelFormat::I420), ColorRange::Video)),
//         kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange => {
//             Some((VideoFormat::Pixel(PixelFormat::NV12), ColorRange::Video))
//         }
//         kCVPixelFormatType_422YpCbCr8_yuvs => Some((VideoFormat::Pixel(PixelFormat::YUYV), ColorRange::Video)),
//         kCVPixelFormatType_422YpCbCr8 => Some((VideoFormat::Pixel(PixelFormat::UYVY), ColorRange::Video)),
//         _ => None,
//     }
// }
