#[macro_use]
extern crate wasmer_runtime;

use wasmer_runtime::LinearMemory;
use wasmer_runtime::types::{FuncSig, Type, Value};
use wasmer_runtime::{Import, Imports, Instance, FuncRef};
/// NOTE: TODO: These emscripten api implementation only support wasm32 for now because they assume offsets are u32
use byteorder::{ByteOrder, LittleEndian};
use libc::c_int;
use std::cell::UnsafeCell;
use std::mem;

// EMSCRIPTEN APIS
mod env;
mod errno;
mod exception;
mod io;
mod jmp;
mod lock;
mod math;
mod memory;
mod nullfunc;
mod process;
mod signal;
mod storage;
mod syscalls;
mod time;
mod utils;
mod varargs;

pub use self::storage::align_memory;
pub use self::utils::{allocate_cstr_on_stack, allocate_on_stack, is_emscripten_module};

// TODO: Magic number - how is this calculated?
const TOTAL_STACK: u32 = 5_242_880;
// TODO: Magic number - how is this calculated?
const DYNAMICTOP_PTR_DIFF: u32 = 1088;
// TODO: make this variable
const STATIC_BUMP: u32 = 215_536;

fn stacktop(static_bump: u32) -> u32 {
    align_memory(dynamictop_ptr(static_bump) + 4)
}

fn stack_max(static_bump: u32) -> u32 {
    stacktop(static_bump) + TOTAL_STACK
}

fn dynamic_base(static_bump: u32) -> u32 {
    align_memory(stack_max(static_bump))
}

fn dynamictop_ptr(static_bump: u32) -> u32 {
    static_bump + DYNAMICTOP_PTR_DIFF
}

pub struct EmscriptenData {
    pub malloc: extern "C" fn(i32, &Instance) -> u32,
    pub free: extern "C" fn(i32, &mut Instance),
    pub memalign: extern "C" fn(u32, u32, &mut Instance) -> u32,
    pub memset: extern "C" fn(u32, i32, u32, &mut Instance) -> u32,
    pub stack_alloc: extern "C" fn(u32, &Instance) -> u32,
    pub jumps: Vec<UnsafeCell<[c_int; 27]>>,
}

pub fn emscripten_set_up_memory(memory: &mut LinearMemory) {
    let dynamictop_ptr = dynamictop_ptr(STATIC_BUMP) as usize;
    let dynamictop_ptr_offset = dynamictop_ptr + mem::size_of::<u32>();

    // println!("value = {:?}");

    // We avoid failures of setting the u32 in our memory if it's out of bounds
    if dynamictop_ptr_offset > memory.len() {
        return; // TODO: We should panic instead?
    }

    // debug!("###### dynamic_base = {:?}", dynamic_base(STATIC_BUMP));
    // debug!("###### dynamictop_ptr = {:?}", dynamictop_ptr);
    // debug!("###### dynamictop_ptr_offset = {:?}", dynamictop_ptr_offset);

    let mem = &mut memory[dynamictop_ptr..dynamictop_ptr_offset];
    LittleEndian::write_u32(mem, dynamic_base(STATIC_BUMP));
}

macro_rules! mock_external {
    ($import:ident, $name:ident) => {{
        use wasmer_runtime::types::{FuncSig, Type};
        use wasmer_runtime::Import;
        extern "C" fn _mocked_fn() -> i32 {
            debug!("emscripten::{} <mock>", stringify!($name));
            -1
        }
        $import.add(
            "env".to_string(),
            stringify!($name).to_string(),
            Import::Func(
                unsafe { FuncRef::new(_mocked_fn as _) },
                FuncSig {
                    params: vec![],
                    returns: vec![Type::I32],
                },
            ),
        );
    }};
}

pub fn generate_emscripten_env() -> Imports {
    let mut import_object = Imports::new();

    //    import_object.add(
    //        "spectest".to_string(),
    //        "print_i32".to_string(),
    //        Import::Func(
    //            print_i32 as _,
    //            FuncSig {
    //                params: vec![Type::I32],
    //                returns: vec![],
    //            },
    //        ),
    //    );
    //
    //    import_object.add(
    //        "spectest".to_string(),
    //        "global_i32".to_string(),
    //        Import::Global(Value::I64(GLOBAL_I32 as _)),
    //    );

    // Globals
    import_object.add(
        "env".to_string(),
        "STACKTOP".to_string(),
        Import::Global(Value::I64(stacktop(STATIC_BUMP) as _)),
    );
    import_object.add(
        "env".to_string(),
        "STACK_MAX".to_string(),
        Import::Global(Value::I64(stack_max(STATIC_BUMP) as _)),
    );
    import_object.add(
        "env".to_string(),
        "DYNAMICTOP_PTR".to_string(),
        Import::Global(Value::I64(dynamictop_ptr(STATIC_BUMP) as _)),
    );
    import_object.add(
        "global".to_string(),
        "Infinity".to_string(),
        Import::Global(Value::I64(std::f64::INFINITY.to_bits() as _)),
    );
    import_object.add(
        "global".to_string(),
        "NaN".to_string(),
        Import::Global(Value::I64(std::f64::NAN.to_bits() as _)),
    );
    import_object.add(
        "env".to_string(),
        "tableBase".to_string(),
        Import::Global(Value::I64(0)),
    );
    //    // Print functions

    import_object.add(
        "env".to_string(),
        "printf".to_string(),
        Import::Func(
            unsafe { FuncRef::new(io::printf as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "putchar".to_string(),
        Import::Func(
            unsafe { FuncRef::new(io::putchar as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    //    // Lock
    import_object.add(
        "env".to_string(),
        "___lock".to_string(),
        Import::Func(
            unsafe { FuncRef::new(lock::___lock as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___unlock".to_string(),
        Import::Func(
            unsafe { FuncRef::new(lock::___unlock as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___wait".to_string(),
        Import::Func(
            unsafe { FuncRef::new(lock::___wait as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![],
            },
        ),
    );
    //    // Env
    import_object.add(
        "env".to_string(),
        "_getenv".to_string(),
        Import::Func(
            unsafe { FuncRef::new(env::_getenv as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_setenv".to_string(),
        Import::Func(
            unsafe { FuncRef::new(env::_setenv as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32, Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_putenv".to_string(),
        Import::Func(
            unsafe { FuncRef::new(env::_putenv as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_unsetenv".to_string(),
        Import::Func(
            unsafe { FuncRef::new(env::_unsetenv as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_getpwnam".to_string(),
        Import::Func(
            unsafe { FuncRef::new(env::_getpwnam as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_getgrnam".to_string(),
        Import::Func(
            unsafe { FuncRef::new(env::_getgrnam as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___buildEnvironment".to_string(),
        Import::Func(
            unsafe { FuncRef::new(env::___build_environment as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    //    // Errno
    import_object.add(
        "env".to_string(),
        "___setErrNo".to_string(),
        Import::Func(
            unsafe { FuncRef::new(errno::___seterrno as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    //    // Syscalls
    import_object.add(
        "env".to_string(),
        "___syscall1".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall1 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall3".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall3 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall4".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall4 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall5".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall5 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall6".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall6 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall12".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall12 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall20".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall20 as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall39".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall39 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall40".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall40 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall54".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall54 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall57".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall57 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall63".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall63 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall64".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall64 as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall102".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall102 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall114".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall114 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall122".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall122 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall140".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall140 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall142".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall142 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall145".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall145 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall146".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall146 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall180".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall180 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall181".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall181 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall192".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall192 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall195".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall195 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall197".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall197 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall201".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall201 as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall202".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall202 as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall212".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall212 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall221".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall221 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall330".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall330 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___syscall340".to_string(),
        Import::Func(
            unsafe { FuncRef::new(syscalls::___syscall340 as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    //    // Process
    import_object.add(
        "env".to_string(),
        "abort".to_string(),
        Import::Func(
            unsafe { FuncRef::new(process::em_abort as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_abort".to_string(),
        Import::Func(
            unsafe { FuncRef::new(process::_abort as _) },
            FuncSig {
                params: vec![],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "abortStackOverflow".to_string(),
        Import::Func(
            unsafe { FuncRef::new(process::abort_stack_overflow as _) },
            FuncSig {
                params: vec![],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_llvm_trap".to_string(),
        Import::Func(
            unsafe { FuncRef::new(process::_llvm_trap as _) },
            FuncSig {
                params: vec![],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_fork".to_string(),
        Import::Func(
            unsafe { FuncRef::new(process::_fork as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_exit".to_string(),
        Import::Func(
            unsafe { FuncRef::new(process::_exit as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_system".to_string(),
        Import::Func(
            unsafe { FuncRef::new(process::_system as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_popen".to_string(),
        Import::Func(
            unsafe { FuncRef::new(process::_popen as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    //    // Signal
    import_object.add(
        "env".to_string(),
        "_sigemptyset".to_string(),
        Import::Func(
            unsafe { FuncRef::new(signal::_sigemptyset as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_sigaddset".to_string(),
        Import::Func(
            unsafe { FuncRef::new(signal::_sigaddset as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_sigprocmask".to_string(),
        Import::Func(
            unsafe { FuncRef::new(signal::_sigprocmask as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_sigaction".to_string(),
        Import::Func(
            unsafe { FuncRef::new(signal::_sigaction as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_signal".to_string(),
        Import::Func(
            unsafe { FuncRef::new(signal::_signal as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    //    // Memory
    import_object.add(
        "env".to_string(),
        "abortOnCannotGrowMemory".to_string(),
        Import::Func(
            unsafe { FuncRef::new(memory::abort_on_cannot_grow_memory as _) },
            FuncSig {
                params: vec![],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_emscripten_memcpy_big".to_string(),
        Import::Func(
            unsafe { FuncRef::new(memory::_emscripten_memcpy_big as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "enlargeMemory".to_string(),
        Import::Func(
            unsafe { FuncRef::new(memory::enlarge_memory as _) },
            FuncSig {
                params: vec![],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "getTotalMemory".to_string(),
        Import::Func(
            unsafe { FuncRef::new(memory::get_total_memory as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___map_file".to_string(),
        Import::Func(
            unsafe { FuncRef::new(memory::___map_file as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    //    // Exception
    import_object.add(
        "env".to_string(),
        "___cxa_allocate_exception".to_string(),
        Import::Func(
            unsafe { FuncRef::new(exception::___cxa_allocate_exception as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___cxa_allocate_exception".to_string(),
        Import::Func(
            unsafe { FuncRef::new(exception::___cxa_throw as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32, Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___cxa_throw".to_string(),
        Import::Func(
            unsafe { FuncRef::new(exception::___cxa_throw as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32, Type::I32],
                returns: vec![],
            },
        ),
    );
    //    // NullFuncs
    import_object.add(
        "env".to_string(),
        "nullFunc_ii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_ii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_iii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_iii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_iiii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_iiii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_iiiii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_iiiii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_iiiiii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_iiiiii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_v".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_v as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_vi".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_vi as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_vii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_vii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_viii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_viii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_viiii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_viiii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_viiiii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_viiiii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "nullFunc_viiiiii".to_string(),
        Import::Func(
            unsafe { FuncRef::new(nullfunc::nullfunc_viiiiii as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );
    //    // Time
    import_object.add(
        "env".to_string(),
        "_gettimeofday".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_gettimeofday as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_clock_gettime".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_clock_gettime as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "___clock_gettime".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::___clock_gettime as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_clock".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_clock as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_difftime".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_difftime as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_asctime".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_asctime as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_asctime_r".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_asctime_r as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_localtime".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_localtime as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_time".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_time as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_strftime".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_strftime as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32, Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_localtime_r".to_string(),
        Import::Func(
            unsafe { FuncRef::new(time::_localtime_r as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_getpagesize".to_string(),
        Import::Func(
            unsafe { FuncRef::new(env::_getpagesize as _) },
            FuncSig {
                params: vec![],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_sysconf".to_string(),
        Import::Func(
            unsafe { FuncRef::new(env::_sysconf as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    //    // Math
    import_object.add(
        "env".to_string(),
        "_llvm_log10_f64".to_string(),
        Import::Func(
            unsafe { FuncRef::new(math::_llvm_log10_f64 as _) },
            FuncSig {
                params: vec![Type::F64],
                returns: vec![Type::F64],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "_llvm_log2_f64".to_string(),
        Import::Func(
            unsafe { FuncRef::new( math::_llvm_log2_f64 as _) },
            FuncSig {
                params: vec![Type::F64],
                returns: vec![Type::F64],
            },
        ),
    );
    import_object.add(
        "asm2wasm".to_string(),
        "f64-rem".to_string(),
        Import::Func(
            unsafe { FuncRef::new(math::f64_rem as _) },
            FuncSig {
                params: vec![Type::F64, Type::F64],
                returns: vec![Type::F64],
            },
        ),
    );
    //
    import_object.add(
        "env".to_string(),
        "__setjmp".to_string(),
        Import::Func(
            unsafe { FuncRef::new(jmp::__setjmp as _) },
            FuncSig {
                params: vec![Type::I32],
                returns: vec![Type::I32],
            },
        ),
    );
    import_object.add(
        "env".to_string(),
        "__longjmp".to_string(),
        Import::Func(
            unsafe { FuncRef::new(jmp::__longjmp as _) },
            FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![],
            },
        ),
    );

    mock_external!(import_object, _waitpid);
    mock_external!(import_object, _utimes);
    mock_external!(import_object, _usleep);
    // mock_external!(import_object, _time);
    // mock_external!(import_object, _sysconf);
    // mock_external!(import_object, _strftime);
    mock_external!(import_object, _sigsuspend);
    // mock_external!(import_object, _sigprocmask);
    // mock_external!(import_object, _sigemptyset);
    // mock_external!(import_object, _sigaddset);
    // mock_external!(import_object, _sigaction);
    mock_external!(import_object, _setitimer);
    mock_external!(import_object, _setgroups);
    mock_external!(import_object, _setgrent);
    mock_external!(import_object, _sem_wait);
    mock_external!(import_object, _sem_post);
    mock_external!(import_object, _sem_init);
    mock_external!(import_object, _sched_yield);
    mock_external!(import_object, _raise);
    mock_external!(import_object, _mktime);
    // mock_external!(import_object, _localtime_r);
    // mock_external!(import_object, _localtime);
    mock_external!(import_object, _llvm_stacksave);
    mock_external!(import_object, _llvm_stackrestore);
    mock_external!(import_object, _kill);
    mock_external!(import_object, _gmtime_r);
    // mock_external!(import_object, _gettimeofday);
    // mock_external!(import_object, _getpagesize);
    mock_external!(import_object, _getgrent);
    mock_external!(import_object, _getaddrinfo);
    // mock_external!(import_object, _fork);
    // mock_external!(import_object, _exit);
    mock_external!(import_object, _execve);
    mock_external!(import_object, _endgrent);
    // mock_external!(import_object, _clock_gettime);
    mock_external!(import_object, ___syscall97);
    mock_external!(import_object, ___syscall91);
    mock_external!(import_object, ___syscall85);
    mock_external!(import_object, ___syscall75);
    mock_external!(import_object, ___syscall66);
    // mock_external!(import_object, ___syscall64);
    // mock_external!(import_object, ___syscall63);
    // mock_external!(import_object, ___syscall60);
    // mock_external!(import_object, ___syscall54);
    // mock_external!(import_object, ___syscall39);
    mock_external!(import_object, ___syscall38);
    // mock_external!(import_object, ___syscall340);
    mock_external!(import_object, ___syscall334);
    mock_external!(import_object, ___syscall300);
    mock_external!(import_object, ___syscall295);
    mock_external!(import_object, ___syscall272);
    mock_external!(import_object, ___syscall268);
    // mock_external!(import_object, ___syscall221);
    mock_external!(import_object, ___syscall220);
    // mock_external!(import_object, ___syscall212);
    // mock_external!(import_object, ___syscall201);
    mock_external!(import_object, ___syscall199);
    // mock_external!(import_object, ___syscall197);
    mock_external!(import_object, ___syscall196);
    // mock_external!(import_object, ___syscall195);
    mock_external!(import_object, ___syscall194);
    mock_external!(import_object, ___syscall191);
    // mock_external!(import_object, ___syscall181);
    // mock_external!(import_object, ___syscall180);
    mock_external!(import_object, ___syscall168);
    // mock_external!(import_object, ___syscall146);
    // mock_external!(import_object, ___syscall145);
    // mock_external!(import_object, ___syscall142);
    mock_external!(import_object, ___syscall140);
    // mock_external!(import_object, ___syscall122);
    // mock_external!(import_object, ___syscall102);
    // mock_external!(import_object, ___syscall20);
    mock_external!(import_object, ___syscall15);
    mock_external!(import_object, ___syscall10);
    mock_external!(import_object, _dlopen);
    mock_external!(import_object, _dlclose);
    mock_external!(import_object, _dlsym);
    mock_external!(import_object, _dlerror);

    import_object
}