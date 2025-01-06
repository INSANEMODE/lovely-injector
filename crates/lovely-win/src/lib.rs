use std::env;
use std::ffi::c_void;
use std::panic;

use itertools::Itertools;
use lovely_core::log::*;
use lovely_core::sys::LuaState;
use lovely_core::Lovely;
use lovely_core::LOVELY_VERSION;

use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;

// Use Windows-specific crates only on Windows
#[cfg(target_os = "windows")]
use retour::static_detour;
#[cfg(target_os = "windows")]
use widestring::U16CString;
#[cfg(target_os = "windows")]
use windows::core::{s, w, PCWSTR};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HINSTANCE, HWND};
#[cfg(target_os = "windows")]
use windows::Win32::System::Console::{AllocConsole, SetConsoleTitleW};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MESSAGEBOX_STYLE};

static RUNTIME: OnceCell<Lovely> = OnceCell::new();

// Only define detours for Windows
#[cfg(target_os = "windows")]
static_detour! {
    pub static LuaLoadbufferx_Detour: unsafe extern "C" fn(*mut LuaState, *const u8, isize, *const u8, *const u8) -> u32;
}

static WIN_TITLE: Lazy<U16CString> =
    Lazy::new(|| U16CString::from_str(format!("Lovely {LOVELY_VERSION}")).unwrap());

unsafe extern "C" fn lua_loadbufferx_detour(
    state: *mut LuaState,
    buf_ptr: *const u8,
    size: isize,
    name_ptr: *const u8,
    mode_ptr: *const u8,
) -> u32 {
    let rt = RUNTIME.get_unchecked();
    rt.apply_buffer_patches(state, buf_ptr, size, name_ptr, mode_ptr)
}

#[no_mangle]
#[allow(non_snake_case)]
unsafe extern "system" fn DllMain(_: HINSTANCE, reason: u32, _: *const c_void) -> u8 {
    if reason != 1 {
        return 1;
    }

    panic::set_hook(Box::new(|x| unsafe {
        let message = format!("lovely-injector has crashed: \n{x}");
        error!("{message}");

        let message = U16CString::from_str(message);
        MessageBoxW(
            HWND(0),
            PCWSTR(message.unwrap().as_ptr()),
            PCWSTR(WIN_TITLE.as_ptr()),
            MESSAGEBOX_STYLE(0),
        );
    }));

    let args = env::args().collect_vec();

    if args.contains(&"--disable-mods".to_string()) || args.contains(&"-d".to_string()) {
        return 1;
    }

    if !args.contains(&"--disable-console".to_string()) {
        let _ = AllocConsole();
        SetConsoleTitleW(PCWSTR(WIN_TITLE.as_ptr())).expect("Failed to set console title.");
    }

    let dump_all = args.contains(&"--dump-all".to_string());

    // Initialize the lovely runtime.
    let rt = Lovely::init(
        &|a, b, c, d, e| LuaLoadbufferx_Detour.call(a, b, c, d, e),
        dump_all,
    );
    RUNTIME
        .set(rt)
        .unwrap_or_else(|_| panic!("Failed to instantiate runtime."));

    // Quick and easy hook injection. Load the lua51.dll module at runtime, determine the address of the luaL_loadbuffer fn, hook it.
    let handle = LoadLibraryW(w!("lua51.dll")).unwrap();
    let proc = GetProcAddress(handle, s!("luaL_loadbufferx")).unwrap();
    let fn_target = std::mem::transmute::<
        _,
        unsafe extern "C" fn(*mut c_void, *const u8, isize, *const u8, *const u8) -> u32,
    >(proc);

    LuaLoadbufferx_Detour
        .initialize(fn_target, |a, b, c, d, e| {
            lua_loadbufferx_detour(a, b, c, d, e)
        })
        .unwrap()
        .enable()
        .unwrap();

    1
}
