use core::ffi::{c_int, c_long, c_ulong};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Sbiret {
    pub error: c_long,
    pub value: c_long,
}

unsafe extern "C" {
    pub fn my_sbi_ecall(
        ext: c_int,
        fid: c_int,
        arg0: c_ulong,
        arg1: c_ulong,
        arg2: c_ulong,
        arg3: c_ulong,
        arg4: c_ulong,
        arg5: c_ulong,
    ) -> Sbiret;
}

#[derive(Debug)]
pub enum SbiError {
    Unknown,
    SbiErrFailed,
    SbiErrNotSupported,
    SbiErrInvalidParam,
    SbiErrDenied,
    SbiErrInvalidAddress,
    SbiErrAlreadyAvailable,
    SbiErrAlreadyStarted,
    SbiErrAlreadyStopped,
    SbiErrNoShmem,
    SbiErrInvalidState,
    SbiErrBadRange,
    SbiErrTimeout,
    SbiErrIo,
    SbiErrDeniedLocked,
}

impl TryFrom<i64> for SbiError {
    type Error = ();

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        use SbiError::*;
        match -value {
            1 => Ok(SbiErrFailed),
            2 => Ok(SbiErrNotSupported),
            3 => Ok(SbiErrInvalidParam),
            4 => Ok(SbiErrDenied),
            5 => Ok(SbiErrInvalidAddress),
            6 => Ok(SbiErrAlreadyAvailable),
            7 => Ok(SbiErrAlreadyStarted),
            8 => Ok(SbiErrAlreadyStopped),
            9 => Ok(SbiErrNoShmem),
            10 => Ok(SbiErrInvalidState),
            11 => Ok(SbiErrBadRange),
            12 => Ok(SbiErrTimeout),
            13 => Ok(SbiErrIo),
            14 => Ok(SbiErrDeniedLocked),
            _ => Err(()),
        }
    }
}

pub fn rust_sbi_ecall(
    ext: c_int,
    fid: c_int,
    arg0: c_ulong,
    arg1: c_ulong,
    arg2: c_ulong,
    arg3: c_ulong,
    arg4: c_ulong,
    arg5: c_ulong,
) -> Result<i64, SbiError> {
    let ret = unsafe { my_sbi_ecall(ext, fid, arg0, arg1, arg2, arg3, arg4, arg5) };
    if ret.error != 0 {
        Err(ret.error.try_into().unwrap())
    } else {
        Ok(ret.value)
    }
}

pub fn get_spec_version() -> u64 {
    let ret = rust_sbi_ecall(0x10, 0, 0, 0, 0, 0, 0, 0);
    ret.unwrap() as u64
}

pub fn get_impl_id() -> u64 {
    let ret = rust_sbi_ecall(0x10, 1, 0, 0, 0, 0, 0, 0);
    ret.unwrap() as u64
}

pub fn get_impl_version() -> u64 {
    let ret = rust_sbi_ecall(0x10, 2, 0, 0, 0, 0, 0, 0);
    ret.unwrap() as u64
}

pub fn set_timer(time: u64) -> Result<i64, SbiError> {
    rust_sbi_ecall(0x54494D45, 0x0, time, 0x0, 0x0, 0x0, 0x0, 0x0)
}
