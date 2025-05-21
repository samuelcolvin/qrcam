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
use objc2::{
    declare_class, extern_methods, msg_send_id, mutability,
    rc::{Allocated, Id},
    runtime::ProtocolObject,
    ClassType, DeclaredClass,
};
use objc2_foundation::{NSMutableArray, NSObject, NSObjectProtocol, NSString};
use x_media::media_frame::MediaFrame;

use crate::decode::Decoder;

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

pub struct DeviceCapture {
    session: Id<AVCaptureSession>,
    input: Id<AVCaptureDeviceInput>,
    output: Id<AVCaptureVideoDataOutput>,
    // we have to keep a reference to the delegate to prevent it from being dropped
    _delegate: Id<OutputDelegate>,
    running: bool,
}

impl DeviceCapture {
    pub fn start(info: &DeviceInfo, decoder: Decoder) -> Result<DeviceCapture, String> {
        let session = AVCaptureSession::new();
        let id = NSString::from_str(&info.id);
        let device = AVCaptureDevice::device_with_unique_id(&id).ok_or("Device not found")?;
        let output = AVCaptureVideoDataOutput::new();
        let input =
            AVCaptureDeviceInput::from_device(&device).map_err(|err| format!("Failed to create input: {}", err))?;
        let mut delegate = OutputDelegate::new();
        let queue = Queue::new("com.video-capture.output", QueueAttribute::Serial);
        let ivars = delegate.ivars_mut();

        ivars.decoder = Some(decoder);

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
    decoder: Option<Decoder>,
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
                let decoder = self.ivars().decoder.as_ref().unwrap();
                decoder.decode(video_frame);
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
