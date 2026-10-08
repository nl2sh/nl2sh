//! Small, bounded protobuf wire reader. Unknown fields are skipped, never recursively decoded.
use anyhow::{bail, Context, Result};

#[derive(Clone, Copy)]
pub(super) struct Field<'a> {
    pub id: u32,
    pub number: Option<u64>,
    pub bytes: &'a [u8],
}

pub(super) fn varint(input: &mut &[u8]) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *input.first().context("truncated protobuf varint")?;
        *input = &input[1..];
        if shift == 63 && byte > 1 {
            bail!("protobuf varint overflow")
        }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    bail!("protobuf varint overflow")
}

pub(super) fn fields(mut input: &[u8]) -> Result<Vec<Field<'_>>> {
    let mut result = Vec::new();
    while !input.is_empty() {
        if result.len() >= 100_000 {
            bail!("protobuf field budget exceeded")
        }
        let key = varint(&mut input)?;
        let id = u32::try_from(key >> 3).context("invalid protobuf field number")?;
        if id == 0 || id > 0x1fff_ffff {
            bail!("invalid protobuf field number")
        }
        let mut field = Field {
            id,
            number: None,
            bytes: &[],
        };
        let size = match key & 7 {
            0 => {
                field.number = Some(varint(&mut input)?);
                0
            }
            1 => 8,
            2 => usize::try_from(varint(&mut input)?).context("protobuf length overflow")?,
            5 => 4,
            _ => bail!("unsupported protobuf wire type"),
        };
        field.bytes = input.get(..size).context("truncated protobuf field")?;
        input = &input[size..];
        result.push(field);
    }
    Ok(result)
}

pub(super) fn num(fields: &[Field<'_>], id: u32) -> Option<u64> {
    fields.iter().rev().find(|f| f.id == id)?.number
}
pub(super) fn bytes<'a>(fields: &[Field<'a>], id: u32) -> Option<&'a [u8]> {
    fields
        .iter()
        .rev()
        .find(|f| f.id == id && f.number.is_none())
        .map(|f| f.bytes)
}
pub(super) fn string(fields: &[Field<'_>], id: u32) -> String {
    bytes(fields, id)
        .and_then(|b| std::str::from_utf8(b).ok())
        .map(|s| s.chars().filter(|c| !c.is_control()).take(256).collect())
        .unwrap_or_default()
}
pub(super) fn put_num(out: &mut Vec<u8>, id: u32, value: u64) {
    put_varint(out, u64::from(id) << 3);
    put_varint(out, value);
}
pub(super) fn put_bytes(out: &mut Vec<u8>, id: u32, value: &[u8]) {
    put_varint(out, (u64::from(id) << 3) | 2);
    put_varint(out, value.len() as u64);
    out.extend(value);
}
fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
