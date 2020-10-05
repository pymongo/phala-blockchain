// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License..

//! A library for acquiring a backtrace at runtime
//!
//! This library is meant to supplement the `RUST_BACKTRACE=1` support of the
//! standard library by allowing an acquisition of a backtrace at runtime
//! programmatically. The backtraces generated by this library do not need to be
//! parsed, for example, and expose the functionality of multiple backend
//! implementations.
//!
//! # Implementation
//!
//! This library makes use of a number of strategies for actually acquiring a
//! backtrace. For example unix uses libgcc's libunwind bindings by default to
//! acquire a backtrace, but coresymbolication or dladdr is used on OSX to
//! acquire symbol names while linux uses gcc's libbacktrace.
//!
//! When using the default feature set of this library the "most reasonable" set
//! of defaults is chosen for the current platform, but the features activated
//! can also be controlled at a finer granularity.
//!
//! # API Principles
//!
//! This library attempts to be as flexible as possible to accommodate different
//! backend implementations of acquiring a backtrace. Consequently the currently
//! exported functions are closure-based as opposed to the likely expected
//! iterator-based versions. This is done due to limitations of the underlying
//! APIs used from the system.
//!
//! # Usage
//!
//! First, add this to your Cargo.toml
//!
//! ```toml
//! [dependencies]
//! sgx_backtrace = "1.1.2"
//! ```
//!
//! Next:
//!
//! ```
//! extern crate sgx_backtrace;
//!
//! fn main() {
//! # // Unsafe here so test passes on no_std.
//! # #[cfg(feature = "std")] {
//!     sgx_backtrace::set_enclave_path("enclave.signed.so");
//!     sgx_backtrace::trace(|frame| {
//!         let ip = frame.ip();
//!         let symbol_address = frame.symbol_address();
//!
//!         // Resolve this instruction pointer to a symbol name
//!         sgx_backtrace::resolve_frame(frame, |symbol| {
//!             if let Some(name) = symbol.name() {
//!                 // ...
//!             }
//!             if let Some(filename) = symbol.filename() {
//!                 // ...
//!             }
//!         });
//!
//!         true // keep going to the next frame
//!     });
//! }
//! # }
//! ```
#![no_std]

#![cfg_attr(all(target_env = "sgx", target_vendor = "mesalock", feature = "std"), feature(rustc_private))]
#![cfg_attr(feature = "nostd", feature(panic_unwind))]

#[cfg(all(not(target_env = "sgx"), feature = "std"))]
#[macro_use]
extern crate sgx_tstd as std;
#[cfg(all(target_env = "sgx", feature = "std"))]
#[macro_use]
extern crate std;

extern crate alloc;
extern crate sgx_backtrace_sys as bt;

#[cfg(feature = "nostd")]
#[allow(unused_extern_crates)]
extern crate sgx_unwind;

#[macro_use]
extern crate sgx_types;
extern crate sgx_trts;
extern crate sgx_libc;
extern crate sgx_demangle;

#[cfg(feature = "serialize")]
extern crate sgx_serialize;
#[cfg(feature = "serialize")]
#[macro_use]
extern crate sgx_serialize_derive;

pub use crate::backtrace::{trace_unsynchronized, Frame};
mod backtrace;

pub use crate::symbolize::resolve_frame_unsynchronized;
pub use crate::symbolize::{resolve_unsynchronized, Symbol, SymbolName};
pub use crate::symbolize::set_enclave_path;
mod symbolize;

pub use crate::types::BytesOrWideString;
mod types;

#[cfg(feature = "std")]
pub use crate::symbolize::clear_symbol_cache;

mod print;
pub use print::{BacktraceFmt, BacktraceFrameFmt, PrintFmt};
cfg_if! {
    if #[cfg(feature = "std")] {
        pub use crate::backtrace::trace;
        pub use crate::symbolize::{resolve, resolve_frame};
        pub use crate::capture::{Backtrace, BacktraceFrame, BacktraceSymbol};
        mod capture;
    }
}

#[allow(dead_code)]
struct Bomb {
    enabled: bool,
}

#[allow(dead_code)]
impl Drop for Bomb {
    fn drop(&mut self) {
        if self.enabled {
            panic!("cannot panic during the backtrace function");
        }
    }
}

#[allow(dead_code)]
#[cfg(feature = "std")]
mod lock {
    use std::boxed::Box;
    use std::cell::Cell;
    use std::sync::{SgxMutex, SgxMutexGuard, Once};

    pub struct LockGuard(Option<SgxMutexGuard<'static, ()>>);

    static mut LOCK: *mut SgxMutex<()> = 0 as *mut _;
    static INIT: Once = Once::new();
    thread_local!(static LOCK_HELD: Cell<bool> = Cell::new(false));

    impl Drop for LockGuard {
        fn drop(&mut self) {
            if self.0.is_some() {
                LOCK_HELD.with(|slot| {
                    assert!(slot.get());
                    slot.set(false);
                });
            }
        }
    }

    pub fn lock() -> LockGuard {
        if LOCK_HELD.with(|l| l.get()) {
            return LockGuard(None);
        }
        LOCK_HELD.with(|s| s.set(true));
        unsafe {
            INIT.call_once(|| {
                LOCK = Box::into_raw(Box::new(SgxMutex::new(())));
            });
            LockGuard(Some((*LOCK).lock().unwrap()))
        }
    }
}