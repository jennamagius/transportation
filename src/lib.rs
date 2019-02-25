#![feature(unsized_locals)]

pub extern crate mio;

mod reactor;

pub use self::reactor::Reactor;
pub use self::reactor::ReactorWeak;
