use openpad_core::led::Rgb;

pub const VID: u16 = 0xD010;
pub const PID: u16 = 0x1601;

pub trait PadLink: Send {
    fn send_frame(&mut self, frame: &[Rgb; 16]) -> Result<(), String>;
}

#[derive(Default)]
pub struct FakePad { pub frames: Vec<[Rgb; 16]> }

impl PadLink for FakePad {
    fn send_frame(&mut self, frame: &[Rgb; 16]) -> Result<(), String> {
        self.frames.push(*frame);
        Ok(())
    }
}

pub struct HidPad { dev: hidapi::HidDevice, last: Option<[Rgb; 16]> }

impl HidPad {
    pub fn open() -> Result<HidPad, String> {
        let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
        let dev = api.device_list()
            .find(|d| d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == 0xFF60)
            .ok_or("openpad: DOIO raw-hid interface not found")?
            .open_device(&api).map_err(|e| e.to_string())?;
        Ok(HidPad { dev, last: None })
    }
    fn via_set(&self, value_id: u8, data: &[u8]) -> Result<(), String> {
        let mut msg = [0u8; 33];
        msg[1] = 0x07; msg[2] = 3; msg[3] = value_id;
        msg[4..4 + data.len()].copy_from_slice(data);
        self.dev.write(&msg).map(|_| ()).map_err(|e| e.to_string())
    }
}

fn rgb_to_hs(c: Rgb) -> (u8, u8) {
    let (r, g, b) = (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0);
    let max = r.max(g).max(b); let min = r.min(g).min(b); let d = max - min;
    let h = if d == 0.0 { 0.0 }
        else if max == r { 60.0 * (((g - b) / d) % 6.0) }
        else if max == g { 60.0 * ((b - r) / d + 2.0) }
        else { 60.0 * ((r - g) / d + 4.0) };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { d / max };
    ((h / 360.0 * 255.0) as u8, (s * 255.0) as u8)
}

impl PadLink for HidPad {
    /// Global-color fallback: renders key 3 (the most-urgent aggregate) to the whole pad.
    fn send_frame(&mut self, frame: &[Rgb; 16]) -> Result<(), String> {
        if self.last.as_ref() == Some(frame) { return Ok(()); } // skip no-op writes
        let c = frame[3];
        let (h, s) = rgb_to_hs(c);
        let v = (c.0.max(c.1).max(c.2)) as u8;
        self.via_set(2, &[1])?;          // effect: solid color
        self.via_set(4, &[h, s])?;       // hue/sat
        self.via_set(1, &[v])?;          // brightness = value → carries the WAITING pulse
        self.last = Some(*frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpad_core::led::Rgb;

    #[test]
    fn fake_pad_records_frames() {
        let mut pad = FakePad::default();
        let frame = [Rgb(1, 2, 3); 16];
        pad.send_frame(&frame).unwrap();
        assert_eq!(pad.frames.len(), 1);
        assert_eq!(pad.frames[0][0], Rgb(1, 2, 3));
    }
}
