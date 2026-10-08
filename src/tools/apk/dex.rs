//! Read-only DEX tables and instruction references, bounded independently of ZIP metadata.
//! Format: https://source.android.com/docs/core/runtime/dex-format
//! Opcodes: https://source.android.com/docs/core/runtime/dalvik-bytecode

use super::{table, u32_at};
use anyhow::{bail, Context, Result};
use serde::Serialize;

const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_INSTRUCTION_UNITS: usize = 4 * 1024 * 1024;
/// Per-string guard, checked before allocating so a bogus `utf16_size` cannot force a large
/// allocation. Real APKs embed whole obfuscator class maps as one string; the largest observed in
/// the field is 475,507 units, so this leaves over 2x headroom while capping a single decoded
/// string at 2 MiB. [`MAX_TEXT_BYTES`] still bounds the decoded total per DEX file.
const MAX_STRING_UNITS: usize = 1024 * 1024;

pub(super) fn u16_at(bytes: &[u8], offset: usize) -> Result<usize> {
    let end = offset.checked_add(2).context("DEX offset overflow")?;
    let bytes: [u8; 2] = bytes
        .get(offset..end)
        .context("truncated DEX halfword")?
        .try_into()
        .context("invalid DEX halfword")?;
    Ok(u16::from_le_bytes(bytes) as usize)
}

fn uleb(bytes: &[u8], position: &mut usize) -> Result<usize> {
    let mut value = 0u32;
    for shift in (0..=28).step_by(7) {
        let byte = *bytes
            .get(*position)
            .context("truncated DEX variable integer")?;
        *position += 1;
        if shift == 28 && byte > 15 {
            bail!("overflowing DEX variable integer")
        }
        value |= u32::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Ok(value as usize);
        }
    }
    bail!("unterminated DEX variable integer")
}

fn string(bytes: &[u8], offset: usize) -> Result<String> {
    let mut position = offset;
    let length = uleb(bytes, &mut position)?;
    if length > MAX_STRING_UNITS {
        bail!("DEX string exceeds {MAX_STRING_UNITS} UTF-16 units")
    }
    let mut units = Vec::with_capacity(length);
    while units.len() < length {
        let a = *bytes.get(position).context("truncated DEX string")?;
        position += 1;
        let unit = match a {
            1..=127 => u16::from(a),
            0xc0..=0xdf => {
                let b = *bytes.get(position).context("truncated DEX string")?;
                position += 1;
                if b & 0xc0 != 0x80 {
                    bail!("invalid DEX modified UTF-8")
                }
                let value = (u16::from(a & 31) << 6) | u16::from(b & 63);
                if value < 128 && !(a == 0xc0 && b == 0x80) {
                    bail!("overlong DEX string encoding")
                }
                value
            }
            0xe0..=0xef => {
                let b = *bytes.get(position).context("truncated DEX string")?;
                let c = *bytes.get(position + 1).context("truncated DEX string")?;
                position += 2;
                if b & 0xc0 != 0x80 || c & 0xc0 != 0x80 {
                    bail!("invalid DEX modified UTF-8")
                }
                let value =
                    (u16::from(a & 15) << 12) | (u16::from(b & 63) << 6) | u16::from(c & 63);
                if value < 2048 {
                    bail!("overlong DEX string encoding")
                }
                value
            }
            _ => bail!("invalid DEX modified UTF-8"),
        };
        units.push(unit);
    }
    if bytes.get(position) != Some(&0) {
        bail!("DEX string length or terminator mismatch")
    }
    // Java strings may contain lone surrogates; retain a bounded, readable representation.
    Ok(String::from_utf16_lossy(&units))
}

#[derive(Debug, Serialize)]
pub(super) struct Method {
    pub class: String,
    pub name: String,
    pub prototype: String,
    pub signature: String,
    #[serde(skip)]
    pub owner: usize,
    #[serde(skip)]
    pub proto: usize,
}

struct Prototype {
    text: String,
    types: Vec<usize>,
}
struct Field {
    owner: usize,
    field_type: usize,
}

pub(super) struct Dex<'a> {
    bytes: &'a [u8],
    pub strings: Vec<String>,
    types: Vec<String>,
    prototypes: Vec<Prototype>,
    pub methods: Vec<Method>,
    fields: Vec<Field>,
    classes: (usize, usize),
}

#[derive(Debug, Serialize)]
pub(super) struct Reference<'a> {
    pub source: &'a str,
    pub kind: &'static str,
    pub target: &'a str,
    pub instruction_offset: Option<usize>,
}

pub(super) enum Query<'a> {
    Class(&'a str),
    Method {
        class: &'a str,
        name: &'a str,
        prototype: &'a str,
    },
}

impl<'a> Dex<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < 112
            || !bytes.starts_with(b"dex\n")
            || bytes.get(7) != Some(&0)
            || !matches!(
                bytes.get(4..7),
                Some(b"035" | b"037" | b"038" | b"039" | b"040")
            )
        {
            bail!("unsupported DEX header; expected standalone version 035–040")
        }
        if u32_at(bytes, 32)? != bytes.len()
            || u32_at(bytes, 36)? != 112
            || u32_at(bytes, 40)? != 0x12345678
        {
            bail!("invalid DEX size, header or endianness")
        }
        let (count, offset) = table(bytes, 56, 60, 4)?;
        if count > 200_000 {
            bail!("DEX string table exceeds inspection limit")
        }
        let mut strings = Vec::with_capacity(count);
        let mut text_bytes = 0usize;
        for index in 0..count {
            let value = string(bytes, u32_at(bytes, offset + index * 4)?)?;
            text_bytes += value.len();
            if text_bytes > MAX_TEXT_BYTES {
                bail!("DEX decoded strings exceed 16 MiB")
            }
            strings.push(value);
        }
        let (count, offset) = table(bytes, 64, 68, 4)?;
        if count > 65_535 {
            bail!("DEX type table exceeds inspection limit")
        }
        let mut types = Vec::with_capacity(count);
        for index in 0..count {
            let value = strings
                .get(u32_at(bytes, offset + index * 4)?)
                .context("invalid DEX type string")?;
            if value.len() > 1024 {
                bail!("DEX type descriptor too long")
            }
            text_bytes += value.len();
            if text_bytes > MAX_TEXT_BYTES {
                bail!("DEX decoded metadata exceeds 16 MiB")
            }
            types.push(value.clone());
        }
        let (count, offset) = table(bytes, 72, 76, 12)?;
        if count > 65_535 {
            bail!("DEX prototype table exceeds inspection limit")
        }
        let mut prototypes = Vec::with_capacity(count);
        let mut prototype_type_count = 0usize;
        for index in 0..count {
            let base = offset + index * 12;
            let return_type = u32_at(bytes, base + 4)?;
            let parameters = Self::type_list(bytes, u32_at(bytes, base + 8)?, types.len())?;
            prototype_type_count += parameters.len() + 1;
            if prototype_type_count > 1_000_000 {
                bail!("DEX prototype types exceed inspection limit")
            }
            let mut text = String::from("(");
            for index in &parameters {
                text.push_str(types.get(*index).context("invalid DEX parameter type")?);
            }
            text.push(')');
            text.push_str(types.get(return_type).context("invalid DEX return type")?);
            text_bytes += text.len();
            if text_bytes > MAX_TEXT_BYTES {
                bail!("DEX decoded metadata exceeds 16 MiB")
            }
            let mut referenced = parameters;
            referenced.push(return_type);
            prototypes.push(Prototype {
                text,
                types: referenced,
            });
        }
        let (count, offset) = table(bytes, 80, 84, 8)?;
        if count > 65_535 {
            bail!("DEX field table exceeds inspection limit")
        }
        let mut fields = Vec::with_capacity(count);
        for index in 0..count {
            let base = offset + index * 8;
            let owner = u16_at(bytes, base)?;
            let field_type = u16_at(bytes, base + 2)?;
            if owner >= types.len()
                || field_type >= types.len()
                || u32_at(bytes, base + 4)? >= strings.len()
            {
                bail!("invalid DEX field index")
            }
            fields.push(Field { owner, field_type });
        }
        let (count, offset) = table(bytes, 88, 92, 8)?;
        if count > 65_535 {
            bail!("DEX method table exceeds inspection limit")
        }
        let mut methods = Vec::with_capacity(count);
        for index in 0..count {
            let base = offset + index * 8;
            let owner = u16_at(bytes, base)?;
            let proto = u16_at(bytes, base + 2)?;
            let descriptor = types.get(owner).context("invalid DEX method class")?;
            let name = strings
                .get(u32_at(bytes, base + 4)?)
                .context("invalid DEX method name")?;
            if name.len() > 1024 {
                bail!("DEX method name too long")
            }
            let prototype = &prototypes
                .get(proto)
                .context("invalid DEX method prototype")?
                .text;
            let signature = format!("{descriptor}->{name}{prototype}");
            let class = dotted(descriptor);
            text_bytes += signature.len() + class.len() + name.len() + prototype.len();
            if text_bytes > MAX_TEXT_BYTES {
                bail!("DEX decoded metadata exceeds 16 MiB")
            }
            methods.push(Method {
                class,
                name: name.clone(),
                prototype: prototype.clone(),
                signature,
                owner,
                proto,
            });
        }
        let classes = table(bytes, 96, 100, 32)?;
        if classes.0 > 40_000 {
            bail!("DEX class table exceeds inspection limit")
        }
        let mut owners = std::collections::HashSet::new();
        for index in 0..classes.0 {
            let owner = u32_at(bytes, classes.1 + index * 32)?;
            let descriptor = types
                .get(owner)
                .context("invalid DEX class definition type")?;
            if !owners.insert(owner) || !descriptor.starts_with('L') || !descriptor.ends_with(';') {
                bail!("duplicate or invalid DEX class definition")
            }
        }
        Ok(Self {
            bytes,
            strings,
            types,
            prototypes,
            methods,
            fields,
            classes,
        })
    }

    pub fn class_names(&self) -> Result<Vec<String>> {
        let mut result = Vec::with_capacity(self.classes.0);
        let mut seen = std::collections::HashSet::new();
        for index in 0..self.classes.0 {
            let owner = u32_at(self.bytes, self.classes.1 + index * 32)?;
            if !seen.insert(owner) {
                bail!("duplicate DEX class definition")
            }
            let descriptor = self
                .types
                .get(owner)
                .context("invalid DEX class definition type")?;
            if !descriptor.starts_with('L') || !descriptor.ends_with(';') {
                bail!("invalid DEX class descriptor")
            }
            result.push(dotted(descriptor));
        }
        Ok(result)
    }

    fn type_list(bytes: &[u8], offset: usize, type_count: usize) -> Result<Vec<usize>> {
        if offset == 0 {
            return Ok(Vec::new());
        }
        let count = u32_at(bytes, offset)?;
        if count > 1024 {
            bail!("DEX type list exceeds inspection limit")
        }
        let mut result = Vec::with_capacity(count);
        for index in 0..count {
            let item = u16_at(bytes, offset + 4 + index * 2)?;
            if item >= type_count {
                bail!("invalid DEX type list index")
            }
            result.push(item);
        }
        Ok(result)
    }

    fn matches_type(&self, index: usize, target: &str) -> Result<bool> {
        Ok(self
            .types
            .get(index)
            .context("invalid DEX type reference")?
            .trim_start_matches('[')
            == target)
    }

    pub fn references(&self, query: Query<'_>, mut emit: impl FnMut(Reference<'_>)) -> Result<()> {
        let class_target = match query {
            Query::Class(class) => Some(format!("L{};", class.replace('.', "/"))),
            _ => None,
        };
        let mut budget = MAX_INSTRUCTION_UNITS;
        for class_index in 0..self.classes.0 {
            let base = self.classes.1 + class_index * 32;
            let owner = u32_at(self.bytes, base)?;
            let source = dotted(
                self.types
                    .get(owner)
                    .context("invalid DEX class definition")?,
            );
            if let Some(target) = &class_target {
                let parent = u32_at(self.bytes, base + 8)?;
                if parent != u32::MAX as usize && self.matches_type(parent, target)? {
                    emit(Reference {
                        source: &source,
                        kind: "superclass",
                        target,
                        instruction_offset: None,
                    });
                }
                let interfaces =
                    Self::type_list(self.bytes, u32_at(self.bytes, base + 12)?, self.types.len())?;
                budget = budget
                    .checked_sub(interfaces.len())
                    .context("DEX interface references exceed inspection budget")?;
                for interface in interfaces {
                    if self.matches_type(interface, target)? {
                        emit(Reference {
                            source: &source,
                            kind: "interface",
                            target,
                            instruction_offset: None,
                        });
                    }
                }
            }
            let mut position = u32_at(self.bytes, base + 24)?;
            if position == 0 {
                continue;
            }
            let static_fields = uleb(self.bytes, &mut position)?;
            let instance_fields = uleb(self.bytes, &mut position)?;
            let direct_methods = uleb(self.bytes, &mut position)?;
            let virtual_methods = uleb(self.bytes, &mut position)?;
            let fields_count = static_fields
                .checked_add(instance_fields)
                .context("DEX class field count overflow")?;
            let methods_count = direct_methods
                .checked_add(virtual_methods)
                .context("DEX class method count overflow")?;
            if fields_count > self.fields.len() || methods_count > self.methods.len() {
                bail!("invalid DEX class data counts")
            }
            budget = budget
                .checked_sub(fields_count + methods_count)
                .context("DEX class data exceeds inspection budget")?;
            for count in [static_fields, instance_fields] {
                let mut index = 0usize;
                for _ in 0..count {
                    index = index
                        .checked_add(uleb(self.bytes, &mut position)?)
                        .context("DEX field index overflow")?;
                    let _flags = uleb(self.bytes, &mut position)?;
                    let field = self
                        .fields
                        .get(index)
                        .context("invalid DEX encoded field")?;
                    if field.owner != owner {
                        bail!("DEX field owner mismatch")
                    }
                    if let Some(target) = &class_target {
                        if self.matches_type(field.field_type, target)? {
                            emit(Reference {
                                source: &source,
                                kind: "field_type",
                                target,
                                instruction_offset: None,
                            });
                        }
                    }
                }
            }
            for count in [direct_methods, virtual_methods] {
                let mut index = 0usize;
                for _ in 0..count {
                    index = index
                        .checked_add(uleb(self.bytes, &mut position)?)
                        .context("DEX method index overflow")?;
                    let _flags = uleb(self.bytes, &mut position)?;
                    let code = uleb(self.bytes, &mut position)?;
                    let method = self
                        .methods
                        .get(index)
                        .context("invalid DEX encoded method")?;
                    if method.owner != owner {
                        bail!("DEX method owner mismatch")
                    }
                    if let Some(target) = &class_target {
                        budget = budget
                            .checked_sub(self.prototypes[method.proto].types.len())
                            .context("DEX type references exceed inspection budget")?;
                        for referenced in &self.prototypes[method.proto].types {
                            if self.matches_type(*referenced, target)? {
                                emit(Reference {
                                    source: &method.signature,
                                    kind: "method_type",
                                    target,
                                    instruction_offset: None,
                                });
                            }
                        }
                    }
                    if code != 0 {
                        self.scan_code(
                            code,
                            method,
                            &query,
                            class_target.as_deref(),
                            &mut budget,
                            &mut emit,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn scan_code(
        &self,
        offset: usize,
        source: &Method,
        query: &Query<'_>,
        class_target: Option<&str>,
        budget: &mut usize,
        emit: &mut impl FnMut(Reference<'_>),
    ) -> Result<()> {
        if offset % 4 != 0 {
            bail!("unaligned DEX code item")
        }
        let start = offset.checked_add(16).context("DEX code offset overflow")?;
        let count = u32_at(self.bytes, start - 4)?;
        if count > *budget {
            bail!("DEX instruction scan exceeds inspection limit")
        }
        *budget -= count;
        let end = count
            .checked_mul(2)
            .and_then(|size| start.checked_add(size))
            .context("DEX code size overflow")?;
        let instructions = self
            .bytes
            .get(start..end)
            .context("truncated DEX instructions")?;
        let mut pc = 0usize;
        while pc < count {
            let word = u16_at(instructions, pc * 2)?;
            let opcode = word & 255;
            let width = instruction_width(instructions, pc, word)?;
            if width == 0 || pc.checked_add(width).is_none_or(|end| end > count) {
                bail!("truncated DEX instruction")
            }
            let mut emit_type = |index: usize, kind| -> Result<()> {
                if let Some(target) = class_target {
                    if self.matches_type(index, target)? {
                        emit(Reference {
                            source: &source.signature,
                            kind,
                            target,
                            instruction_offset: Some(pc),
                        });
                    }
                }
                Ok(())
            };
            match opcode {
                0x1c | 0x1f | 0x20 | 0x22..=0x25 => {
                    emit_type(u16_at(instructions, (pc + 1) * 2)?, "type_instruction")?
                }
                0x52..=0x6d => {
                    let field = self
                        .fields
                        .get(u16_at(instructions, (pc + 1) * 2)?)
                        .context("invalid DEX instruction field")?;
                    emit_type(field.owner, "field_owner")?;
                    emit_type(field.field_type, "field_type_instruction")?;
                }
                0xff => {
                    let proto = self
                        .prototypes
                        .get(u16_at(instructions, (pc + 1) * 2)?)
                        .context("invalid DEX constant method type")?;
                    if class_target.is_some() {
                        *budget = budget
                            .checked_sub(proto.types.len())
                            .context("DEX constant method types exceed inspection budget")?;
                        for index in &proto.types {
                            emit_type(*index, "constant_method_type")?;
                        }
                    }
                }
                0x6e..=0x72 | 0x74..=0x78 | 0xfa | 0xfb => {
                    let method = self
                        .methods
                        .get(u16_at(instructions, (pc + 1) * 2)?)
                        .context("invalid DEX invoke method")?;
                    emit_type(method.owner, "invoke_class")?;
                    if class_target.is_some() {
                        *budget = budget
                            .checked_sub(self.prototypes[method.proto].types.len())
                            .context("DEX invoke types exceed inspection budget")?;
                        for index in &self.prototypes[method.proto].types {
                            emit_type(*index, "invoke_type")?;
                        }
                    }
                    if matches!(opcode, 0xfa | 0xfb) {
                        let proto = self
                            .prototypes
                            .get(u16_at(instructions, (pc + 3) * 2)?)
                            .context("invalid DEX polymorphic prototype")?;
                        if class_target.is_some() {
                            *budget = budget
                                .checked_sub(proto.types.len())
                                .context("DEX polymorphic types exceed inspection budget")?;
                            for index in &proto.types {
                                emit_type(*index, "polymorphic_type")?;
                            }
                        }
                    }
                    if let Query::Method {
                        class,
                        name,
                        prototype,
                    } = query
                    {
                        if method.class == *class
                            && method.name == *name
                            && (prototype.is_empty() || method.prototype == *prototype)
                        {
                            emit(Reference {
                                source: &source.signature,
                                kind: "invoke",
                                target: &method.signature,
                                instruction_offset: Some(pc),
                            });
                        }
                    }
                }
                _ => {}
            }
            pc += width;
        }
        Ok(())
    }
}

pub(super) fn dotted(descriptor: &str) -> String {
    descriptor
        .strip_prefix('L')
        .and_then(|value| value.strip_suffix(';'))
        .unwrap_or(descriptor)
        .replace('/', ".")
}

fn instruction_width(bytes: &[u8], pc: usize, word: usize) -> Result<usize> {
    let opcode = word & 255;
    if opcode == 0 && word != 0 {
        if pc % 2 != 0 {
            bail!("unaligned DEX instruction payload")
        }
        let size = u16_at(bytes, (pc + 1) * 2)?;
        return match word >> 8 {
            1 => Ok(4 + size * 2),
            2 => Ok(2 + size * 4),
            3 => {
                if !matches!(size, 1 | 2 | 4 | 8) {
                    bail!("invalid DEX array payload element width")
                }
                let count = u32_at(bytes, (pc + 2) * 2)?;
                size.checked_mul(count)
                    .and_then(|size| size.checked_add(1))
                    .and_then(|size| (size / 2).checked_add(4))
                    .context("DEX array payload overflow")
            }
            _ => bail!("unknown DEX instruction payload"),
        };
    }
    Ok(match opcode {
        0x00
        | 0x01
        | 0x04
        | 0x07
        | 0x0a..=0x12
        | 0x1d
        | 0x1e
        | 0x21
        | 0x27
        | 0x28
        | 0x7b..=0x8f
        | 0xb0..=0xcf => 1,
        0x02
        | 0x05
        | 0x08
        | 0x13
        | 0x15
        | 0x16
        | 0x19
        | 0x1a
        | 0x1c
        | 0x1f
        | 0x20
        | 0x22
        | 0x23
        | 0x29
        | 0x2d..=0x3d
        | 0x44..=0x6d
        | 0x90..=0xaf
        | 0xd0..=0xe2
        | 0xfe
        | 0xff => 2,
        0x03
        | 0x06
        | 0x09
        | 0x14
        | 0x17
        | 0x1b
        | 0x24..=0x26
        | 0x2a..=0x2c
        | 0x6e..=0x72
        | 0x74..=0x78
        | 0xfc
        | 0xfd => 3,
        0xfa | 0xfb => 4,
        0x18 => 5,
        _ => bail!("unsupported or reserved DEX opcode 0x{opcode:02x}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/apk/classes.dex");

    #[test]
    fn indexes_modified_utf8_overloads_and_real_instruction_references() -> Result<()> {
        let dex = Dex::parse(FIXTURE)?;
        assert!(dex.strings.iter().any(|value| value == "needle\0🐈"));
        let overloads: Vec<_> = dex
            .methods
            .iter()
            .filter(|method| method.class == "example.Target" && method.name == "touch")
            .collect();
        assert_eq!(overloads.len(), 2);
        assert!(overloads
            .iter()
            .any(|method| method.prototype == "(Ljava/lang/String;)V"));
        let mut methods = Vec::new();
        dex.references(
            Query::Method {
                class: "example.Target",
                name: "touch",
                prototype: "(Ljava/lang/String;)V",
            },
            |reference| {
                methods.push(serde_json::to_value(reference).expect("reference serialization"));
            },
        )?;
        assert_eq!(methods.len(), 1);
        assert!(methods[0]["source"]
            .as_str()
            .is_some_and(|source| source.contains("->work(")));
        assert_eq!(methods[0]["kind"], "invoke");
        assert!(methods[0]["instruction_offset"].is_number());
        let mut kinds = std::collections::HashSet::new();
        dex.references(Query::Class("example.Target"), |reference| {
            kinds.insert(reference.kind);
        })?;
        for kind in [
            "field_type",
            "method_type",
            "type_instruction",
            "invoke_class",
        ] {
            assert!(kinds.contains(kind), "missing {kind}");
        }
        Ok(())
    }

    #[test]
    fn accepts_strings_well_above_the_former_bound_and_rejects_beyond_the_current_one() -> Result<()>
    {
        // Obfuscator class maps embedded as single DEX strings exceed 8,192 units in real APKs, and the
        // largest measured in the field is 475,507 units.
        for units in [8193usize, 10_790, 475_507, MAX_STRING_UNITS] {
            let mut bytes = Vec::with_capacity(units + 8);
            let mut value = units;
            while value >= 128 {
                bytes.push(0x80 | (value & 127) as u8);
                value >>= 7;
            }
            bytes.push(value as u8);
            bytes.extend(std::iter::repeat_n(b'a', units));
            bytes.push(0);
            assert_eq!(string(&bytes, 0)?, "a".repeat(units));
        }
        let mut bytes = vec![0x80 | (MAX_STRING_UNITS & 127) as u8];
        let mut value = MAX_STRING_UNITS >> 7;
        while value >= 128 {
            bytes.push(0x80 | (value & 127) as u8);
            value >>= 7;
        }
        bytes.push(value as u8);
        bytes.extend(std::iter::repeat_n(b'a', MAX_STRING_UNITS + 1));
        bytes.push(0);
        assert!(string(&bytes, 0).is_err());
        Ok(())
    }

    #[test]
    fn refuses_truncated_tables_overflowing_lengths_and_invalid_payloads() -> Result<()> {
        for length in 0..FIXTURE.len() {
            assert!(Dex::parse(&FIXTURE[..length]).is_err());
        }
        let mut corrupted = FIXTURE.to_vec();
        corrupted[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Dex::parse(&corrupted).is_err());
        let mut position = 0;
        assert!(uleb(&[0xff; 5], &mut position).is_err());
        assert!(string(&[1, 0xe0, 0x80, 0x80, 0], 0).is_err());
        assert!(instruction_width(&[0, 4, 0, 0], 0, 0x400).is_err());
        assert!(instruction_width(&[0, 1, 0, 0], 1, 0x100).is_err());
        Ok(())
    }
}
