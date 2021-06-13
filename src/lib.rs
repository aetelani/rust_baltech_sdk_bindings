// (c) 2021 Anssi Etelaeniemi
#![feature(min_specialization)]
#![allow(
    nonstandard_style,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_must_use
)]

use core::fmt;
use std::ffi::{CStr, CString};
use std::fmt::{Debug, Formatter};
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_uchar, c_uint};
use std::ptr::null_mut;
use std::str::FromStr;

use log::error;
use log_derive::{logfn, logfn_inputs};
include!(concat!(env!("OUT_DIR"), "/api.rs"));
include!(concat!(env!("OUT_DIR"), "/brp_wrapper.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    // Test creating context and using the logger as mutex to init just once.
    fn init_context() {
        let init_run_first_time = env_logger::builder().is_test(true).try_init().is_ok();
    }

    fn select_exec_and_close(context: brp_protocol) -> BrpResult<()> {
        let mut select_params: BrpVhlSelectParams = BrpVhlSelectParams::default();
        select_params.0 = brp_CardFamilies {
            Iso14443A: true,
            ..brp_CardFamilies::default()
        };

        if vhl_select(context, select_params).is_ok()
            && vhl_get_serial_number(context).is_ok()
            && vhl_get_atr(context).is_ok()
            && desfire_select_application(context, 0).is_ok()
        {
            let _ = desfire_exec_cmd(context, Default::default());
        };
        close_session(context)
    }

    #[test]
    fn test_open_reader() {
        init_context();
        // Tests works only when reader is on port COM3
        const PORT: &str = "ttyS3";
        const PARITY_N: c_char = 78;
        const BAUDRATE: c_uint = 115200;
        const FIRMWARE_STABLE: &'static str = "1019 IDE STD   1.43.03 02/02/10 34008187";

        let params = ContextParams {
            0: PORT,
            1: BAUDRATE,
            2: PARITY_N,
        };
        let ctx = create_context(params).unwrap();

        // BUG open_session returns OK even parameters are wrong. ctx::opened true by default
        assert!(open_session(ctx).is_ok());

        let firmware_version_result = get_firmware_version(ctx).unwrap();

        assert!(firmware_version_result.eq(FIRMWARE_STABLE));

        select_exec_and_close(ctx).ok();
    }
}

#[allow(dead_code)]
#[derive(thiserror::Error, Debug)]
pub enum BrpError {
    #[error("brp_errcode:{0}")]
    ErrorCode(brp_errcode),
    #[error("Failed to create context: {0}")]
    FailedToCreateContext(ContextParams),
}

pub type BrpResult<T> = Result<T, BrpError>;

trait TryOk<T>
where
    T: Debug,
{
    fn try_ok(self, ok_value: T) -> BrpResult<T>;
    fn try_ok_or(self, ok_value: T, or_value: BrpError) -> BrpResult<T>;
}

trait TryDefault {
    fn try_default(self) -> BrpResult<()>;
    fn try_default_or(self, or_value: BrpError) -> BrpResult<()>;
}

impl TryDefault for brp_errcode {
    fn try_default(self) -> BrpResult<()> {
        self.try_default_or(BrpError::ErrorCode(self))
    }

    fn try_default_or(self, or_value: BrpError) -> BrpResult<()> {
        self.try_ok_or((), or_value)
    }
}

impl<T> TryOk<T> for brp_errcode
where
    T: Debug,
{
    fn try_ok(self, ok_value: T) -> BrpResult<T> {
        self.try_ok_or(ok_value, BrpError::ErrorCode(self))
    }

    //#[logfn(Trace)]
    fn try_ok_or(self, ok_value: T, or_value: BrpError) -> BrpResult<T> {
        if self == BRP_OK {
            Ok(ok_value)
        } else {
            Err(or_value)
        }
    }
}

#[logfn(Debug)]
fn get_firmware_version(context: brp_protocol) -> BrpResult<String> {
    unsafe {
        let mut info_ptr = MaybeUninit::<*mut c_char>::uninit();
        let error_code = brp_Sys_GetInfo(context, info_ptr.as_mut_ptr(), null_mut());
        let version_str = CStr::from_ptr(info_ptr.assume_init()).to_str().unwrap();
        let version = String::from_str(version_str).unwrap();
        error_code.try_ok(version)
    }
}

#[derive(Debug)]
pub struct ContextParams(&'static str, c_uint, c_char);

impl fmt::Display for ContextParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.0, self.1, self.2.to_string())
    }
}

#[logfn(Debug)]
#[logfn_inputs(Debug)]
fn create_context(params: ContextParams) -> BrpResult<brp_protocol> {
    let port = CString::new(params.0).unwrap();

    unsafe {
        let ctx = MaybeUninit::<brp_protocol>::new(brp_create());
        let con = MaybeUninit::<brp_protocol>::new(brp_create_rs232(
            port.as_ptr() as *mut _,
            params.1,
            params.2,
        ));
        if ctx.as_ptr().is_null() || con.as_ptr().is_null() {
            Err(BrpError::FailedToCreateContext(params))
        } else {
            let result = brp_set_io(ctx.assume_init(), con.assume_init());
            result.try_ok(ctx.assume_init())
        }
    }
}

#[logfn(Debug)]
fn destroy_context(ctx: brp_protocol) -> BrpResult<()> {
    unsafe { brp_destroy(ctx) }.try_default()
}

#[logfn(Debug)]
fn open_session(context: brp_protocol) -> BrpResult<()> {
    let error_code = unsafe { brp_open(context) };

    error_code.try_default()
}

#[logfn(Debug)]
fn close_session(context: brp_protocol) -> BrpResult<()> {
    #[logfn(Trace)]
    fn flush_buffer(ctx: brp_protocol) -> BrpResult<()> {
        unsafe { brp_flush(ctx) }.try_default()
    }

    #[logfn(Trace)]
    fn close_buffer(ctx: brp_protocol) -> BrpResult<()> {
        unsafe { brp_close(ctx) }.try_default()
    }

    let _ = flush_buffer(context);
    let _ = close_buffer(context);
    destroy_context(context)
}
struct Buf<T>(MaybeUninit<T>);

impl<T> Default for Buf<T> {
    default fn default() -> Self {
        Self(MaybeUninit::<T>::uninit())
    }
}

impl Default for Buf<brp_mempool> {
    fn default() -> Self {
        Self(MaybeUninit::<brp_mempool>::new(null_mut()))
    }
}

impl Buf<brp_mempool> {
    fn free_mem_pool(self) {
        unsafe { brp_mempool_free(self.0.assume_init() as *mut _) }
    }
}

impl<T> Debug for Buf<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Buf({:?})", self.0)
    }
}

#[derive(Default, Debug)]
struct DesfireAuthenticateParams(
    brp_Desfire_Authenticate_SecureMessaging,
    ::std::os::raw::c_uint,
    ::std::os::raw::c_uint,
    bool,
    brp_Desfire_Authenticate_KeyDivMode,
    bool,
    Buf<brp_buf>,
    size_t,
    ::std::os::raw::c_uint,
);

#[logfn(Debug)]
fn desfire_authenticate(context: brp_protocol, params: DesfireAuthenticateParams) -> BrpResult<()> {
    let error_code = unsafe {
        brp_Desfire_Authenticate(
            context,
            params.0,
            params.1,
            params.2,
            params.3,
            params.4,
            params.5,
            params.6 .0.assume_init(),
            params.7,
            params.8,
        )
    };
    error_code.try_default()
}

#[logfn(Debug)]
fn desfire_select_application(context: brp_protocol, app_id: c_uint) -> BrpResult<()> {
    let error_code = unsafe { brp_Desfire_SelectApplication(context, app_id) };

    error_code.try_default()
}

#[derive(Default, Debug)]
struct DesfireExecCommandParams(
    ::std::os::raw::c_uint,
    Buf<brp_buf>,
    size_t,
    Buf<brp_buf>,
    size_t,
    brp_Desfire_ExecCommand_CryptoMode,
    ::std::os::raw::c_uint,
    Buf<brp_buf>,
    Buf<size_t>,
    Buf<brp_mempool>,
);

#[logfn(Debug)]
#[logfn_inputs(Trace)]
fn desfire_exec_cmd(context: brp_protocol, mut params: DesfireExecCommandParams) -> BrpResult<()> {
    let error_code = unsafe {
        brp_Desfire_ExecCommand(
            context,
            params.0,
            params.1 .0.assume_init(),
            params.2,
            params.3 .0.assume_init(),
            params.4,
            params.5,
            params.6,
            params.7 .0.as_mut_ptr(),
            params.8 .0.as_mut_ptr(),
            null_mut(),
        )
    };

    error_code.try_default()
}

struct DesfireWriteDataParams(
    ::std::os::raw::c_uint,
    ::std::os::raw::c_uint,
    Buf<brp_buf>,
    size_t,
    brp_Desfire_WriteData_Mode,
);

#[logfn(Debug)]
fn desfire_write_data(context: brp_protocol, params: DesfireWriteDataParams) -> BrpResult<()> {
    let error_code = unsafe {
        brp_Desfire_WriteData(
            context,
            params.0,
            params.1,
            params.2 .0.assume_init(),
            params.3,
            params.4,
        )
    };
    error_code.try_default()
}

#[derive(Default, Debug)]
struct BrpVhlSelectParams(brp_CardFamilies, bool, bool, Buf<brp_CardType>);

impl Default for brp_CardFamilies {
    fn default() -> Self {
        brp_CardFamilies {
            LEGICPrime: false,
            BluetoothMce: false,
            Khz125Part2: false,
            Srix: false,
            Khz125Part1: false,
            Felica: false,
            IClass: false,
            IClassIso14B: false,
            Iso14443B: false,
            Iso15693: false,
            Iso14443A: false,
        }
    }
}

#[logfn(Debug)]
fn vhl_get_serial_number(context: brp_protocol) -> BrpResult<Option<Vec<c_uchar>>> {
    let mut buf = MaybeUninit::<brp_buf>::uninit();
    let mut size_of_serial = MaybeUninit::<size_t>::uninit();
    unsafe {
        let error_code = brp_VHL_GetSnr(
            context,
            buf.as_mut_ptr(),
            size_of_serial.as_mut_ptr(),
            null_mut(),
        );

        if size_of_serial.as_ptr().is_null() {
            error_code.try_ok(None)
        } else {
            let sn_slice = std::slice::from_raw_parts(
                buf.assume_init(),
                size_of_serial.assume_init() as usize,
            );
            error_code.try_ok(Some(sn_slice.to_vec()))
        }
    }
}

#[logfn(Debug)]
fn vhl_get_atr(context: brp_protocol) -> BrpResult<Option<Vec<c_uchar>>> {
    let mut buf: Buf<brp_buf> = Default::default();
    let mut size_of_serial: Buf<size_t> = Default::default();
    unsafe {
        let error_code = brp_VHL_GetATR(
            context,
            buf.0.as_mut_ptr(),
            size_of_serial.0.as_mut_ptr(),
            null_mut(),
        );

        if size_of_serial.0.as_ptr().is_null() {
            error_code.try_ok(None)
        } else {
            let atr_slice = std::slice::from_raw_parts(
                buf.0.assume_init(),
                size_of_serial.0.assume_init() as usize,
            );
            error_code.try_ok(Some(atr_slice.to_vec()))
        }
    }
}

#[logfn(Debug)]
fn vhl_select(context: brp_protocol, mut params: BrpVhlSelectParams) -> BrpResult<()> {
    unsafe { brp_VHL_Select(context, params.0, false, false, params.3 .0.as_mut_ptr()) }
        .try_default()
}
