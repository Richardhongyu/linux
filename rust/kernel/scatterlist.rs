// SPDX-License-Identifier: GPL-2.0

//! scatterlist
//!
//! C header: [`include/scatterlist.h`](../../../../../include/scatterlist.h)

use crate::{bindings, str::CStr, to_result, ARef, AlwaysRefCounted, Error, Result};
use core::{cell::UnsafeCell, ptr::NonNull};

struct Scatterlist {
    scatterlist: bindings::scatterlist,
}

impl Scatterlist {
    fn new() -> Self {
        Self {
            scatterlist: bindings::scatterlist::default(),
        }
    }

    fn init(&mut self) {
        bindings::sg_init_table(self)
    }
}
