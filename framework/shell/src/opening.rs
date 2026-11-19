use std::any::Any;
use std::rc::Rc;

#[derive(Clone, Default)]
pub struct Opening(Option<Rc<dyn Any>>);

impl Opening {
    pub fn of<T: 'static>(value: T) -> Self {
        Self(Some(Rc::new(value)))
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.0.as_deref()?.downcast_ref()
    }

    pub const fn is_some(&self) -> bool {
        self.0.is_some()
    }
}
