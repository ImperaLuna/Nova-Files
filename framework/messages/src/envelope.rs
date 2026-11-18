use std::any::{Any, TypeId, type_name};
use std::fmt;

pub trait Message: Any + Send + fmt::Debug {}

impl<T: Any + Send + fmt::Debug> Message for T {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageType {
    id: TypeId,
    name: &'static str,
}

impl MessageType {
    pub fn of<M: Message>() -> Self {
        Self {
            id: TypeId::of::<M>(),
            name: type_name::<M>(),
        }
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }
}

pub struct Envelope {
    body: Box<dyn Any + Send>,
    kind: MessageType,
    debug: fn(&dyn Any, &mut fmt::Formatter<'_>) -> fmt::Result,
}

impl Envelope {
    pub fn new<M: Message>(message: M) -> Self {
        Self {
            body: Box::new(message),
            kind: MessageType::of::<M>(),
            debug: debug_as::<M>,
        }
    }

    pub const fn kind(&self) -> MessageType {
        self.kind
    }

    pub fn get<M: Message>(&self) -> Option<&M> {
        self.body.downcast_ref()
    }

    pub fn open<M: Message>(self) -> Result<M, Self> {
        let Self { body, kind, debug } = self;
        body.downcast::<M>()
            .map(|message| *message)
            .map_err(|body| Self { body, kind, debug })
    }
}

fn debug_as<M: Message>(body: &dyn Any, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match body.downcast_ref::<M>() {
        Some(message) => message.fmt(f),
        None => f.write_str(type_name::<M>()),
    }
}

impl fmt::Debug for Envelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.debug)(self.body.as_ref(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum Ask {
        Ping,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Other(u8);

    #[test]
    fn an_envelope_opens_as_what_was_put_in() {
        let envelope = Envelope::new(Ask::Ping);
        assert_eq!(envelope.kind(), MessageType::of::<Ask>());
        assert_eq!(envelope.get::<Ask>(), Some(&Ask::Ping));
        assert_eq!(envelope.get::<Other>(), None);
        assert_eq!(envelope.open::<Ask>().ok(), Some(Ask::Ping));
    }

    #[test]
    fn opened_as_another_type_it_comes_back_whole() {
        let envelope = Envelope::new(Other(7));
        let back = envelope.open::<Ask>().err();
        assert_eq!(
            back.and_then(|back| back.open::<Other>().ok()),
            Some(Other(7))
        );
    }

    #[test]
    fn it_prints_as_its_message() {
        assert_eq!(format!("{:?}", Envelope::new(Other(3))), "Other(3)");
        assert!(MessageType::of::<Other>().name().ends_with("Other"));
    }
}
