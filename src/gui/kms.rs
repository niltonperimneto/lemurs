use drm::buffer::Buffer;
use drm::control::dumbbuffer::DumbBuffer;
use drm::control::{connector, crtc, encoder, framebuffer, Device as ControlDevice};
use nix::sys::mman;
use std::fs::File;
use std::num::NonZeroUsize;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::{AsFd, BorrowedFd};

// Robust DRM Implementation

#[derive(Debug)]
#[allow(dead_code)]
pub enum KmsError {
    OpenDevice(std::io::Error),
    ModeSet(std::io::Error),
    NoConnector,
    NoEncoder,
    NoCrtc,
    DumbBufferCreate(std::io::Error),
    DumbBufferMap(std::io::Error),
    Framebuffer(std::io::Error),
    Mmap(nix::Error),
}

impl From<nix::Error> for KmsError {
    fn from(e: nix::Error) -> Self {
        KmsError::ModeSet(std::io::Error::from_raw_os_error(e as i32))
    }
}

pub struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl AsRawFd for Card {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.0.as_raw_fd()
    }
}

impl drm::Device for Card {}
impl ControlDevice for Card {}

#[allow(dead_code)]
pub struct KmsBackend {
    card: Card,
    crtc: crtc::Handle,
    connector: connector::Handle,
    _buffer: DumbBuffer,
    _framebuffer: framebuffer::Handle,
    mapping: *mut u8,
    size: usize,
    width: u32,
    height: u32,
    saved_crtc: Option<drm::control::crtc::Info>,
}

impl Drop for KmsBackend {
    fn drop(&mut self) {
        // Restore CRTC state if possible?
        // For a login manager, we generally don't care about restoring the previous state
        // as we are handing over to a display server or another TTY, but it's good practice.
        // Doing proper atomic restore is complex, so we just clean up memory.

        if !self.mapping.is_null() {
            unsafe {
                let _ = mman::munmap(
                    std::ptr::NonNull::new(self.mapping as *mut std::ffi::c_void).unwrap(),
                    self.size,
                );
            }
        }
    }
}

impl KmsBackend {
    /// Attempts to open the first available DRM card
    pub fn open_card() -> Result<Card, KmsError> {
        let mut file = None;
        for i in 0..10 {
            let path = format!("/dev/dri/card{}", i);
            let result = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path);

            if let Ok(f) = result {
                file = Some(f);
                break;
            }
        }
        let file = file.ok_or_else(|| {
            KmsError::OpenDevice(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No DRM card found (checked card0..card9)",
            ))
        })?;
        Ok(Card(file))
    }

    pub fn new() -> Result<Self, KmsError> {
        let card = Self::open_card()?;

        let res = card.resource_handles().map_err(KmsError::ModeSet)?;

        let mut connector_info = None;
        let mut connector_handle = None;

        for &con in res.connectors() {
            if let Ok(info) = card.get_connector(con, true) {
                if info.state() == connector::State::Connected {
                    connector_info = Some(info);
                    connector_handle = Some(con);
                    break;
                }
            }
        }

        let con_info = connector_info.ok_or(KmsError::NoConnector)?;
        let con_handle = connector_handle.unwrap();

        let mode = con_info
            .modes()
            .iter()
            .find(|m| {
                m.mode_type()
                    .contains(drm::control::ModeTypeFlags::PREFERRED)
            })
            .or_else(|| con_info.modes().first())
            .ok_or(KmsError::ModeSet(std::io::Error::from_raw_os_error(
                libc::EINVAL,
            )))?;

        let mode = *mode;

        let (_enc_handle, crtc_handle) = Self::find_encoder_crtc(&card, &con_info, &res)?;

        let (width, height) = mode.size();
        eprintln!("Creating dumb buffer with size {}x{}", width, height);

        let db = card
            .create_dumb_buffer(
                (width as u32, height as u32),
                drm::buffer::DrmFourcc::Xrgb8888,
                32,
            )
            .map_err(|e| {
                eprintln!("Failed to create dumb buffer: {:?}", e);
                KmsError::DumbBufferCreate(e)
            })?;

        eprintln!("Dumb buffer created. Handle: {:?}", db.handle());

        eprintln!("Adding framebuffer...");
        let fb = card
            .add_framebuffer(&db, 24, 32)
            .map_err(KmsError::Framebuffer)?;

        eprintln!("Mapping dumb buffer (handle: {:?})...", db.handle());
        let mut map_args = drm_sys::drm_mode_map_dumb {
            handle: db.handle().into(),
            pad: 0,
            offset: 0,
        };
        const DRM_IOCTL_MODE_MAP_DUMB: libc::c_ulong = 0xC01064B3;
        let ret = unsafe {
            libc::ioctl(
                card.as_fd().as_raw_fd(),
                DRM_IOCTL_MODE_MAP_DUMB,
                &mut map_args,
            )
        };
        eprintln!("IOCTL result: {}, Offset: 0x{:x}", ret, map_args.offset);

        if ret < 0 {
            let err = std::io::Error::last_os_error();
            eprintln!("IOCTL failed: {:?}", err);
            return Err(KmsError::DumbBufferMap(err));
        }

        let pitch = db.pitch();
        let byte_size = (height as u32 * pitch) as usize;
        eprintln!(
            "Mmapping {} bytes at offset 0x{:x}",
            byte_size, map_args.offset
        );

        let mapping = unsafe {
            mman::mmap(
                None,
                NonZeroUsize::new(byte_size).unwrap(),
                mman::ProtFlags::PROT_READ | mman::ProtFlags::PROT_WRITE,
                mman::MapFlags::MAP_SHARED,
                &card,
                map_args.offset as i64,
            )
            .map_err(|e| {
                eprintln!("Mmap failed: {:?}", e);
                KmsError::Mmap(e)
            })?
        };
        eprintln!("Mmap successful: {:?}", mapping);

        let saved_crtc = card.get_crtc(crtc_handle).ok();

        eprintln!("Setting CRTC: {:?}", crtc_handle);
        card.set_crtc(crtc_handle, Some(fb), (0, 0), &[con_handle], Some(mode))
            .map_err(KmsError::ModeSet)?;

        Ok(Self {
            card,
            crtc: crtc_handle,
            connector: con_handle,
            _buffer: db,
            _framebuffer: fb,
            mapping: mapping.as_ptr() as *mut u8,
            size: byte_size,
            width: width as u32,
            height: height as u32,
            saved_crtc,
        })
    }

    /// robustly finds an encoder and CRTC that work with the connector
    fn find_encoder_crtc(
        card: &Card,
        con_info: &connector::Info,
        res: &drm::control::ResourceHandles,
    ) -> Result<(encoder::Handle, crtc::Handle), KmsError> {
        if let Some(enc_handle) = con_info.current_encoder() {
            if let Ok(enc_info) = card.get_encoder(enc_handle) {
                if let Some(crtc_handle) = enc_info.crtc() {
                    return Ok((enc_handle, crtc_handle));
                }
            }
        }

        for &enc_handle in con_info.encoders() {
            let _enc_info = card.get_encoder(enc_handle).map_err(KmsError::ModeSet)?;

            if let Some(&crtc_handle) = res.crtcs().iter().next() {
                // Get the CRTC info
                let crtc = match card.get_crtc(crtc_handle) {
                    Ok(c) => c,
                    Err(e) => {
                        // Using eprintln for now as warn! is not available without log crate
                        eprintln!("Failed to get CRTC info: {}", e);
                        continue;
                    }
                };

                if crtc.mode().is_some() {
                    return Ok((enc_handle, crtc_handle));
                }
            }
        }

        Err(KmsError::NoCrtc)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn fill_screen(&mut self, color: u32) {
        let pixel_count = (self.width * self.height) as usize;
        let buffer =
            unsafe { std::slice::from_raw_parts_mut(self.mapping as *mut u32, pixel_count) };
        buffer.fill(color);
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y * self.width + x) as usize;
        let buffer = unsafe {
            std::slice::from_raw_parts_mut(
                self.mapping as *mut u32,
                (self.width * self.height) as usize,
            )
        };
        buffer[offset] = color;
    }

    pub fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: u32) {
        let start_x = x.min(self.width);
        let start_y = y.min(self.height);
        let end_x = (x + width).min(self.width);
        let end_y = (y + height).min(self.height);

        if start_x >= end_x || start_y >= end_y {
            return;
        }

        let rect_width = (end_x - start_x) as usize;
        let buffer_len = (self.width * self.height) as usize;
        let buffer =
            unsafe { std::slice::from_raw_parts_mut(self.mapping as *mut u32, buffer_len) };

        for row_y in start_y..end_y {
            let row_start = (row_y * self.width + start_x) as usize;
            let row_slice = &mut buffer[row_start..row_start + rect_width];
            row_slice.fill(color);
        }
    }

    pub fn flush(&mut self) {
        let mut dirty = drm_sys::drm_mode_fb_dirty_cmd {
            fb_id: self._framebuffer.into(),
            flags: 0,
            color: 0,
            num_clips: 0,
            clips_ptr: 0,
        };
        const DRM_IOCTL_MODE_DIRTYFB: libc::c_ulong = 0xC01864B1;
        unsafe {
            libc::ioctl(
                self.card.as_fd().as_raw_fd(),
                DRM_IOCTL_MODE_DIRTYFB,
                &mut dirty,
            );
        }
    }
}
