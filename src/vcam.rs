// SPDX-License-Identifier: GPL-3.0-or-later
// A second virtual camera, next to OBS: Unity Capture (github.com/schellingb/UnityCapture, filter under the MIT licence), a free DirectShow
// camera called "Unity Video Capture" that Discord, Zoom, browsers and OBS can pick. The program does not contain it: it is downloaded
// (File, Downloads) and registered once with its Install.bat. Here only the way pictures are handed to it is written, the same as the Unity
// plugin of that project does it: named Windows objects (a mutex, two events and a block of shared memory) that the filter makes when a
// program opens the camera. Until a program does, there is nobody to send to and nothing is sent.

use image::RgbaImage;

/// What happened to a picture that was sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sent {
    /// No program has the camera open: nothing was sent.
    Nobody,
    /// The picture was handed over.
    Delivered,
    /// The picture was handed over, but the program was still busy with the one before.
    Skipped,
    /// The picture is bigger than the camera takes (4K).
    TooBig,
}

/// The name of the camera in the other programs.
pub const CAMERA_NAME: &str = "Unity Video Capture";

/// Is the camera registered in Windows (Install.bat of Unity Capture was run).
pub fn registered() -> bool {
    #[cfg(windows)]
    {
        // the filter is a COM class: its key is in the registry
        std::process::Command::new("reg")
            .args(["query", r"HKLM\SOFTWARE\Classes\CLSID\{5C2CD55C-92AD-4999-8666-912BD3E70010}"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
mod sys {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Memory::{FILE_MAP_WRITE, MapViewOfFile, OpenFileMappingA, UnmapViewOfFile};
    use windows_sys::Win32::System::Threading::{EVENT_MODIFY_STATE, INFINITE, OpenEventA, OpenMutexW, ReleaseMutex, SetEvent, WaitForSingleObject};

    /// SYNCHRONIZE access: a mutex can be waited for.
    const SYNCHRONIZE: u32 = 0x0010_0000;
    /// The part of the shared block before the picture: the most it takes, then the size and the way the picture is made.
    const HEADER: usize = 32;

    pub struct Sender {
        cap: u8,
        mutex: HANDLE,
        want: HANDLE,
        sent: HANDLE,
        file: HANDLE,
        view: *mut u8,
    }

    // the handles are only used from the thread that owns the sender
    unsafe impl Send for Sender {}

    fn name(base: &str, cap: u8) -> Vec<u8> {
        // "UnityCapture_Mutx" + the number of the camera as a character ('0' + n), nothing for camera 0
        let mut v = base.as_bytes().to_vec();
        if cap > 0 {
            v.push(b'0' + cap);
        }
        v.push(0);
        v
    }

    /// The same name as an array of 16 bit characters (the mutex is opened by one).
    fn wname(base: &str, cap: u8) -> Vec<u16> {
        let mut v: Vec<u16> = base.encode_utf16().collect();
        if cap > 0 {
            v.push((b'0' + cap) as u16);
        }
        v.push(0);
        v
    }

    impl Sender {
        pub fn new(cap: u8) -> Sender {
            Sender { cap: cap.min(74), mutex: 0 as HANDLE, want: 0 as HANDLE, sent: 0 as HANDLE, file: 0 as HANDLE, view: std::ptr::null_mut() }
        }

        /// Open what the filter made. False while no program has the camera open.
        fn open(&mut self) -> bool {
            if !self.view.is_null() {
                return true;
            }
            unsafe {
                if self.mutex as usize == 0 {
                    self.mutex = OpenMutexW(SYNCHRONIZE, 0, wname("UnityCapture_Mutx", self.cap).as_ptr());
                    if self.mutex as usize == 0 {
                        return false;
                    }
                }
                WaitForSingleObject(self.mutex, INFINITE);
                let ok = (|| {
                    if self.want as usize == 0 {
                        self.want = OpenEventA(EVENT_MODIFY_STATE, 0, name("UnityCapture_Want", self.cap).as_ptr());
                        if self.want as usize == 0 {
                            return false;
                        }
                    }
                    if self.sent as usize == 0 {
                        self.sent = OpenEventA(EVENT_MODIFY_STATE, 0, name("UnityCapture_Sent", self.cap).as_ptr());
                        if self.sent as usize == 0 {
                            return false;
                        }
                    }
                    if self.file as usize == 0 {
                        self.file = OpenFileMappingA(FILE_MAP_WRITE, 0, name("UnityCapture_Data", self.cap).as_ptr());
                        if self.file as usize == 0 {
                            return false;
                        }
                    }
                    let v = MapViewOfFile(self.file, FILE_MAP_WRITE, 0, 0, 0);
                    self.view = v.Value as *mut u8;
                    !self.view.is_null()
                })();
                ReleaseMutex(self.mutex);
                ok
            }
        }

        /// Hand one picture (RGBA, top row first unless `upside_down`) to the camera.
        pub fn send(&mut self, img: &RgbaImage, upside_down: bool) -> Sent {
            if !self.open() {
                return Sent::Nobody;
            }
            let (w, h) = (img.width() as usize, img.height() as usize);
            let size = w * h * 4;
            unsafe {
                let max = std::ptr::read_unaligned(self.view as *const u32) as usize;
                if max < size {
                    return Sent::TooBig;
                }
                WaitForSingleObject(self.mutex, INFINITE);
                let head = self.view as *mut i32;
                // width, height, stride (in pixels), format 0 (8 bit RGBA), resize mode 1 (fit to the asked size), mirror mode 0, time out 100
                std::ptr::write_unaligned(head.add(1), w as i32);
                std::ptr::write_unaligned(head.add(2), h as i32);
                std::ptr::write_unaligned(head.add(3), w as i32);
                std::ptr::write_unaligned(head.add(4), 0);
                std::ptr::write_unaligned(head.add(5), 1);
                std::ptr::write_unaligned(head.add(6), 0);
                std::ptr::write_unaligned(head.add(7), 100);
                let dst = self.view.add(HEADER);
                let src = img.as_raw();
                if upside_down {
                    for y in 0..h {
                        std::ptr::copy_nonoverlapping(src.as_ptr().add(y * w * 4), dst.add((h - 1 - y) * w * 4), w * 4);
                    }
                } else {
                    std::ptr::copy_nonoverlapping(src.as_ptr(), dst, size);
                }
                ReleaseMutex(self.mutex);
                SetEvent(self.sent);
                if WaitForSingleObject(self.want, 0) == WAIT_OBJECT_0 { Sent::Delivered } else { Sent::Skipped }
            }
        }
    }

    impl Drop for Sender {
        fn drop(&mut self) {
            unsafe {
                if !self.view.is_null() {
                    UnmapViewOfFile(windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS { Value: self.view as *mut c_void });
                }
                for h in [self.mutex, self.want, self.sent, self.file] {
                    if h as usize != 0 {
                        CloseHandle(h);
                    }
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use super::*;
    pub struct Sender;
    impl Sender {
        pub fn new(_cap: u8) -> Sender {
            Sender
        }
        pub fn send(&mut self, _img: &RgbaImage, _upside_down: bool) -> Sent {
            Sent::Nobody
        }
    }
}

pub use sys::Sender;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_nobody_listening_nothing_is_sent_and_nothing_goes_wrong() {
        let mut s = Sender::new(7);
        let img = RgbaImage::from_pixel(64, 48, image::Rgba([10, 20, 30, 255]));
        assert_eq!(s.send(&img, false), Sent::Nobody);
        assert_eq!(s.send(&img, true), Sent::Nobody);
    }

    /// The filter's side, made here: the objects it makes when a program opens the camera, so the sender can be tried without the driver.
    #[cfg(windows)]
    #[test]
    fn a_picture_arrives_in_the_shared_memory_the_way_the_filter_reads_it() {
        use windows_sys::Win32::System::Memory::{CreateFileMappingA, FILE_MAP_WRITE, MapViewOfFile, PAGE_READWRITE};
        use windows_sys::Win32::System::Threading::{CreateEventA, CreateMutexW};
        let cap = 5u8; // a camera number nobody else uses
        let n = |b: &str| {
            let mut v = b.as_bytes().to_vec();
            v.push(b'0' + cap);
            v.push(0);
            v
        };
        let total = 32 + 3840 * 2160 * 4 * 2;
        unsafe {
            let wide: Vec<u16> = format!("UnityCapture_Mutx{}", (b'0' + cap) as char).encode_utf16().chain(std::iter::once(0)).collect();
            let _m = CreateMutexW(std::ptr::null(), 0, wide.as_ptr());
            let _w = CreateEventA(std::ptr::null(), 0, 0, n("UnityCapture_Want").as_ptr());
            let _s = CreateEventA(std::ptr::null(), 0, 0, n("UnityCapture_Sent").as_ptr());
            let f = CreateFileMappingA(-1isize as _, std::ptr::null(), PAGE_READWRITE, 0, total as u32, n("UnityCapture_Data").as_ptr());
            assert!(f as usize != 0);
            let view = MapViewOfFile(f, FILE_MAP_WRITE, 0, 0, 0).Value as *mut u8;
            std::ptr::write_unaligned(view as *mut u32, (3840 * 2160 * 4 * 2) as u32);
            let mut s = Sender::new(cap);
            let mut img = RgbaImage::new(4, 2);
            for (i, p) in img.pixels_mut().enumerate() {
                *p = image::Rgba([i as u8, 100, 200, 255]);
            }
            let r = s.send(&img, false);
            assert!(matches!(r, Sent::Skipped | Sent::Delivered), "{r:?}");
            let head = view as *const i32;
            assert_eq!((std::ptr::read_unaligned(head.add(1)), std::ptr::read_unaligned(head.add(2)), std::ptr::read_unaligned(head.add(3))), (4, 2, 4));
            assert_eq!(std::ptr::read_unaligned(head.add(4)), 0, "8 bit RGBA");
            assert_eq!(*view.add(32), 0);
            assert_eq!(*view.add(32 + 4 * 4 + 4 * 1 - 4), 4, "the second row starts with pixel 4");
            // upside down: the rows come in the other order
            s.send(&img, true);
            assert_eq!(*view.add(32), 4, "the first row sent is the last one of the picture");
        }
    }
}
