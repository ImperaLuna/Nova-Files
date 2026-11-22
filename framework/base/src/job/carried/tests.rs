use super::*;

#[derive(Debug, PartialEq, Eq)]
struct Pack(u8);

#[derive(Debug, PartialEq, Eq)]
struct Other(u8);

#[test]
fn it_opens_only_as_its_own_type() {
    let carried = Carried::new(Pack(3));
    assert_eq!(carried.get::<Pack>(), Some(&Pack(3)));
    assert_eq!(carried.get::<Other>(), None);
    assert!(carried.type_name().ends_with("Pack"));
}

#[test]
fn it_equals_only_the_same_value_of_the_same_type() {
    assert_eq!(Carried::new(Pack(1)), Carried::new(Pack(1)));
    assert_ne!(Carried::new(Pack(1)), Carried::new(Pack(2)));
    assert_ne!(Carried::new(Pack(1)), Carried::new(Other(1)));
}

#[test]
fn it_prints_as_its_value() {
    assert_eq!(format!("{:?}", Carried::new(Pack(4))), "Pack(4)");
}
