#![allow(unused)]
#![feature(register_tool)]
#![register_tool(__pyly)]

use std::collections::{HashMap, HashSet};

use pyly::{python as py, Exposed, Python as Py};

struct A(usize, usize);

pub struct Svelte {
    a1: A,
    a2: A,
}

impl Exposed<Py> for Svelte {
    const AS: <Py as pyly::Language>::Type = <(
        (),
        u8,
        f32,
        bool,
        &'static str,
        (Vec<u8>, HashSet<u16>, HashMap<String, usize>),
    )>::AS;
}

//ad adsba sdsaadsasddas
fn main() {
    // dsd
}
