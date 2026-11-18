use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div,
};

pub type Draw = Box<dyn Fn(&mut Window, &mut App) -> Option<AnyElement>>;

pub struct SlotView {
    draw: Draw,
    fills: bool,
}

impl SlotView {
    pub fn new(draw: Draw, fills: bool) -> Self {
        Self { draw, fills }
    }
}

impl Render for SlotView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let drawn = (self.draw)(window, cx);
        let slot = div().children(drawn);
        if self.fills {
            slot.flex().flex_col().flex_1().min_h_0()
        } else {
            slot
        }
    }
}
