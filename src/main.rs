use gpui::{
    actions, div, img, prelude::*, px, size, App, Application, Bounds, Context, ImageSource, KeyBinding, Menu,
    MenuItem, Point, RenderImage, SharedString, Task, Timer, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use image::{Frame, RgbaImage};
use std::{sync::Arc, time::Duration};

use camera::{DeviceCapture, DeviceInfo};
use decode::Decoder;
use qr::QRCode;

mod camera;
mod decode;
mod qr;

struct ImageDisplay {
    decoder: Option<Decoder>,
    task: Option<Task<()>>,
    camera: Option<SharedString>,
    qrcodes: Vec<QRCode>,
    img: Option<RgbaImage>,
    last_image: Option<Arc<RenderImage>>,
}

impl ImageDisplay {
    fn new(decoder: Decoder) -> Self {
        Self {
            decoder: Some(decoder),
            task: None,
            camera: None,
            qrcodes: Vec::new(),
            img: None,
            last_image: None,
        }
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(decoder) = self.decoder.take() else {
            return;
        };

        self.task = Some(cx.spawn_in(window, async move |view, cx| {
            let devices = DeviceInfo::find_all();
            let device_info = devices.first().unwrap();

            view.update(cx, |view, cx| {
                view.camera = Some(device_info.name.clone().into());
                cx.notify();
            })
            .unwrap();

            let _capture = DeviceCapture::start(&device_info, decoder.clone()).unwrap();

            loop {
                Timer::after(Duration::from_millis(37)).await;
                let opt_img = decoder.take_img();
                let opt_qrcodes = decoder.take_qrcodes();

                if opt_img.is_some() || opt_qrcodes.is_some() {
                    view.update(cx, |view, cx| {
                        if let Some(img) = opt_img {
                            view.img = Some(img);
                        }
                        if let Some(qrcodes) = opt_qrcodes {
                            view.qrcodes = qrcodes;
                        }
                        cx.notify();
                    })
                    .unwrap();
                }
            }
        }));
    }
}

impl Render for ImageDisplay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.start(window, cx);

        let image_data = if let Some(qr_img) = self.img.take() {
            let frame = Frame::new(qr_img);
            let image_render = Arc::new(RenderImage::new(vec![frame]));
            if let Some(last_image) = self.last_image.replace(image_render.clone()) {
                window.drop_image(last_image).unwrap();
            }
            ImageSource::Render(image_render)
        } else if let Some(last_image) = self.last_image.as_ref() {
            ImageSource::Render(last_image.clone())
        } else {
            ImageSource::Image(gpui::Image::empty().into())
        };

        let text = match self.camera.as_ref() {
            Some(text) => text.clone(),
            None => "Loading...".into(),
        };

        div()
            .size_full()
            .flex()
            .flex_col_reverse()
            .font_family(".SystemUIFont")
            .bg(gpui::black())
            .text_color(gpui::white())
            .items_center()
            .child(img(image_data).size_full().object_fit(gpui::ObjectFit::Cover))
            .child(
                self.qrcodes
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<String>>()
                    .join("\n"),
            )
            .child(text)
    }
}

actions!(qr_cam, [Quit]);

pub fn main() {
    Application::new().run(move |cx: &mut App| {
        cx.activate(true);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("ctrl-c", Quit, None)]);
        cx.on_window_closed(|cx| {
            cx.quit();
        })
        .detach();

        let decoder = Decoder::new();
        let decoder_display = decoder.clone();

        cx.on_app_quit(move |_| {
            let decoder_quit = decoder.clone();
            async move {
                decoder_quit.shutdown();
            }
        })
        .detach();

        cx.set_menus(vec![Menu {
            name: "QR Cam".into(),
            items: vec![MenuItem::action("Quit", Quit)],
        }]);

        let window_options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                appears_transparent: true,
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                size: size(px(900.), px(480.)),
                origin: Point::new(px(400.), px(100.)),
            })),
            focus: true,
            show: true,
            ..Default::default()
        };

        cx.open_window(window_options, |_, cx| cx.new(|_| ImageDisplay::new(decoder_display)))
            .unwrap();
    });
}
