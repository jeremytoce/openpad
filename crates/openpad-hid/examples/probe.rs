fn main() -> Result<(), String> {
    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let dev = api.device_list()
        .find(|d| d.vendor_id() == 0xD010 && d.product_id() == 0x1601 && d.usage_page() == 0xFF60)
        .ok_or("pad raw-hid interface not found")?
        .open_device(&api).map_err(|e| e.to_string())?;
    // VIA report: [report_id=0x00, command, channel, value_id, data...] padded to 33 bytes
    let mut msg = [0u8; 33];
    msg[1] = 0x07; msg[2] = 3; msg[3] = 2; msg[4] = 1;        // effect = solid color
    dev.write(&msg).map_err(|e| e.to_string())?;
    let mut msg = [0u8; 33];
    msg[1] = 0x07; msg[2] = 3; msg[3] = 4; msg[4] = 28; msg[5] = 255; // color: hue=amber, sat=max
    dev.write(&msg).map_err(|e| e.to_string())?;
    println!("sent solid amber — did the pad change color?");
    Ok(())
}
