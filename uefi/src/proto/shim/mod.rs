// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shim lock protocol.

#![cfg(any(
    target_arch = "x86",
    target_arch = "x86_64",
    target_arch = "arm",
    target_arch = "aarch64"
))]

use crate::proto::unsafe_protocol;
use crate::result::Error;
use crate::{Result, Status, StatusExt};
use core::ffi::c_void;
use core::mem::MaybeUninit;

// The `PE_COFF_LOADER_IMAGE_CONTEXT` type. None of our methods need to inspect
// the fields of this struct, we just need to make sure it is the right size.
#[repr(C)]
struct Context {
    _image_address: u64,
    _image_size: u64,
    _entry_point: u64,
    _size_of_headers: usize,
    _image_type: u16,
    _number_of_sections: u16,
    _section_alignment: u32,
    _first_section: *const c_void,
    _reloc_dir: *const c_void,
    _sec_dir: *const c_void,
    _number_of_rva_and_sizes: u64,
    _pe_hdr: *const c_void,
}

const SHA1_DIGEST_SIZE: usize = 20;
const SHA256_DIGEST_SIZE: usize = 32;

/// Authenticode hashes of some UEFI application.
#[derive(Debug)]
pub struct Hashes {
    /// SHA256 Authenticode Digest
    pub sha256: [u8; SHA256_DIGEST_SIZE],
    /// SHA1 Authenticode Digest
    pub sha1: [u8; SHA1_DIGEST_SIZE],
}

// These macros set the correct calling convention for the Shim protocol methods.

#[cfg(target_arch = "x86")]
macro_rules! shim_function {
    (fn $args:tt -> $return_type:ty) => (extern "cdecl" fn $args -> $return_type)
}

#[cfg(target_arch = "x86_64")]
macro_rules! shim_function {
    (fn $args:tt -> $return_type:ty) => (extern "sysv64" fn $args -> $return_type)
}

#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
macro_rules! shim_function {
    (fn $args:tt -> $return_type:ty) => (extern "C" fn $args -> $return_type)
}

/// Shim Lock [`Protocol`].
///
/// This protocol is not part of the UEFI specification, but is
/// installed by the [Shim bootloader](https://github.com/rhboot/shim)
/// which is commonly used by Linux distributions to support UEFI
/// Secure Boot. Shim is built with an embedded certificate that is
/// used to validate another EFI application before running it. That
/// application may itself be a bootloader that needs to validate
/// another EFI application before running it, and the shim lock
/// protocol exists to support that.
///
/// [`Protocol`]: uefi::proto::Protocol
#[derive(Debug)]
#[repr(C)]
#[unsafe_protocol("605dab50-e046-4300-abb6-3dd810dd8b23")]
pub struct ShimLock {
    verify: shim_function! { fn(buffer: *const u8, size: u32) -> Status },
    hash: shim_function! {
        fn(
            buffer: *const u8,
            size: u32,
            context: *mut Context,
            sha256: *mut [u8; SHA256_DIGEST_SIZE],
            sha1: *mut [u8; SHA1_DIGEST_SIZE]
        ) -> Status
    },
    context: shim_function! { fn(buffer: *const u8, size: u32, context: *mut Context) -> Status },
}

impl ShimLock {
    /// Verify that an EFI application is signed by the certificate
    /// embedded in shim.
    ///
    /// The buffer's size must fit in a `u32`; if that condition is not
    /// met then a `BAD_BUFFER_SIZE` error will be returned and the shim
    /// lock protocol will not be called.
    pub fn verify(&self, buffer: &[u8]) -> Result {
        let size: u32 = buffer
            .len()
            .try_into()
            .map_err(|_| Error::from(Status::BAD_BUFFER_SIZE))?;
        (self.verify)(buffer.as_ptr(), size).to_result()
    }
    /// Compute the Authenticode Hash of the provided EFI application.
    ///
    /// The buffer's size must fit in a `u32`; if that condition is not
    /// met then a `BAD_BUFFER_SIZE` error will be returned and the shim
    /// lock protocol will not be called.
    pub fn hash(&self, buffer: &[u8], hashes: &mut Hashes) -> Result {
        let ptr: *const u8 = buffer.as_ptr();
        let size: u32 = buffer
            .len()
            .try_into()
            .map_err(|_| Error::from(Status::BAD_BUFFER_SIZE))?;

        let mut context = MaybeUninit::<Context>::uninit();
        (self.context)(ptr, size, context.as_mut_ptr()).to_result()?;
        (self.hash)(
            ptr,
            size,
            context.as_mut_ptr(),
            &mut hashes.sha256,
            &mut hashes.sha1,
        )
        .to_result()
    }
}
