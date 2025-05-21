use std::{sync::Arc, time::Duration};

use gpui::{
    actions, div, img, prelude::*, px, size, App, Application, Bounds, Context, ImageSource, KeyBinding, Menu,
    MenuItem, Point, RenderImage, SharedString, Task, Timer, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use image::{Frame, RgbaImage};

use camera::{DeviceCapture, DeviceInfo, Handler, QrCode};

mod camera;

#[derive(Default)]
struct ImageDisplay {
    task: Option<Task<()>>,
    camera: Option<SharedString>,
    qrcodes: Vec<QrCode>,
    img: Option<RgbaImage>,
    last_image: Option<Arc<RenderImage>>,
}

impl ImageDisplay {
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task.is_some() {
            return;
        }

        self.task = Some(cx.spawn_in(window, async move |view, cx| {
            let devices = DeviceInfo::find_all();
            let device_info = devices.first().unwrap();

            view.update(cx, |view, cx| {
                view.camera = Some(device_info.name.clone().into());
                cx.notify();
            })
            .unwrap();

            let handler = Handler::new();

            let _capture = DeviceCapture::start(&device_info, handler.clone()).unwrap();

            loop {
                Timer::after(Duration::from_millis(40)).await;

                if let Some((img, qrcodes)) = handler.take_img() {
                    view.update(cx, |view, cx| {
                        view.img = Some(img);
                        view.qrcodes = qrcodes;
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

actions!(image, [Quit]);

pub fn main() {
    Application::new().run(move |cx: &mut App| {
        cx.activate(true);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("ctrl-c", Quit, None)]);
        cx.on_window_closed(|cx| {
            cx.quit();
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

        cx.open_window(window_options, |_, cx| cx.new(|_| ImageDisplay::default()))
            .unwrap();
    });
}
