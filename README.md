# ajazz-rs
Rust library for interacting with Ajazz Stream Docks.

## Example
```rust
use ajazz_rs::{new_hidapi, Ajazz};

// Create instance of HidApi
let hid = new_hidapi();

// List devices and unsafely take first one
let (kind, serial) = Ajazz::list_devices(&hid).remove(0);

// Connect to the device
let mut device = Ajazz::connect(&hid, kind, &serial)
    .expect("Failed to connect");

// Print out some info from the device
println!(
    "Connected to '{}' with version '{}'",
    device.serial_number().unwrap(),
    device.firmware_version().unwrap()
);

// Set device brightness
device.set_brightness(35).unwrap();

// Use image-rs to load an image
let image = image::open("no-place-like-localhost.jpg").unwrap();

// Write it to the device
device.set_button_image(7, image).unwrap();

// Flush
if device.updated {
    device.flush().unwrap();
}
```
