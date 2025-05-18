use std::{sync::Arc, time::Duration};

use smallvec::smallvec;

use gpui::{
    actions, div, img, prelude::*, px, rgb, size, App, AppContext, Application, Bounds, Context, ImageCacheError,
    ImageSource, KeyBinding, Menu, MenuItem, Point, RenderImage, SharedString, Task, Timer, TitlebarOptions, Window,
    WindowBounds, WindowOptions,
};
use image::{Frame, RgbaImage};

use camera::{DeviceCapture, DeviceInfo, Handler};

mod camera;

#[derive(Default)]
struct ImageDisplay {
    _task: Option<Task<()>>,
    text: SharedString,
    img: Option<RgbaImage>,
}

impl ImageDisplay {
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self._task.is_some() {
            return;
        }
        cx.notify();

        self._task = Some(cx.spawn_in(window, async move |view, cx| {
            let devices = DeviceInfo::find_all();
            let device_info = devices.first().unwrap();

            view.update(cx, |view, cx| {
                view.text = format!("Using {}", device_info.name).into();
                cx.notify();
            })
            .unwrap();

            let handler = Handler::default();

            let _capture = DeviceCapture::start(&device_info, handler.clone()).unwrap();

            loop {
                Timer::after(Duration::from_millis(100)).await;

                if let Some(img) = handler.take_img() {
                    view.update(cx, |view, cx| {
                        view.img = Some(img);
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
            let render_image = RenderImage::new(smallvec![frame]);
            ImageSource::Render(render_image.into())
        } else {
            ImageSource::Image(gpui::Image::empty().into())
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x000000))
            .text_color(rgb(0xffffff))
            .items_center()
            .child(img(image_data).size_full().object_fit(gpui::ObjectFit::Cover))
            .child(self.text.clone())
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
            name: "QR Image".into(),
            items: vec![MenuItem::action("Quit", Quit)],
        }]);

        let window_options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from("Image Example")),
                appears_transparent: false,
                ..Default::default()
            }),

            window_bounds: Some(WindowBounds::Windowed(Bounds {
                size: size(px(1100.), px(600.)),
                origin: Point::new(px(200.), px(200.)),
            })),

            ..Default::default()
        };

        cx.open_window(window_options, |_, cx| cx.new(|_| ImageDisplay::default()))
            .unwrap();
    });
}
