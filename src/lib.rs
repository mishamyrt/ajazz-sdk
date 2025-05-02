//! Ajazz library
//!
//! Library for interacting with Ajazz devices through [hidapi](https://crates.io/crates/hidapi).

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::iter::zip;
use std::str::Utf8Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::images::{convert_image, ImageRect};
use hidapi::{HidApi, HidDevice, HidError, HidResult};
use image::{DynamicImage, ImageError};

use crate::info::{is_vendor_familiar, Kind};
use crate::util::{ajazz03_read_input, mirabox_extend_packet, ajazz153_to_elgato_input, elgato_to_ajazz153, extract_str, inverse_key_index, get_feature_report, read_button_states, read_data, write_data};

/// Various information about Ajazz devices
pub mod info;
/// Utility functions for working with Ajazz devices
pub mod util;
/// Image processing functions
pub mod images;

/// Async Ajazz
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod asynchronous;
#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub use asynchronous::AsyncAjazz;

/// Creates an instance of the HidApi
///
/// Can be used if you don't want to link hidapi crate into your project
pub fn new_hidapi() -> HidResult<HidApi> {
    HidApi::new()
}

/// Actually refreshes the device list
pub fn refresh_device_list(hidapi: &mut HidApi) -> HidResult<()> {
    hidapi.refresh_devices()
}

/// Returns a list of devices as (Kind, Serial Number) that could be found using HidApi.
///
/// **WARNING:** To refresh the list, use [refresh_device_list]
pub fn list_devices(hidapi: &HidApi) -> Vec<(Kind, String)> {
    hidapi
        .device_list()
        .filter_map(|d| {
            if !is_vendor_familiar(&d.vendor_id()) {
                return None;
            }

            if let Some(serial) = d.serial_number() {
                Some((Kind::from_vid_pid(d.vendor_id(), d.product_id())?, serial.to_string()))
            } else {
                None
            }
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

/// Type of input that the device produced
#[derive(Clone, Debug)]
pub enum AjazzInput {
    /// No data was passed from the device
    NoData,

    /// Button was pressed
    ButtonStateChange(Vec<bool>),

    /// Encoder/Knob was pressed
    EncoderStateChange(Vec<bool>),

    /// Encoder/Knob was twisted/turned
    EncoderTwist(Vec<i8>),
}

impl AjazzInput {
    /// Checks if there's data received or not
    pub fn is_empty(&self) -> bool {
        matches!(self, AjazzInput::NoData)
    }
}

/// Interface for an Ajazz device
pub struct Ajazz {
    /// Kind of the device
    kind: Kind,
    /// Connected HIDDevice
    device: HidDevice,
    /// Temporarily cache the image before sending it to the device
    image_cache: RwLock<Vec<ImageCache>>,
    /// Device needs to be initialized
    initialized: AtomicBool,
}

struct ImageCache {
    key: u8,
    image_data: Vec<u8>,
}

/// Static functions of the struct
impl Ajazz {
    /// Attempts to connect to the device
    pub fn connect(hidapi: &HidApi, kind: Kind, serial: &str) -> Result<Ajazz, AjazzError> {
        let device = hidapi.open_serial(kind.vendor_id(), kind.product_id(), serial)?;

        Ok(Ajazz {
            kind,
            device,
            image_cache: RwLock::new(vec![]),
            initialized: false.into(),
        })
    }
}

/// Instance methods of the struct
impl Ajazz {
    /// Returns kind of the Ajazz device
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Returns manufacturer string of the device
    pub fn manufacturer(&self) -> Result<String, AjazzError> {
        Ok(self.device.get_manufacturer_string()?.unwrap_or_else(|| "Unknown".to_string()))
    }

    /// Returns product string of the device
    pub fn product(&self) -> Result<String, AjazzError> {
        Ok(self.device.get_product_string()?.unwrap_or_else(|| "Unknown".to_string()))
    }

    /// Returns serial number of the device
    pub fn serial_number(&self) -> Result<String, AjazzError> {
        let serial = self.device.get_serial_number_string()?;
        match serial {
            Some(serial) => {
                if serial.is_empty() {
                    Ok("Unknown".to_string())
                } else {
                    Ok(serial)
                }
            }
            None => Ok("Unknown".to_string()),
        }
    }

    /// Returns firmware version of the device
    pub fn firmware_version(&self) -> Result<String, AjazzError> {
        let bytes = get_feature_report(&self.device, 0x01, 20)?;
        Ok(extract_str(&bytes[0..])?)
    }

    /// Initializes the device
    fn initialize(&self) -> Result<(), AjazzError> {
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }

        self.initialized.store(true, Ordering::Release);

        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x44, 0x49, 0x53];
        mirabox_extend_packet(&self.kind, &mut buf);
        write_data(&self.device, buf.as_slice())?;

        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x4c, 0x49, 0x47, 0x00, 0x00, 0x00, 0x00];
        mirabox_extend_packet(&self.kind, &mut buf);
        write_data(&self.device, buf.as_slice())?;

        Ok(())
    }

    /// Reads all possible input from Ajazz device
    pub fn read_input(&self, timeout: Option<Duration>) -> Result<AjazzInput, AjazzError> {
        self.initialize()?;
        match &self.kind {
            kind if kind.is_ajazz_v1() => {
                let data = read_data(&self.device, 512, timeout)?;

                if data[0] == 0 {
                    return Ok(AjazzInput::NoData);
                }

                let mut states = vec![0x01];
                states.extend(vec![0u8; (self.kind.key_count() + 1) as usize]);

                if data[9] != 0 {
                    let key = match self.kind {
                        Kind::Akp815 => inverse_key_index(&self.kind, data[9] - 1),
                        Kind::Akp153 | Kind::Akp153E | Kind::Akp153R => ajazz153_to_elgato_input(&self.kind, data[9] - 1),
                        _ => unimplemented!(),
                    };

                    states[(key + 1) as usize] = 0x1u8;
                }

                Ok(AjazzInput::ButtonStateChange(read_button_states(&self.kind, &states)))
            }

            kind if kind.is_ajazz_v2() => {
                let data = read_data(&self.device, 512, timeout)?;

                if data[0] == 0 {
                    return Ok(AjazzInput::NoData);
                }

                // Devices not returning a state for the input
                ajazz03_read_input(&self.kind, data[9], 0x01)
            }

            _ => Err(AjazzError::UnsupportedOperation),
        }
    }

    /// Resets the device
    pub fn reset(&self) -> Result<(), AjazzError> {
        self.initialize()?;
        self.set_brightness(100)?;
        self.clear_all_button_images()?;
        Ok(())
    }

    /// Sets brightness of the device, value range is 0 - 100
    pub fn set_brightness(&self, percent: u8) -> Result<(), AjazzError> {
        self.initialize()?;
        let percent = percent.clamp(0, 100);

        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x4c, 0x49, 0x47, 0x00, 0x00, percent];

        mirabox_extend_packet(&self.kind, &mut buf);

        write_data(&self.device, buf.as_slice())?;

        Ok(())
    }

    fn send_image(&self, key: u8, image_data: &[u8]) -> Result<(), AjazzError> {
        if key >= self.kind.key_count() {
            return Err(AjazzError::InvalidKeyIndex);
        }

        let key = match self.kind {
            Kind::Akp153 | Kind::Akp153E | Kind::Akp153R => elgato_to_ajazz153(&self.kind, key),
            Kind::Akp815 => inverse_key_index(&self.kind, key),
            _ => key,
        };

        let mut buf = vec![
            0x00,
            0x43,
            0x52,
            0x54,
            0x00,
            0x00,
            0x42,
            0x41,
            0x54,
            0x00,
            0x00,
            (image_data.len() >> 8) as u8,
            image_data.len() as u8,
            key + 1,
        ];

        mirabox_extend_packet(&self.kind, &mut buf);

        write_data(&self.device, buf.as_slice())?;

        self.write_image_data_reports(image_data, WriteImageParameters::for_key(self.kind, image_data.len()), |page_number, this_length, last_package| {
            vec![0x00]
        })?;
        Ok(())
    }

    /// Writes image data to Ajazz device, changes must be flushed with `.flush()` before
    /// they will appear on the device!
    pub fn write_image(&self, key: u8, image_data: &[u8]) -> Result<(), AjazzError> {
        // Key count is 9 for AKP03x, but only the first 6 (0-5) have screens, so don't output anything for keys 6, 7, 8
        if matches!(self.kind, Kind::Akp03 | Kind::Akp03E | Kind::Akp03R | Kind::Akp03RRev2) && key >= 6 {
            return Ok(());
        }

        let cache_entry = ImageCache {
            key,
            image_data: image_data.to_vec(), // Convert &[u8] to Vec<u8>
        };

        self.image_cache.write()?.push(cache_entry);

        Ok(())
    }

    /// Sets button's image to blank, changes must be flushed with `.flush()` before
    /// they will appear on the device!
    pub fn clear_button_image(&self, key: u8) -> Result<(), AjazzError> {
        self.initialize()?;

        let key = match self.kind {
            Kind::Akp815 => inverse_key_index(&self.kind, key),
            Kind::Akp153 | Kind::Akp153E | Kind::Akp153R => elgato_to_ajazz153(&self.kind, key),
            _ => key,
        };

        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x43, 0x4c, 0x45, 0x00, 0x00, 0x00, if key == 0xff { 0xff } else { key + 1 }];

        mirabox_extend_packet(&self.kind, &mut buf);

        write_data(&self.device, buf.as_slice())?;

        Ok(())
    }

    /// Sets blank images to every button, changes must be flushed with `.flush()` before
    /// they will appear on the device!
    pub fn clear_all_button_images(&self) -> Result<(), AjazzError> {
        self.initialize()?;
        match self.kind {
            kind if kind.is_ajazz_v1() => self.clear_button_image(0xff),
            kind if kind.is_ajazz_v2() => {
                self.clear_button_image(0xFF)?;

                // Mirabox "v2" requires STP to commit clearing the screen
                let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x53, 0x54, 0x50];
                mirabox_extend_packet(&self.kind, &mut buf);
                write_data(&self.device, buf.as_slice())?;

                Ok(())
            }
            _ => {
                for i in 0..self.kind.key_count() {
                    self.clear_button_image(i)?
                }
                Ok(())
            }
        }
    }

    /// Sets specified button's image, changes must be flushed with `.flush()` before
    /// they will appear on the device!
    pub fn set_button_image(&self, key: u8, image: DynamicImage) -> Result<(), AjazzError> {
        self.initialize()?;
        let image_data = convert_image(self.kind, image)?;
        self.write_image(key, &image_data)?;
        Ok(())
    }

    /// Set logo image
    pub fn set_logo_image(&self, image: DynamicImage) -> Result<(), AjazzError> {
        self.initialize()?;

        if self.kind.lcd_strip_size().is_none() {
            return Err(AjazzError::UnsupportedOperation);
        }
        // 854 * 480 * 3
        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x4c, 0x4f, 0x47, 0x00, 0x12, 0xc3, 0xc0, 0x01];

        mirabox_extend_packet(&self.kind, &mut buf);

        write_data(&self.device, buf.as_slice())?;

        let mut image_buffer: DynamicImage = DynamicImage::new_rgb8(854, 480);

        let ratio = 854.0 / 480.0;

        let mode = "cover";

        match mode {
            "contain" => {
                let (image_w, image_h) = (image.width(), image.height());
                let image_ratio = image_w as f32 / image_h as f32;

                let (ws, hs) = if image_ratio > ratio {
                    (854, (854.0 / image_ratio) as u32)
                } else {
                    ((480.0 * image_ratio) as u32, 480)
                };

                let resized_image = image.resize(ws, hs, image::imageops::FilterType::Nearest);
                image::imageops::overlay(
                    &mut image_buffer,
                    &resized_image,
                    ((854 - resized_image.width()) / 2) as i64,
                    ((480 - resized_image.height()) / 2) as i64,
                );
            }
            "cover" => {
                let resized_image = image.resize_to_fill(854, 480, image::imageops::FilterType::Nearest);
                image::imageops::overlay(
                    &mut image_buffer,
                    &resized_image,
                    ((854 - resized_image.width()) / 2) as i64,
                    ((480 - resized_image.height()) / 2) as i64,
                );
            }
            _ => {
                let (image_w, image_h) = (image.width(), image.height());
                let image_ratio = image_w as f32 / image_h as f32;

                let (ws, hs) = if image_ratio > ratio {
                    ((480.0 * image_ratio) as u32, 480)
                } else {
                    (854, (854.0 / image_ratio) as u32)
                };

                let resized_image = image.resize(ws, hs, image::imageops::FilterType::Nearest);
                image::imageops::overlay(
                    &mut image_buffer,
                    &resized_image,
                    ((854 - resized_image.width()) / 2) as i64,
                    ((480 - resized_image.height()) / 2) as i64,
                );
            }
        }

        let mut image_data = image_buffer.rotate90().fliph().flipv().into_rgb8().to_vec();
        for x in (0..image_data.len()).step_by(3) {
            (image_data[x], image_data[x + 2]) = (image_data[x + 2], image_data[x])
        }

        let image_report_length = match self.kind {
            kind if kind.is_ajazz_v1() => 513,
            kind if kind.is_ajazz_v2() => 1025,
            _ => 1024,
        };

        let image_report_header_length = 1;

        let image_report_payload_length = image_report_length - image_report_header_length;

        let mut page_number = 0;
        let mut bytes_remaining = image_data.len();

        while bytes_remaining > 0 {
            let this_length = bytes_remaining.min(image_report_payload_length);
            let bytes_sent = page_number * image_report_payload_length;

            // Create buffer with Report ID as first byte
            let mut buf: Vec<u8> = vec![0x00];

            // Selecting header based on device
            buf.extend(&image_data[bytes_sent..bytes_sent + this_length]);

            // Adding padding
            buf.extend(vec![0u8; image_report_length - buf.len()]);

            write_data(&self.device, &buf)?;

            bytes_remaining -= this_length;
            page_number += 1;
        }

        Ok(())
    }

    /// Sleeps the device
    pub fn sleep(&self) -> Result<(), AjazzError> {
        self.initialize()?;

        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x48, 0x41, 0x4e];

        mirabox_extend_packet(&self.kind, &mut buf);

        write_data(&self.device, buf.as_slice())?;

        Ok(())
    }

    /// Make periodic events to the device, to keep it alive
    pub fn keep_alive(&self) -> Result<(), AjazzError> {
        self.initialize()?;

        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x43, 0x4F, 0x4E, 0x4E, 0x45, 0x43, 0x54];
        mirabox_extend_packet(&self.kind, &mut buf);
        write_data(&self.device, buf.as_slice())?;
        Ok(())
    }

    /// Shutdown the device
    pub fn shutdown(&self) -> Result<(), AjazzError> {
        self.initialize()?;

        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x43, 0x4c, 0x45, 0x00, 0x00, 0x44, 0x43];
        mirabox_extend_packet(&self.kind, &mut buf);
        write_data(&self.device, buf.as_slice())?;

        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x48, 0x41, 0x4E];
        mirabox_extend_packet(&self.kind, &mut buf);
        write_data(&self.device, buf.as_slice())?;

        Ok(())
    }

    /// Flushes the button's image to the device
    pub fn flush(&self) -> Result<(), AjazzError> {
        self.initialize()?;

        if self.image_cache.write()?.is_empty() {
            return Ok(());
        }

        for image in self.image_cache.read()?.iter() {
            self.send_image(image.key, &image.image_data)?;
        }

        let mut buf = vec![0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x53, 0x54, 0x50];

        mirabox_extend_packet(&self.kind, &mut buf);

        write_data(&self.device, buf.as_slice())?;

        self.image_cache.write()?.clear();

        Ok(())
    }

    /// Returns button state reader for this device
    pub fn get_reader(self: &Arc<Self>) -> Arc<DeviceStateReader> {
        #[allow(clippy::arc_with_non_send_sync)]
        Arc::new(DeviceStateReader {
            device: self.clone(),
            states: Mutex::new(DeviceState {
                buttons: vec![false; self.kind.key_count() as usize],
                encoders: vec![false; self.kind.encoder_count() as usize],
            }),
        })
    }

    fn write_image_data_reports<T>(&self, image_data: &[u8], parameters: WriteImageParameters, header_fn: T) -> Result<(), AjazzError>
    where
        T: Fn(usize, usize, bool) -> Vec<u8>,
    {
        let image_report_length = parameters.image_report_length;
        let image_report_payload_length = parameters.image_report_payload_length;

        let mut page_number = 0;
        let mut bytes_remaining = image_data.len();

        while bytes_remaining > 0 {
            let this_length = bytes_remaining.min(image_report_payload_length);
            let bytes_sent = page_number * image_report_payload_length;

            // Selecting header based on device
            let mut buf: Vec<u8> = header_fn(page_number, this_length, this_length == bytes_remaining);

            buf.extend(&image_data[bytes_sent..bytes_sent + this_length]);

            // Adding padding
            buf.extend(vec![0u8; image_report_length - buf.len()]);

            write_data(&self.device, &buf)?;

            bytes_remaining -= this_length;
            page_number += 1;
        }

        Ok(())
    }
}

#[derive(Clone, Copy)]
struct WriteImageParameters {
    pub image_report_length: usize,
    pub image_report_payload_length: usize,
}

impl WriteImageParameters {
    pub fn for_key(kind: Kind, image_data_len: usize) -> Self {
        let image_report_length = match kind {
            kind if kind.is_ajazz_v1() => 513,
            kind if kind.is_ajazz_v2() => 1025,
            _ => 1024,
        };

        let image_report_header_length = 1;

        let image_report_payload_length = image_report_length - image_report_header_length;

        Self {
            image_report_length,
            image_report_payload_length,
        }
    }
}

/// Errors that can occur while working with Ajazz devices
#[derive(Debug)]
pub enum AjazzError {
    /// HidApi error
    HidError(HidError),

    /// Failed to convert bytes into string
    Utf8Error(Utf8Error),

    /// Failed to encode image
    ImageError(ImageError),

    #[cfg(feature = "async")]
    #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
    /// Tokio join error
    JoinError(tokio::task::JoinError),

    /// Reader mutex was poisoned
    PoisonError,

    /// Key index is invalid
    InvalidKeyIndex,

    /// Unrecognized Product ID
    UnrecognizedPID,

    /// The device doesn't support doing that
    UnsupportedOperation,

    /// Device sent unexpected data
    BadData,
}

impl Display for AjazzError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error for AjazzError {}

impl From<HidError> for AjazzError {
    fn from(e: HidError) -> Self {
        Self::HidError(e)
    }
}

impl From<Utf8Error> for AjazzError {
    fn from(e: Utf8Error) -> Self {
        Self::Utf8Error(e)
    }
}

impl From<ImageError> for AjazzError {
    fn from(e: ImageError) -> Self {
        Self::ImageError(e)
    }
}

#[cfg(feature = "async")]
impl From<tokio::task::JoinError> for AjazzError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::JoinError(e)
    }
}

impl<T> From<PoisonError<T>> for AjazzError {
    fn from(_value: PoisonError<T>) -> Self {
        Self::PoisonError
    }
}

/// Tells what changed in button states
#[derive(Copy, Clone, Debug, Hash)]
pub enum DeviceStateUpdate {
    /// Button got pressed down
    ButtonDown(u8),

    /// Button got released
    ButtonUp(u8),

    /// Encoder got pressed down
    EncoderDown(u8),

    /// Encoder was released from being pressed down
    EncoderUp(u8),

    /// Encoder was twisted
    EncoderTwist(u8, i8),
}

#[derive(Default)]
struct DeviceState {
    pub buttons: Vec<bool>,
    pub encoders: Vec<bool>,
}

/// Button reader that keeps state of the Ajazz and returns events instead of full states
pub struct DeviceStateReader {
    device: Arc<Ajazz>,
    states: Mutex<DeviceState>,
}

impl DeviceStateReader {
    /// Reads states and returns updates
    pub fn read(&self, timeout: Option<Duration>) -> Result<Vec<DeviceStateUpdate>, AjazzError> {
        let input = self.device.read_input(timeout)?;
        let mut my_states = self.states.lock()?;

        let mut updates = vec![];

        match input {
            AjazzInput::ButtonStateChange(buttons) => {
                for (index, (their, mine)) in zip(buttons.iter(), my_states.buttons.iter()).enumerate() {
                    if *their && !*mine {
                        updates.push(DeviceStateUpdate::ButtonDown(index as u8));
                    } else if *their && *mine {
                        updates.push(DeviceStateUpdate::ButtonUp(index as u8));
                    }
                }

                my_states.buttons = buttons;
            }

            AjazzInput::EncoderStateChange(encoders) => {
                for (index, (their, mine)) in zip(encoders.iter(), my_states.encoders.iter()).enumerate() {
                    if *their {
                        updates.push(DeviceStateUpdate::EncoderDown(index as u8));
                        updates.push(DeviceStateUpdate::EncoderUp(index as u8));
                    }
                }

                my_states.encoders = encoders;
            }

            AjazzInput::EncoderTwist(twist) => {
                for (index, change) in twist.iter().enumerate() {
                    if *change != 0 {
                        updates.push(DeviceStateUpdate::EncoderTwist(index as u8, *change));
                    }
                }
            }

            _ => {}
        }

        drop(my_states);

        Ok(updates)
    }
}
