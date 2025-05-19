use std::{sync::Arc, time::Duration};

use gpui::{
    actions, div, img, prelude::*, px, size, App, Application, Bounds, Context, ElementId, Entity, ImageCache,
    ImageCacheProvider, ImageSource, KeyBinding, Menu, MenuItem, Point, RenderImage, SharedString, Task, Timer,
    TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use image::{Frame, RgbaImage};

use camera::{DeviceCapture, DeviceInfo, Handler};

mod camera;

#[derive(Default)]
struct ImageDisplay {
    task: Option<Task<()>>,
    text: SharedString,
    img: Option<RgbaImage>,
}

impl ImageDisplay {
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task.is_some() {
            return;
        }
        cx.notify();

        self.task = Some(cx.spawn_in(window, async move |view, cx| {
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

        let image_data = if let Some(qr_img) = self.img.as_ref() {
            let frame = Frame::new(qr_img.clone());
            let image_render = RenderImage::new(vec![frame]);
            ImageSource::Render(image_render.into())
        } else {
            ImageSource::Image(gpui::Image::empty().into())
        };

        div()
            .image_cache(no_image_cache("lru-cache"))
            .size_full()
            .flex()
            .flex_col()
            .font_family(".SystemUIFont")
            .bg(gpui::black())
            .text_color(gpui::white())
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

fn no_image_cache(id: impl Into<ElementId>) -> NoCacheProvider {
    NoCacheProvider(id.into())
}

struct NoCacheProvider(ElementId);

impl ImageCacheProvider for NoCacheProvider {
    fn provide(&mut self, window: &mut Window, cx: &mut App) -> gpui::AnyImageCache {
        window
            .with_global_id(self.0.clone(), |global_id, window| {
                window.with_element_state::<Entity<NoImageCache>, _>(global_id, |no_cache, _| {
                    let no_cache = no_cache.unwrap_or_else(|| cx.new(|_| NoImageCache::default()));
                    (no_cache.clone(), no_cache)
                })
            })
            .into()
    }
}

#[derive(Default)]
struct NoImageCache;

impl ImageCache for NoImageCache {
    fn load(
        &mut self,
        _resource: &gpui::Resource,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<Result<Arc<gpui::RenderImage>, gpui::ImageCacheError>> {
        None
    }
}
