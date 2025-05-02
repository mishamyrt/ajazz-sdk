use std::str::{from_utf8, Utf8Error};
use std::time::Duration;
use hidapi::{HidDevice, HidError};
use crate::{Kind, AjazzError, AjazzInput};

/// Performs get_feature_report on [HidDevice]
pub fn get_feature_report(device: &HidDevice, report_id: u8, length: usize) -> Result<Vec<u8>, HidError> {
    let mut buff = vec![0u8; length];

    // Inserting report id byte
    buff.insert(0, report_id);

    // Getting feature report
    device.get_feature_report(buff.as_mut_slice())?;

    Ok(buff)
}

/// Performs send_feature_report on [HidDevice]
pub fn send_feature_report(device: &HidDevice, payload: &[u8]) -> Result<(), HidError> {
    device.send_feature_report(payload)
}

/// Reads data from [HidDevice]. Blocking mode is used if timeout is specified
pub fn read_data(device: &HidDevice, length: usize, timeout: Option<Duration>) -> Result<Vec<u8>, HidError> {
    device.set_blocking_mode(timeout.is_some())?;

    let mut buf = vec![0u8; length];

    match timeout {
        Some(timeout) => device.read_timeout(buf.as_mut_slice(), timeout.as_millis() as i32),
        None => device.read(buf.as_mut_slice()),
    }?;

    Ok(buf)
}

/// Writes data to [HidDevice]
pub fn write_data(device: &HidDevice, payload: &[u8]) -> Result<usize, HidError> {
    device.write(payload)
}

/// Extracts string from byte array, removing \0 symbols
pub fn extract_str(bytes: &[u8]) -> Result<String, Utf8Error> {
    Ok(from_utf8(bytes)?.replace('\0', "").to_string())
}

/*
 Ajazz AKP153x's key index
 -----------------------------
| 0d | 0a | 07 | 04 | 01 | 10 |
|----|----|----|----|----|----|
| 0e | 0b | 08 | 05 | 02 | 11 |
|----|----|----|----|----|----|
| 0f | 0c | 09 | 06 | 03 | 12 |
 -----------------------------

 Ajazz AKP815's key index
  --------------
 | 0f | 0e | 0d |
 |----|----|----|
 | 0c | 0b | 0a |
 |----|----|----|
 | 09 | 08 | 07 |
 |----|----|----|
 | 06 | 05 | 04 |
 |----|----|----|
 | 03 | 02 | 01 |
  --------------

*/

/// Converts Elgato key index to Ajazz key index
pub fn elgato_to_ajazz153(kind: &Kind, key: u8) -> u8 {
    if key < kind.key_count() {
        [12, 9, 6, 3, 0, 15, 13, 10, 7, 4, 1, 16, 14, 11, 8, 5, 2, 17][key as usize]
    } else {
        key
    }
}

/// Converts Ajazz key index to Elgato key index
pub fn ajazz153_to_elgato_input(kind: &Kind, key: u8) -> u8 {
    if key < kind.key_count() {
        [4, 10, 16, 3, 9, 15, 2, 8, 14, 1, 7, 13, 0, 6, 12, 5, 11, 17][key as usize]
    } else {
        key
    }
}

/// Make last key index first, and first key index last
pub fn inverse_key_index(kind: &Kind, key: u8) -> u8 {
    if key < kind.key_count() {
        kind.key_count() - 1 - key
    } else {
        key
    }
}

/// Extends buffer up to required packet length
pub fn mirabox_extend_packet(kind: &Kind, buf: &mut Vec<u8>) {
    let length = if kind.is_ajazz_v2() { 1025 } else { 513 };

    buf.extend(vec![0u8; length - buf.len()]);
}

/// Reads button states, empty vector if no data
pub fn read_button_states(kind: &Kind, states: &[u8]) -> Vec<bool> {
    if states[0] == 0 {
        return vec![];
    }

    let mut bools = vec![];

    for i in 0..kind.key_count() {
        bools.push(states[(i + 1) as usize] != 0);
    }

    bools
}

/// Reads encoder input
pub fn read_encoder_input(kind: &Kind, data: &[u8]) -> Result<AjazzInput, AjazzError> {
    match &data[4] {
        0x0 => Ok(AjazzInput::EncoderStateChange(data[5..5 + kind.encoder_count() as usize].iter().map(|s| *s != 0).collect())),

        0x1 => Ok(AjazzInput::EncoderTwist(
            data[5..5 + kind.encoder_count() as usize].iter().map(|s| i8::from_le_bytes([*s])).collect(),
        )),

        _ => Err(AjazzError::BadData),
    }
}

/// Read inputs from Ajazz AKP03x devices
pub fn ajazz03_read_input(kind: &Kind, input: u8, state: u8) -> Result<AjazzInput, AjazzError> {
    match input {
        (0..=6) | 0x25 | 0x30 | 0x31 => ajazz03_read_button_press(kind, input, state),
        0x90 | 0x91 | 0x50 | 0x51 | 0x60 | 0x61 => ajazz03_read_encoder_value(kind, input),
        0x33..=0x35 => ajazz03_read_encoder_press(kind, input, state),
        _ => Err(AjazzError::BadData),
    }
}

fn ajazz03_read_button_press(kind: &Kind, input: u8, state: u8) -> Result<AjazzInput, AjazzError> {
    let mut button_states = vec![0x01];
    button_states.extend(vec![0u8; (kind.key_count() + 1) as usize]);

    if input == 0 {
        return Ok(AjazzInput::ButtonStateChange(read_button_states(kind, &button_states)));
    }

    let pressed_index: usize = match input {
        // Six buttons with displays
        (1..=6) => input as usize,
        // Three buttons without displays
        0x25 => 7,
        0x30 => 8,
        0x31 => 9,
        _ => return Err(AjazzError::BadData),
    };

    button_states[pressed_index] = state;

    Ok(AjazzInput::ButtonStateChange(read_button_states(kind, &button_states)))
}

fn ajazz03_read_encoder_value(kind: &Kind, input: u8) -> Result<AjazzInput, AjazzError> {
    let mut encoder_values = vec![0i8; kind.encoder_count() as usize];

    let (encoder, value): (usize, i8) = match input {
        // Left encoder
        0x90 => (0, -1),
        0x91 => (0, 1),
        // Middle (top) encoder
        0x50 => (1, -1),
        0x51 => (1, 1),
        // Right encoder
        0x60 => (2, -1),
        0x61 => (2, 1),
        _ => return Err(AjazzError::BadData),
    };

    encoder_values[encoder] = value;
    Ok(AjazzInput::EncoderTwist(encoder_values))
}

fn ajazz03_read_encoder_press(kind: &Kind, input: u8, state: u8) -> Result<AjazzInput, AjazzError> {
    let mut encoder_states = vec![false; kind.encoder_count() as usize];

    let encoder: usize = match input {
        0x33 => 0, // Left encoder
        0x35 => 1, // Middle (top) encoder
        0x34 => 2, // Right encoder
        _ => return Err(AjazzError::BadData),
    };

    encoder_states[encoder] = state != 0;
    Ok(AjazzInput::EncoderStateChange(encoder_states))
}
