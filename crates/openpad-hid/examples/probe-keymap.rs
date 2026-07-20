// probe: VIA dynamic_keymap_get_keycode (cmd 0x04): layer 0, row 0, col 0
// expect KC_F13 = 0x0068 if our layout is loaded
fn main() -> Result<(), String> {
    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let dev = api.device_list()
        .find(|d| d.vendor_id() == 0xD010 && d.product_id() == 0x1601 && d.usage_page() == 0xFF60)
        .ok_or("pad not found")?.open_device(&api).map_err(|e| e.to_string())?;
    for (layer, row, col) in [(0u8,0u8,0u8),(0,0,1),(0,2,0),(1,0,0)] {
        let mut msg = [0u8; 33];
        msg[1] = 0x04; msg[2] = layer; msg[3] = row; msg[4] = col;
        dev.write(&msg).map_err(|e| e.to_string())?;
        let mut buf = [0u8; 32];
        let n = dev.read_timeout(&mut buf, 500).map_err(|e| e.to_string())?;
        println!("L{layer} r{row}c{col}: read {n} bytes, keycode = 0x{:02X}{:02X}", buf[4], buf[5]);
    }
    Ok(())
}
