// SPDX-License-Identifier: GPL-2.0

//! Virtio.
//!
//! C header: [`include/linux/virtio.h`](../../../../include/linux/virtio.h)

use crate::interrupt::IrqAffinity;
use crate::{bindings, Opaque, c_types, error::from_kernel_result, types::PointerWrapper, Error, Result, };
use crate::error::{code::*,};
use alloc::{boxed::Box, slice::from_raw_parts, slice::from_raw_parts_mut};
use core::{cell::UnsafeCell, marker, mem, ptr, ops::Deref, pin::Pin};
use core::marker::{PhantomPinned, PhantomData};

/// Corresponds to the kernel's `struct virtio_config_ops
pub trait VirtioConfigOps {
    /// Context data associated with the gpio chip.
    ///
    /// It determines the type of the context data passed to each of the methods of the trait.
    type DataBase: PointerWrapper + Sync + Send;
    
    /// Context data associated with the gpio chip.
    ///
    /// It determines the type of the context data passed to each of the methods of the trait.
    type DataVq: PointerWrapper + Sync + Send;

    /// The methods to use to populate [`struct virtio_config_ops`]. This is typically populated with
    /// [`declare_virtio_config_ops`].
    const TO_USE: ToUseVirtioConfigOps;

    /// 
    fn get(
        _data: <Self::DataBase as PointerWrapper>::Borrowed<'_>,
        _offset: u32,
        buffer: &mut [u8],
    ) {
        Err(EINVAL)
    }

    /// 
    fn set(
        _data: <Self::DataBase as PointerWrapper>::Borrowed<'_>,
        _offset: u32,
        buffer: & [u8],
    ) {
        Err(EINVAL)
    }

    fn set_status(
        _data: <Self::DataBase as PointerWrapper>::Borrowed<'_>,
        status: c_types::c_uchar,
    ) {
        Err(EINVAL)        
    }

    fn reset(
        _data: <Self::DataBase as PointerWrapper>::Borrowed<'_>,
    ) {
        Err(EINVAL)            
    }

    fn find_vqs(
        _data: <Self::DataVq as PointerWrapper>::Borrowed<'_>,
        vqs_buffer: &mut [u8],
        callbacks_buffer: &mut [u8],
        names_buffer: &mut [u8],
        wait: bool,
        irq_affinity: IrqAffinity
    ) -> Result {
        Err(EINVAL)            
    }

    fn get_features(
        _data: <Self::DataBase as PointerWrapper>::Borrowed<'_>,
    ) -> Result<u64> {
        Err(EINVAL)        
    }
}

/// Represents which fields of [`struct vringh_config_ops`] should be populated with pointers.
///
/// This is typically populated with the [`declare_vringh_config_ops`] macro.
pub struct ToUseVirtioConfigOps {
    /// The `get` field of [`struct vringh_config_ops`].
    pub get: bool,

    /// The `set` field of [`struct vringh_config_ops`].
    pub set: bool,

    /// The `set_status` field of [`struct vringh_config_ops`].
    pub set_status: bool,

    /// The `reset` field of [`struct vringh_config_ops`].
    pub reset: bool,

    /// The `find_vqs` field of [`struct vringh_config_ops`].
    pub find_vqs: bool,

    /// The `get_features` field of [`struct vringh_config_ops`].
    pub get_features: bool,
}

/// A constant version where all values are set to `false`, that is, all supported fields will be
/// set to null pointers.
pub const USE_NONE_VIRTIOCONFIGOPS: ToUseVirtioConfigOps = ToUseVirtioConfigOps {
    get: false,
    set: false,
    set_status: false,
    reset: false,
    find_vqs: false,
    get_features: false,
};

/// Defines the [`VirtioConfigOps::TO_USE`] field based on a list of fields to be populated.
#[macro_export]
macro_rules! declare_virtio_config_ops {
    () => {
        const ToUseVirtioConfigOps: $crate::virtio::ToUseVirtioConfigOps = $crate::virtio::ToUseVirtioConfigOps;
    };
    ($($i:ident),+) => {
        #[allow(clippy::needless_update)]
        const TO_USE_VIRTIOCONFIGOPS: $crate::virtio::ToUseVirtioConfigOps =
            $crate::virtio::ToUseVirtioConfigOps {
                $($i: true),+ ,
                ..$crate::virtio::USE_NONE_VIRTIOCONFIGOPS
            };
    };
}

pub(crate) struct VirtioConfigOpsTable<D: DataBaseAdapter<T> + DataVqAdapter<T>, T: VirtioConfigOps + core::marker::Sync>(PhantomData<D>, PhantomData<T>);

impl<D: DataBaseAdapter<T> + DataVqAdapter<T>, T: VirtioConfigOps + core::marker::Sync> VirtioConfigOpsTable<D, T> {
    // todo                    
    unsafe extern "C" fn get_callback(
        vdev: *mut bindings::virtio_device,
        offset: c_types::c_uint,
        buf: *mut c_types::c_void,
        len: c_types::c_uint,
    ) {
        let ptr = D::base(vdev);
        let data = unsafe{T::DataBase::borrow(ptr)};
        
        // SAFETY: Slice is created from `data` and `max` arguments that are C's buffer
        // along with its size in bytes that are safe for this conversion.
        let buffer = unsafe{from_raw_parts_mut(buf, len)};
        T::get(data, offset, buffer);
    }

    unsafe extern "C" fn set_callback( 
        vdev: *mut bindings::virtio_device,
        offset: c_types::c_uint,
        buf: *const c_types::c_void,
        len: c_types::c_uint,
     ) {
        let ptr = D::base(vdev);
        let data = unsafe{T::DataBase::borrow(ptr)};
        
        // SAFETY: Slice is created from `data` and `max` arguments that are C's buffer
        // along with its size in bytes that are safe for this conversion.
        let buffer = unsafe{from_raw_parts(buf, len)};
        T::set(data, offset, buffer);
    }

    unsafe extern "C" fn set_status_callback(
        vdev: *mut bindings::virtio_device, 
        status: bindings::u8_,
    ) {
        let ptr = D::base(vdev);
        let data = unsafe{T::DataBase::borrow(ptr)};
        T::set_status(data, status as c_types::c_uchar);
    }

    unsafe extern "C" fn reset_callback( 
        vdev: *mut bindings::virtio_device 
    ) {
        let ptr = D::base(vdev);
        let data = unsafe{T::DataBase::borrow(ptr)};
        T::reset(data);
    }

    unsafe extern "C" fn find_vqs_callback(             
        arg1: *mut bindings::virtio_device,
        nvqs: c_types::c_uint,
        vqs: *mut *mut bindings::virtqueue,
        callbacks: *mut bindings::vq_callback_t,
        names: *const *const c_types::c_char,
        ctx: *const bindings::bool_,
        desc: *mut bindings::irq_affinity,
    ) -> c_types::c_int {
        from_kernel_result! {
            let ptr = D::vq(arg1);
            let data = unsafe{T::DataBase::borrow(ptr)};
            
            // SAFETY: Slice is created from `data` and `max` arguments that are C's buffer
            // along with its size in bytes that are safe for this conversion.
            let vqs_buffer = unsafe{from_raw_parts_mut(vqs, nvqs)};
            let callbacks_buffer = unsafe{from_raw_parts_mut(callbacks, nvqs)};
            let names_buffer = unsafe{from_raw_parts_mut(names, nvqs)};
            let irqa = IrqAffinity::from_ptr(desc);

            T::find_vqs(data, vqs_buffer, callbacks_buffer, names_buffer, ctx, irqa);
            Ok(0)
        }
    }

    unsafe extern "C" fn get_features_callback(             
        vdev: *mut bindings::virtio_device
    ) -> bindings::u64_ {
        from_kernel_result! {
            let ptr = D::base(vdev);
            let data = unsafe{T::DataBase::borrow(ptr)};
            let mut feature = T::get_features(data)?;
            Ok(feature as _)
        }
    }
    
    const VTABLE: bindings::virtio_config_ops = bindings::virtio_config_ops {
        get: if T::TO_USE.get {
            Some(Self::get_callback)
        } else {
            None
        },
        set: if T::TO_USE.set {
            Some(Self::set_callback)
        } else {
            None
        },
        generation : None,
        get_status : None,
        set_status : if T::TO_USE.set_status {
            Some(Self::set_status_callback)
        } else {
            None
        },
        reset : if T::TO_USE.reset {
            Some(Self::reset_callback)
        } else {
            None
        },
        find_vqs : if T::TO_USE.find_vqs {
            Some(Self::find_vqs_callback)
        } else {
            None
        },
        del_vqs : None,
        synchronize_cbs : None,
        get_features : if T::TO_USE.get_features {
            Some(Self::get_features_callback)
        } else {
            None
        },
        finalize_features : None,
        bus_name : None,
        set_vq_affinity : None,
        get_vq_affinity : None,
        get_shm_region : None,
    };

    /// Builds an instance of `struct virtio_config_ops`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the adapter is compatible with the way the device is registered.
    pub(crate) const unsafe fn build() -> &'static bindings::virtio_config_ops {
        &Self::VTABLE
    }
}

/// Trait for extracting file open arguments from kernel data structures.
///
/// This is meant to be implemented by registration managers.
pub trait DataBaseAdapter<T: Sync> {
    /// Converts untyped data stored in [`struct inode`] and [`struct file`] (when [`struct
    /// file_operations::open`] is called) into the given type. For example, for `miscdev`
    /// devices, a pointer to the registered [`struct miscdev`] is stored in [`struct
    /// file::private_data`].
    ///
    /// # Safety
    ///
    /// This function must be called only when [`struct file_operations::open`] is being called for
    /// a file that was registered by the implementer. The returned pointer must be valid and
    /// not-null.
    unsafe fn base(vdev: *mut bindings::virtio_device) -> *mut T;
}

/// Trait for extracting file open arguments from kernel data structures.
///
/// This is meant to be implemented by registration managers.
pub trait DataVqAdapter<T: Sync> {
    /// Converts untyped data stored in [`struct inode`] and [`struct file`] (when [`struct
    /// file_operations::open`] is called) into the given type. For example, for `miscdev`
    /// devices, a pointer to the registered [`struct miscdev`] is stored in [`struct
    /// file::private_data`].
    ///
    /// # Safety
    ///
    /// This function must be called only when [`struct file_operations::open`] is being called for
    /// a file that was registered by the implementer. The returned pointer must be valid and
    /// not-null.
    unsafe fn vq(vdev: *mut bindings::virtio_device) -> *mut T;
}

pub struct Registration<T: VirtioConfigOps> {
    vdev: *mut bindings::virtio_device,
    base: u64,
    len: u64,
    vq: Opaque<bindings::list_head>,
    _p: marker::PhantomData<T>,
    _pin: marker::PhantomPinned,
}

impl<T: VirtioConfigOps> Registration<T> {
    // /// Creates a [`Registration`] object for a character device.
    // ///
    // /// This does *not* register the device: see [`Self::register()`].
    // ///
    // /// This associated function is intended to be used when you need to avoid
    // /// a memory allocation, e.g. when the [`Registration`] is a member of
    // /// a bigger structure inside your [`crate::Module`] instance. If you
    // /// are going to pin the registration right away, call
    // /// [`Self::new_pinned()`] instead.
    // pub fn new() -> Self {
    //     Registration {
    //         vdev: bindings::virtio_device::default(),
    //         base: 0,
    //         len: 0,
    //         vq: Opaque::uninit(),
    //         _p: PhantomData,
    //         _pin: PhantomPinned,
    //     }
    // }

    // /// Creates a pinned [`Registration`] object for a character device.
    // ///
    // /// This does *not* register the device: see [`Self::register()`].
    // pub fn new_pinned(
    //     base: u64,
    //     len: u64,
    //     vq: *mut bindings::list_head,
    //     data_base: T::DataBase,
    //     data_vq: T::DataVq,
    // ) -> Result<Pin<Box<Self>>> {
    //     let mut reg = Pin::from(Box::try_new(Self::new())?);
    //     reg.as_mut().register(base, len, vq, data_base, data_vq)?;
    //     Ok(reg)
    // }

    // /// Registers a character device.
    // ///
    // /// You may call this once per device type, up to `N` times.
    // pub fn register(
    //     self: Pin<&mut Self>,
    //     base: u64,
    //     len: u64,
    //     vq: *mut bindings::list_head,
    //     data_base: T::DataBase,
    //     data_vq: T::DataVq,
    // ) -> Result {
    //     // // SAFETY: We must ensure that we never move out of `this`.
    //     // let this = unsafe { self.get_unchecked_mut() };

    //     // if base = ;
    //     // if this.registered {
    //     //     // Already registered.
    //     //     return Err(EINVAL);
    //     // }


    //     Ok(())
    // }
}

impl<T: VirtioConfigOps> Default for Registration<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: VirtioConfigOps> DataBaseAdapter<T::DataBase> for Registration<T> {
    unsafe fn base(
        vdev: *mut bindings::virtio_device,
    ) -> *mut T::DataBase {
        // SAFETY: The caller must guarantee that `vdev` is valid.        
        let parent = crate::container_of!(vdev, Self, vdev);

        // SAFETY: This function is only called while the misc device is still registered, so the
        // registration must be valid. Additionally, the type invariants guarantee that while the
        // miscdev is registered, `open_data` is initialised.
        unsafe { (*parent).base.as_ptr() }
    }
}

impl<T: VirtioConfigOps> DataVqAdapter<T::DataVq> for Registration<T> {
    unsafe fn vq(
        vdev: *mut bindings::virtio_device,
    ) -> *mut T::DataVq {
        // SAFETY: The caller must guarantee that `vdev` is valid.        
        let parent = crate::container_of!(vdev, Self, vdev);

        // SAFETY: This function is only called while the misc device is still registered, so the
        // registration must be valid. Additionally, the type invariants guarantee that while the
        // miscdev is registered, `open_data` is initialised.
        unsafe { (*parent).vq.as_ptr() }
    }
}

// SAFETY: `Registration` doesn't offer any methods or access to fields when shared between threads
// or CPUs, so it is safe to share it.
unsafe impl<T: VirtioConfigOps> Sync for Registration<T> {}

// SAFETY: Registration with and unregistration from the gpio subsystem can happen from any thread.
// Additionally, `T::Data` (which is dropped during unregistration) is `Send`, so it is ok to move
// `Registration` to different threads.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T: VirtioConfigOps> Send for Registration<T> {}

impl<T: VirtioConfigOps> Drop for Registration<T> {
    /// Removes the registration from the kernel if it has completed successfully before.
    fn drop(&mut self) {
        // if self.parent.is_some() {
            // // Get a pointer to the data stored in chip before destroying it.
            // // SAFETY: `gc` was during registration, which is guaranteed to have succeeded (because
            // // `parent` is `Some(_)`, so it remains valid.
            // let data_pointer = unsafe { bindings::gpiochip_get_data(self.gc.get()) };

            // // SAFETY: By the same argument above, `gc` is still valid.
            // unsafe { bindings::gpiochip_remove(self.gc.get()) };

            // // Free data as well.
            // // SAFETY: `data_pointer` was returned by `into_pointer` during registration.
            // unsafe { <T::Data as PointerWrapper>::from_pointer(data_pointer) };
        // }
    }
}


// code line----------------------------------------------------------------

/// Wraps the kernel's `struct virtqueue`.
///
/// # Invariants
///
/// The pointer `VirtQueue::ptr` is non-null and valid.
// pub struct VirtQueue {
//     ptr: *mut bindings::virtqueue,
// }

// impl VirtQueue {
//     /// Creates a new `VirtQueue` instance from a raw pointer.
//     ///
//     /// # Safety
//     ///
//     /// Callers must ensure that `ptr` is non-null and valid when the function is called, and that
//     /// it remains valid for the lifetime of the return [`VirtQueue`] instance.
//     pub unsafe fn new(        
//         index: c_types::c_uint,
//         num: c_types::c_uint,
//         vring_align: c_types::c_uint,
//         vdev: *mut virtio_device,
//         weak_barriers: bool_,
//         may_reduce_num: bool_,
//         ctx: bool_,
//         notify: ::core::option::Option<unsafe extern "C" fn(vq: *mut virtqueue) -> bool_>,
//         callback: ::core::option::Option<unsafe extern "C" fn(vq: *mut virtqueue)>,
//         name: *const c_types::c_char) -> Self {
//         // SAFETY: 
//         unsafe{Self { bindings::vring_create_virtqueue(index, num, vring_align, vdev, weak_barriers, 
//             may_reduce_num, ctx, notify, callback, name) }}
//     }

//     pub unsafe fn from_ptr(ptr: *mut bindings::virtqueue) -> Self {
//         // INVARIANT: The safety requirements of the function ensure the lifetime invariant.
//         Self { ptr }
//     }
// }

// impl Deref for VirtQueue {
//     fn deref(&mut self) {
//         // SAFETY: The type invariant guarantees that the [`VirtQueue`] is valid.
//         unsafe { bindings::vring_del_virtqueue(self.ptr) };
//     }
// }

// backup code

// /// Wraps the kernel's `struct virtio_device`.
// ///
// /// # Invariants
// ///
// /// The pointer `VirtioDevice::ptr` is non-null and valid.
// pub struct VirtioDevice {
//     dev: bindings::virtio_device,
// }

// impl VirtioDevice {
//     /// Creates a new `VirtioDevice` instance from a raw pointer.
//     ///
//     /// # Safety
//     ///
//     /// Callers must ensure that `ptr` is non-null and valid when the function is called, and that
//     /// it remains valid for the lifetime of the return [`VirtQueue`] instance.
//     pub unsafe fn new() -> Result<Self> {
//         let vdev = bindings::virtio_device::default();
//         // SAFETY: The vdev is valid.
//         unsafe{bindings::register_virtio_device(&mut vdev as *mut bindings::virtio_device); }
//         to_result
//         Self {vdev}
//     }
// }

// vring ops

// pub trait VringhConfigOps {
//     /// Context data associated with the gpio chip.
//     ///
//     /// It determines the type of the context data passed to each of the methods of the trait.
//     type DataRing: PointerWrapper + Sync + Send;

//     /// The methods to use to populate [`struct vringh_config_ops`]. This is typically populated with
//     /// [`declare_vringh_config_ops`].
//     const TO_USE: ToUseVringhConfigOps;

//     /// 
//     // todo
//     fn find_vrhs(
//         _data: <Self::Data as PointerWrapper>::Borrowed<'_>,
//         _offset: u32,
//     ) -> Result<LineDirection> {
//         Err(ENOTSUPP)
//     }

//     /// 
//     // todo
//     fn del_vrhs(
//         _data: <Self::Data as PointerWrapper>::Borrowed<'_>,
//         _offset: u32,
//     ) -> Result {
//         Err(EIO)
//     }
// }

// /// Represents which fields of [`struct vringh_config_ops`] should be populated with pointers.
// ///
// /// This is typically populated with the [`declare_vringh_config_ops`] macro.
// pub struct ToUseVringhConfigOps {
//     /// The `find_vrhs` field of [`struct vringh_config_ops`].
//     pub find_vrhs: bool,

//     /// The `del_vrhs` field of [`struct vringh_config_ops`].
//     pub del_vrhs: bool,
// }

// /// A constant version where all values are set to `false`, that is, all supported fields will be
// /// set to null pointers.
// pub const TO_USE_VRINGHCONFIGOPS: ToUseVringhConfigOps = ToUseVringhConfigOps {
//     find_vrhs: false,
//     del_vrhs: false,
// };

// /// Defines the [`VringhConfigOps::TO_USE`] field based on a list of fields to be populated.
// #[macro_export]
// macro_rules! declare_vringh_config_ops {
//     () => {
//         const ToUseVringhConfigOps: $crate::virtio::ToUseVringhConfigOps = $crate::virtio::ToUseVringhConfigOps;
//     };
//     ($($i:ident),+) => {
//         #[allow(clippy::needless_update)]
//         const TO_USE_VRINGHCONFIGOPS: $crate::virtio::ToUseVringhConfigOps =
//             $crate::virtio::ToUseVringhConfigOps {
//                 $($i: true),+ ,
//                 ..$crate::virtio::USE_NONE_VRINGHCONFIGOPS
//             };
//     };
// }

// pub(crate) struct VringhConfigOpsTable<R: DataBaseAdapter, T: VringhConfigOps>(PhantomData<A>, PhantomData<T>);

// impl<R: DataBaseAdapter<T::DataRing>, T: VringhConfigOps> VringhConfigOpsTable<A, T> {
//     // todo
//     unsafe extern "C" fn find_vrhs_callback(
//         vdev: *mut virtio_device,
//         nhvrs: c_types::c_uint,
//         vrhs: *mut *mut vringh,
//         callbacks: *mut vrh_callback_t,
//     ) -> c_types::c_int {
//         from_kernel_result! {
//             let ptr = V::vq(arg1);
//             let data = unsafe{T::DataBase::borrow(ptr)};
            
//             // SAFETY: Slice is created from `data` and `max` arguments that are C's buffer
//             // along with its size in bytes that are safe for this conversion.
//             let vqs_buffer = unsafe{from_raw_parts_mut(vqs, nvqs)};
//             let callbacks_buffer = unsafe{from_raw_parts_mut(callbacks, nvqs)};
//             let names_buffer = unsafe{from_raw_parts_mut(names, nvqs)};
//             let irqa = IrqAffinity::from_ptr(desc);

//             T::find_vqs(data, vqs_buffer, callbacks_buffer, names_buffer, ctx, irqa);
//             Ok(0)

//             let vdev_ref = ;
//             T::find_vrhs();
//             Ok(0)
//         }
//     }

//     unsafe extern "C" fn del_vrhs_callback( vdev: *mut virtio_device ) {
//         from_kernel_result! {
//             // todo
//             T::del_vrhs();
//             Ok(0)
//         }
//     }

//     const VTABLE: bindings::vringh_config_ops = bindings::vringh_config_ops {
//         find_vrhs: if T::TO_USE.find_vrhs {
//             Some(Self::find_vrhs_callback)
//         } else {
//             None
//         },
//         del_vrhs: if T::TO_USE.del_vrhs {
//             Some(Self::del_vrhs_callback)
//         } else {
//             None
//         },
//     };

//     /// Builds an instance of `struct vringh_config_ops`.
//     ///
//     /// # Safety
//     ///
//     /// The caller must ensure that the adapter is compatible with the way the device is registered.
//     pub(crate) const unsafe fn build() -> &'static bindings::vringh_config_ops {
//         &Self::VTABLE
//     }
// }

// /// Trait for extracting file open arguments from kernel data structures.
// ///
// /// This is meant to be implemented by registration managers.
// pub trait DataRingAdapter<T: Sync> {
//     /// Converts untyped data stored in [`struct inode`] and [`struct file`] (when [`struct
//     /// file_operations::open`] is called) into the given type. For example, for `miscdev`
//     /// devices, a pointer to the registered [`struct miscdev`] is stored in [`struct
//     /// file::private_data`].
//     ///
//     /// # Safety
//     ///
//     /// This function must be called only when [`struct file_operations::open`] is being called for
//     /// a file that was registered by the implementer. The returned pointer must be valid and
//     /// not-null.
//     unsafe fn ring(vdev: *mut bindings::virtio_device) -> *const T;
// }

