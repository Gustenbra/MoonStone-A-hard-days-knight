//! Creative Voice File. All 49 of the game's samples are in this format, which is
//! a documented standard rather than anything Moonstone invented.
//!
//! Header is `"Creative Voice File\x1a"`, a u16 header size, version and checksum.
//! Then typed blocks with 24-bit lengths. Type 1 is 8-bit unsigned PCM whose rate
//! comes from a time constant, `1_000_000 / (256 - tc)`. Type 9 states the rate
//! outright. Types 2 and 3 continue or pad the previous block.

pub struct Sample {
    pub rate: u32,
    pub channels: u16,
    pub bits: u16,
    /// 8-bit samples are unsigned, as the format stores them.
    pub data: Vec<u8>,
}

pub fn parse(d: &[u8]) -> anyhow::Result<Sample> {
    anyhow::ensure!(
        d.len() > 26 && &d[..19] == b"Creative Voice File",
        "not a Creative Voice File"
    );
    let header_size = u16::from_le_bytes([d[20], d[21]]) as usize;
    let mut o = header_size;
    let mut out = Sample {
        rate: 11025,
        channels: 1,
        bits: 8,
        data: Vec::new(),
    };
    let mut seen_rate = false;

    while o + 4 <= d.len() {
        let kind = d[o];
        if kind == 0 {
            break;
        }
        let len = u32::from_le_bytes([d[o + 1], d[o + 2], d[o + 3], 0]) as usize;
        let body = o + 4;
        let end = (body + len).min(d.len());
        match kind {
            1 => {
                anyhow::ensure!(end > body + 1, "truncated sound block");
                if !seen_rate {
                    let tc = d[body] as u32;
                    out.rate = 1_000_000 / (256 - tc).max(1);
                    seen_rate = true;
                }
                anyhow::ensure!(d[body + 1] == 0, "compressed VOC data is not supported");
                out.data.extend_from_slice(&d[body + 2..end]);
            }
            2 => out.data.extend_from_slice(&d[body..end]),
            9 => {
                anyhow::ensure!(end > body + 11, "truncated extended sound block");
                out.rate = u32::from_le_bytes([d[body], d[body + 1], d[body + 2], d[body + 3]]);
                out.bits = d[body + 4] as u16;
                out.channels = d[body + 5] as u16;
                seen_rate = true;
                out.data.extend_from_slice(&d[body + 12..end]);
            }
            _ => {} // silence, markers, text, repeat markers: nothing we need
        }
        o = body + len;
    }
    anyhow::ensure!(!out.data.is_empty(), "no audio in this VOC");
    Ok(out)
}

impl Sample {
    /// A plain RIFF/WAVE file, so the game itself never has to know about VOC.
    pub fn to_wav(&self) -> Vec<u8> {
        let block_align = self.channels * self.bits / 8;
        let byte_rate = self.rate * block_align as u32;
        let mut w = Vec::with_capacity(44 + self.data.len());
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + self.data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes()); // PCM
        w.extend_from_slice(&self.channels.to_le_bytes());
        w.extend_from_slice(&self.rate.to_le_bytes());
        w.extend_from_slice(&byte_rate.to_le_bytes());
        w.extend_from_slice(&block_align.to_le_bytes());
        w.extend_from_slice(&self.bits.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        w.extend_from_slice(&self.data);
        w
    }
}
