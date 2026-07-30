use crate::error::{Error, Result};

pub const MAX_LEN: usize = 9;

pub fn decode(bytes: &[u8]) -> Result<(u64, usize)> {
    let mut value = 0_u64;

    for index in 0..MAX_LEN {
        let byte = *bytes.get(index).ok_or(Error::InvalidVarint)?;

        if index == 8 {
            value = (value << 8) | u64::from(byte);
            return Ok((value, 9));
        }

        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok((value, index + 1));
        }
    }

    Err(Error::InvalidVarint)
}

pub fn encode(mut value: u64, out: &mut [u8]) -> Result<usize> {
    let len = encoded_len(value);
    if out.len() < len {
        return Err(Error::truncated("varint output", len, out.len()));
    }

    if len == 9 {
        out[8] = value as u8;
        value >>= 8;
        for index in (0..8).rev() {
            out[index] = ((value & 0x7f) as u8) | 0x80;
            value >>= 7;
        }
        return Ok(9);
    }

    for index in (0..len).rev() {
        out[index] = (value & 0x7f) as u8;
        if index != len - 1 {
            out[index] |= 0x80;
        }
        value >>= 7;
    }

    Ok(len)
}

pub const fn encoded_len(value: u64) -> usize {
    match value {
        0..=0x7f => 1,
        0x80..=0x3fff => 2,
        0x4000..=0x1f_ffff => 3,
        0x20_0000..=0x0fff_ffff => 4,
        0x1000_0000..=0x0007_ffff_ffff => 5,
        0x0008_0000_0000..=0x03ff_ffff_ffff => 6,
        0x0400_0000_0000..=0x0001_ffff_ffff_ffff => 7,
        0x0002_0000_0000_0000..=0x00ff_ffff_ffff_ffff => 8,
        _ => 9,
    }
}

pub fn encode_to_vec(value: u64, out: &mut Vec<u8>) {
    let mut buf = [0; MAX_LEN];
    let len = encode(value, &mut buf).expect("fixed-size varint buffer must fit");
    out.extend_from_slice(&buf[..len]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_boundary_values() {
        let values = [
            0,
            1,
            127,
            128,
            16_383,
            16_384,
            u32::MAX as u64,
            0x00ff_ffff_ffff_ffff,
            u64::MAX,
        ];

        for value in values {
            let mut buf = [0; MAX_LEN];
            let len = encode(value, &mut buf).unwrap();
            let (decoded, read) = decode(&buf[..len]).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(read, len);
        }
    }

    #[test]
    fn encodes_known_values() {
        let mut buf = [0; MAX_LEN];
        assert_eq!(encode(127, &mut buf).unwrap(), 1);
        assert_eq!(&buf[..1], &[0x7f]);

        assert_eq!(encode(128, &mut buf).unwrap(), 2);
        assert_eq!(&buf[..2], &[0x81, 0x00]);
    }
}
