#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(clippy::erasing_op)]
#![allow(clippy::identity_op)]

use crate as protocrap;

include!(concat!(env!("OUT_DIR"), "/descriptor.pc.rs"));