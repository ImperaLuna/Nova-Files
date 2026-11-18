use gpui::{App, Global};

pub struct Published<T>(pub T);

impl<T: 'static> Global for Published<T> {}

impl<T: 'static> Published<T> {
    pub fn set(value: T, cx: &mut App) {
        cx.set_global(Self(value));
    }

    pub fn get(cx: &App) -> Option<&T> {
        cx.try_global::<Self>().map(|published| &published.0)
    }
}
