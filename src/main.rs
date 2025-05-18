use camera::{DeviceCapture, DeviceInfo, Handler};

mod camera;
mod ui;
// mod ui2;

fn main() {
    let devices = DeviceInfo::find_all();
    let device_info = devices.first().unwrap();
    println!("Using {}", device_info.name);
    let handler = Handler::default();

    let mut capture = DeviceCapture::start(&device_info, handler.clone()).unwrap();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if handler.image_set() {
            break;
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    capture.stop();

    // handler.save();

    let img = handler.img().unwrap();
    ui::ui(img);
    // ui2::ui();
}
