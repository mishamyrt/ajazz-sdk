use std::{sync::Arc, thread::sleep};
use std::time::Duration;

use image::open;

use ajazz_sdk::{list_devices, new_hidapi, Ajazz, DeviceStateUpdate};

fn main() {
    let hid = match new_hidapi() {
        Ok(hid) => hid,
        Err(e) => {
            eprintln!("Failed to create HidApi instance: {}", e);
            return;
        }
    };

    let devices = list_devices(&hid);
    let (kind, serial) = devices.first().unwrap();

    let Ok(device) = Ajazz::connect(&hid, *kind, serial) else {
        println!("Failed to connect");
        return;
    };
    // Print out some info from the device
    println!(
        "Connected to '{}' with version '{}'",
        device.serial_number().unwrap(),
        device.firmware_version().unwrap()
    );

    let mut i = 0;
    loop {
        let image_name = {
            if i % 2 == 0 {
                "bg1.jpg"
            } else if i % 3 == 1 {
                "bg.jpg"
            } else {
                "bg2.jpg"
            }
        };
        println!("Setting logo image {}", image_name);
        let bg = image::open(image_name).unwrap();
        device.set_logo_image(bg).unwrap();
        println!("Logo image set");
        sleep(Duration::from_millis(1000));
        i += 1;
    }

    // match new_hidapi() {
    //     Ok(hid) => {
    //         // Refresh device list
    //         let device: Option<Ajazz> = {

    //             None
    //         };

    // device.set_brightness(50).unwrap();
    // device.clear_all_button_images().unwrap();
    // // Use image-rs to load an image
    // let image = open("frame.jpg").unwrap();

    // println!("Key count: {}", kind.key_count());
    // // Write it to the device
    // for i in 0..kind.display_key_count() {
    //     device.set_button_image(i, image.clone()).unwrap();
    // }

    // println!("Flushing");
    // // Flush
    // device.flush().unwrap();

    //         println!("Setting logo image");
    //         let bg = image::open("bg1.jpg").unwrap();
    //         device.set_logo_image(bg).unwrap();
    //         // device.flush().unwrap();
    //         // device.shutdown().unwrap();
    //         println!("Flushed logo image");

    //         println!("Getting reader");

    //         let device = Arc::new(device);
    //         let reader = device.get_reader();

    //         loop {
    //             let updates = match reader.read(Some(Duration::from_secs_f64(100.0))) {
    //                 Ok(updates) => updates,
    //                 Err(e) => {
    //                     println!("Error: {}", e);
    //                     break;
    //                 }
    //             };
    //             for update in updates {
    //                 match update {
    //                     DeviceStateUpdate::ButtonDown(button) => {
    //                         println!("Button {} down", button);
    //                     }
    //                     DeviceStateUpdate::ButtonUp(button) => {
    //                         println!("Button {} up", button);
    //                     }
    //                     DeviceStateUpdate::EncoderTwist(dial, ticks) => {
    //                         println!("Dial {} twisted by {}", dial, ticks);
    //                     }
    //                     DeviceStateUpdate::EncoderDown(dial) => {
    //                         println!("Dial {} down", dial);
    //                     }
    //                     DeviceStateUpdate::EncoderUp(dial) => {
    //                         println!("Dial {} up", dial);
    //                     }
    //                 }
    //             }
    //         }

    //         device.shutdown().ok();
    //     }
    //     Err(e) => eprintln!("Failed to create HidApi instance: {}", e),
    // }
}
