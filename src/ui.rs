use smallvec::smallvec;

use gpui::{
    actions, div, img, prelude::*, px, size, App, AppContext, Application, Bounds, Context, ImageSource, KeyBinding,
    Menu, MenuItem, Point, RenderImage, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use image::{Frame, RgbaImage};

struct ImageShowcase {
    img: RgbaImage,
}

impl Render for ImageShowcase {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let frame = Frame::new(self.img.clone());
        let render_image = RenderImage::new(smallvec![frame]);
        let image_data = ImageSource::Render(render_image.into());

        div().size_full().child(
            img(image_data)
                .size_full()
                .object_fit(gpui::ObjectFit::Contain)
                .id("png"),
        )
    }
}

actions!(image, [Quit]);

pub fn ui(img: RgbaImage) {
    Application::new().run(move |cx: &mut App| {
        cx.activate(true);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.set_menus(vec![Menu {
            name: "Image".into(),
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

        cx.open_window(window_options, |_, cx| cx.new(|_| ImageShowcase { img }))
            .unwrap();
    });
}
