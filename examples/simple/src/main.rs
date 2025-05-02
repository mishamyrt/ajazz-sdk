use std::sync::Arc;
use std::time::Duration;

use image::open;

use ajazz_sdk::{AjazzInput, list_devices, new_hidapi, Ajazz};
use ajazz_sdk::images::{convert_image_with_format, ImageRect};

fn main() {
    // Create instance of HidApi
    match new_hidapi() {
        Ok(hid) => {
            // Refresh device list
            for (kind, serial) in list_devices(&hid) {
                println!("{:?} {} {}", kind, serial, kind.product_id());

                // Connect to the device
                let device = Ajazz::connect(&hid, kind, &serial).expect("Failed to connect");
                // Print out some info from the device
                println!("Connected to '{}' with version '{}'", device.serial_number().unwrap(), device.firmware_version().unwrap());

                device.set_brightness(50).unwrap();
                device.clear_all_button_images().unwrap();
                // Use image-rs to load an image
                let image = open("frame.jpg").unwrap();

                println!("Key count: {}", kind.key_count());
                // Write it to the device
                for i in 0..kind.key_count() as u8 {
                    device.set_button_image(i, image.clone()).unwrap();
                }

                if let Some(format) = device.kind().lcd_strip_size() {
                    let scaled_image = image.clone().resize_to_fill(format.0 as u32, format.1 as u32, image::imageops::FilterType::Nearest);
                    let format = device.kind().key_image_format();
                    let converted_image = convert_image_with_format(format, scaled_image).unwrap();
                    let _ = device.write_lcd_fill(&converted_image);
                }

                let small = match device.kind().lcd_strip_size() {
                    Some((w, h)) => {
                        let min = w.min(h) as u32;
                        let scaled_image = image.clone().resize_to_fill(min, min, image::imageops::Nearest);
                        Some(ImageRect::from_image(scaled_image).unwrap())
                    }
                    None => None,
                };

                // Flush
                device.flush().unwrap();

                let device = Arc::new(device);
                {
                    let reader = device.get_reader();

                    'infinite: loop {
                        let updates = match reader.read(Some(Duration::from_secs_f64(100.0))) {
                            Ok(updates) => updates,
                            Err(_) => break,
                        };
                        for update in updates {
                            match update {
                                AjazzInput::ButtonStateChange(button_states) => {
                                    for (i, state) in button_states.iter().enumerate() {
                                        if *state {
                                            println!("Button {} down", i);
                                        } else {
                                            println!("Button {} up", i);
                                        }
                                    }
                                }
                                AjazzInput::EncoderTwist(dial_values) => {
                                    for (i, ticks) in dial_values.iter().enumerate() {
                                        if *ticks != 0 {
                                            println!("Dial {} twisted by {}", i, ticks);
                                        }
                                    }
                                }
                                AjazzInput::EncoderStateChange(dial_states) => {
                                    for (i, state) in dial_states.iter().enumerate() {
                                        if *state {
                                            println!("Dial {} down", i);
                                        } else {
                                            println!("Dial {} up", i);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    drop(reader);
                }

                device.shutdown().ok();
            }
        }
        Err(e) => eprintln!("Failed to create HidApi instance: {}", e),
    }
}
