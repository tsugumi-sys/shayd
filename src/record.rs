use crate::error::{Error, Result};
use crate::varint;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    values: Vec<Value>,
}

impl Record {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    pub fn values(&self) -> &[Value] {
        &self.values
    }

    pub fn decode(payload: &[u8]) -> Result<Self> {
        let (header_size, header_size_len) = varint::decode(payload)?;
        let header_size = usize::try_from(header_size).map_err(|_| Error::InvalidVarint)?;
        if header_size < header_size_len || header_size > payload.len() {
            return Err(Error::truncated(
                "record header",
                header_size,
                payload.len(),
            ));
        }

        let mut header_cursor = header_size_len;
        let mut data_cursor = header_size;
        let mut values = Vec::new();

        while header_cursor < header_size {
            let (serial_type, serial_len) = varint::decode(&payload[header_cursor..header_size])?;
            header_cursor += serial_len;
            let value_len = serial_type_len(serial_type)?;
            let data_end = data_cursor + value_len;
            let data = payload
                .get(data_cursor..data_end)
                .ok_or_else(|| Error::truncated("record data", data_end, payload.len()))?;
            values.push(decode_value(serial_type, data)?);
            data_cursor = data_end;
        }

        Ok(Self { values })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let serial_types: Vec<u64> = self.values.iter().map(serial_type_for_value).collect();
        let serial_types_len: usize = serial_types
            .iter()
            .map(|serial_type| varint::encoded_len(*serial_type))
            .sum();

        let mut header_size = serial_types_len + 1;
        loop {
            let next = serial_types_len + varint::encoded_len(header_size as u64);
            if next == header_size {
                break;
            }
            header_size = next;
        }

        let mut out =
            Vec::with_capacity(header_size + self.values.iter().map(value_len).sum::<usize>());
        varint::encode_to_vec(header_size as u64, &mut out);
        for serial_type in serial_types {
            varint::encode_to_vec(serial_type, &mut out);
        }
        for value in &self.values {
            encode_value(value, &mut out);
        }

        Ok(out)
    }
}

fn serial_type_for_value(value: &Value) -> u64 {
    match value {
        Value::Null => 0,
        Value::Integer(0) => 8,
        Value::Integer(1) => 9,
        Value::Integer(value) => match *value {
            -128..=127 => 1,
            -32_768..=32_767 => 2,
            -8_388_608..=8_388_607 => 3,
            -2_147_483_648..=2_147_483_647 => 4,
            -140_737_488_355_328..=140_737_488_355_327 => 5,
            _ => 6,
        },
        Value::Real(_) => 7,
        Value::Blob(bytes) => 12 + (bytes.len() as u64 * 2),
        Value::Text(text) => 13 + (text.len() as u64 * 2),
    }
}

fn serial_type_len(serial_type: u64) -> Result<usize> {
    match serial_type {
        0 => Ok(0),
        1 => Ok(1),
        2 => Ok(2),
        3 => Ok(3),
        4 => Ok(4),
        5 => Ok(6),
        6 | 7 => Ok(8),
        8 | 9 => Ok(0),
        10 | 11 => Err(Error::ReservedSerialType(serial_type)),
        value if value >= 12 => Ok(((value - 12) / 2) as usize),
        _ => Err(Error::ReservedSerialType(serial_type)),
    }
}

fn decode_value(serial_type: u64, data: &[u8]) -> Result<Value> {
    match serial_type {
        0 => Ok(Value::Null),
        1..=6 => Ok(Value::Integer(decode_integer(data))),
        7 => {
            let mut bytes = [0; 8];
            bytes.copy_from_slice(data);
            Ok(Value::Real(f64::from_bits(u64::from_be_bytes(bytes))))
        }
        8 => Ok(Value::Integer(0)),
        9 => Ok(Value::Integer(1)),
        10 | 11 => Err(Error::ReservedSerialType(serial_type)),
        value if value >= 12 && value % 2 == 0 => Ok(Value::Blob(data.to_vec())),
        value if value >= 13 && value % 2 == 1 => {
            Ok(Value::Text(std::str::from_utf8(data)?.to_owned()))
        }
        _ => Err(Error::ReservedSerialType(serial_type)),
    }
}

fn decode_integer(data: &[u8]) -> i64 {
    let negative = data.first().is_some_and(|byte| byte & 0x80 != 0);
    let fill = if negative { 0xff } else { 0 };
    let mut bytes = [fill; 8];
    bytes[8 - data.len()..].copy_from_slice(data);
    i64::from_be_bytes(bytes)
}

fn encode_value(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null | Value::Integer(0 | 1) => {}
        Value::Integer(value) => {
            let bytes = value.to_be_bytes();
            let len = integer_value_len(*value);
            out.extend_from_slice(&bytes[8 - len..]);
        }
        Value::Real(value) => out.extend_from_slice(&value.to_bits().to_be_bytes()),
        Value::Text(text) => out.extend_from_slice(text.as_bytes()),
        Value::Blob(bytes) => out.extend_from_slice(bytes),
    }
}

fn value_len(value: &Value) -> usize {
    match value {
        Value::Null | Value::Integer(0 | 1) => 0,
        Value::Integer(value) => integer_value_len(*value),
        Value::Real(_) => 8,
        Value::Text(text) => text.len(),
        Value::Blob(bytes) => bytes.len(),
    }
}

fn integer_value_len(value: i64) -> usize {
    match value {
        -128..=127 => 1,
        -32_768..=32_767 => 2,
        -8_388_608..=8_388_607 => 3,
        -2_147_483_648..=2_147_483_647 => 4,
        -140_737_488_355_328..=140_737_488_355_327 => 6,
        _ => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_sqlite_record() {
        let payload = [
            6,    // header size
            0,    // NULL
            8,    // integer 0
            9,    // integer 1
            1,    // 1-byte integer
            15,   // 1-byte text
            0x7f, // integer data
            b'x',
        ];

        let record = Record::decode(&payload).unwrap();
        assert_eq!(
            record.values(),
            &[
                Value::Null,
                Value::Integer(0),
                Value::Integer(1),
                Value::Integer(127),
                Value::Text("x".to_owned()),
            ]
        );
    }

    #[test]
    fn roundtrips_values() {
        let values = vec![
            Value::Null,
            Value::Integer(-129),
            Value::Integer(0),
            Value::Integer(1),
            Value::Integer(8_388_608),
            Value::Real(3.5),
            Value::Text("hello".to_owned()),
            Value::Blob(vec![0, 1, 2, 3]),
        ];

        let record = Record::new(values.clone());
        let payload = record.encode().unwrap();
        let decoded = Record::decode(&payload).unwrap();
        assert_eq!(decoded.values(), values);
    }

    #[test]
    fn rejects_reserved_serial_type() {
        let payload = [2, 10];
        assert!(matches!(
            Record::decode(&payload),
            Err(Error::ReservedSerialType(10))
        ));
    }
}
