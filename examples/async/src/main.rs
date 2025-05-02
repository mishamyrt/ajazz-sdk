use std::time::Duration;
use image::open;

use ajazz_rs::{DeviceStateUpdate, list_devices, new_hidapi, AsyncAjazz};
use ajazz_rs::images::{convert_image_with_format, ImageRect};
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    // Create instance of HidApi
    match new_hidapi() {
        Ok(hid) => {
            // Refresh device list
            for (kind, serial) in list_devices(&hid) {
                println!("{:?} {} {}", kind, serial, kind.product_id());

                // Connect to the device
                let device = AsyncAjazz::connect(&hid, kind, &serial).expect("Failed to connect");
                // Print out some info from the device
                println!("Connected to '{}' with version '{}'", device.serial_number().await.unwrap(), device.firmware_version().await.unwrap());

                device.set_brightness(50).await.unwrap();
                device.clear_all_button_images().await.unwrap();

                // Use image-rs to load an image
                let image = open("frame.jpg").unwrap();
                let alternative = image.grayscale().brighten(-50);

                // device.set_logo_image(image.clone()).await.unwrap();

                println!("Key count: {}", kind.key_count());
                // Write it to the device
                for i in 0..kind.key_count() as u8 {
                    device.set_button_image(i, image.clone()).await.unwrap();
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
                device.flush().await.unwrap();

                // Start new task to animate the button images
                let device_clone = device.clone();
                tokio::spawn(async move {
                    let mut index = 0;
                    let mut previous = 0;

                    loop {
                        device_clone.set_button_image(index, image.clone()).await.unwrap();
                        device_clone.set_button_image(previous, alternative.clone()).await.unwrap();

                        device_clone.flush().await.unwrap();

                        sleep(Duration::from_secs_f32(0.5)).await;

                        previous = index;

                        index += 1;
                        if index >= kind.key_count() as u8 {
                            index = 0;
                        }
                    }
                });

                let reader = device.get_reader();

                'infinite: loop {
                    let updates = match reader.read(100.0).await {
                        Ok(updates) => updates,
                        Err(_) => break,
                    };
                    for update in updates {
                        match update {
                            DeviceStateUpdate::ButtonDown(key) => {
                                println!("Button {} down", key);
                            }
                            DeviceStateUpdate::ButtonUp(key) => {
                                println!("Button {} up", key);
                            }
                            DeviceStateUpdate::EncoderTwist(dial, ticks) => {
                                println!("Dial {} twisted by {}", dial, ticks);
                            }
                            DeviceStateUpdate::EncoderDown(dial) => {
                                println!("Dial {} down", dial);
                            }
                            DeviceStateUpdate::EncoderUp(dial) => {
                                println!("Dial {} up", dial);
                            }
                        }
                    }
                }

                drop(reader);

                device.shutdown().await.ok();
            }
        }
        Err(e) => eprintln!("Failed to create HidApi instance: {}", e),
    }
}
