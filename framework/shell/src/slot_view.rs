use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div,
};

pub type Draw = Box<dyn Fn(&mut Window, &mut App) -> Option<AnyElement>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Content,
    Fill,
    Cover,
    Shrink,
}

pub struct SlotView {
    draw: Draw,
    fit: Fit,
}

impl SlotView {
    pub fn new(draw: Draw, fit: Fit) -> Self {
        Self { draw, fit }
    }
}

impl Render for SlotView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let drawn = (self.draw)(window, cx);
        let slot = div().children(drawn);
        match self.fit {
            Fit::Content => slot,
            Fit::Fill => slot.flex().flex_col().flex_1().min_h_0(),
            Fit::Cover => slot.absolute().inset_0(),
            Fit::Shrink => slot.flex().flex_shrink().min_w_0(),
        }
    }
}
