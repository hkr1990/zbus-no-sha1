use std::borrow::Cow;

use crate::{EncodingFormat, SharedData};
use crate::{Variant, VariantError, VariantType, VariantTypeConstants};

#[derive(Debug, Clone)]
pub struct Structure(Vec<Variant>);

impl Structure {
    pub fn take_fields(self) -> Vec<Variant> {
        self.0
    }

    pub fn fields(&self) -> &[Variant] {
        &self.0
    }

    pub fn new() -> Self {
        Self(vec![])
    }

    pub fn add_field<T>(mut self, field: T) -> Self
    where
        T: VariantType,
    {
        self.0.push(field.to_variant());

        self
    }
}

impl VariantTypeConstants for Structure {
    // The real single character signature for STRUCT is `r` but that's not actually used in practice for D-Bus at least
    // (the spec clearly states that this signature must never appear on the bus). The openning and closing braces are
    // used in practice and that's why we'll declare the opening brace as the signature for this type.
    const SIGNATURE_CHAR: char = '(';
    const SIGNATURE_STR: &'static str = "(";
    const ALIGNMENT: usize = 8;
}

impl VariantType for Structure {
    fn signature_char() -> char {
        Self::SIGNATURE_CHAR
    }
    fn signature_str() -> &'static str {
        Self::SIGNATURE_STR
    }
    fn alignment() -> usize {
        Self::ALIGNMENT
    }

    fn encode_into(&self, bytes: &mut Vec<u8>, format: EncodingFormat) {
        Self::add_padding(bytes, format);

        // Since a Structure always starts at 8-byte boundry, the fields and their children are
        // already aligned correctly.
        for field in &self.0 {
            field.encode_value_into(bytes, format);
        }
    }

    fn slice_data(
        data: &SharedData,
        signature: &str,
        format: EncodingFormat,
    ) -> Result<SharedData, VariantError> {
        let padding = Self::padding(data.position(), format);
        if data.len() < padding || signature.len() < 3 {
            return Err(VariantError::InsufficientData);
        }
        Self::ensure_correct_signature(signature)?;

        let mut extracted = padding;
        let mut i = 1;
        let last_index = signature.len() - 1;
        while i < last_index {
            let child_signature = crate::variant_type::slice_signature(&signature[i..last_index])?;
            let slice = crate::variant_type::slice_data(
                &data.tail(extracted as usize),
                child_signature,
                format,
            )?;
            extracted += slice.len();
            if extracted > data.len() {
                return Err(VariantError::InsufficientData);
            }

            i += child_signature.len();
        }
        if extracted == 0 {
            return Err(VariantError::ExcessData);
        }

        Ok(data.head(extracted))
    }

    fn decode(
        data: &SharedData,
        signature: &str,
        format: EncodingFormat,
    ) -> Result<Self, VariantError> {
        // Similar to slice_data, except we create variants.
        let padding = Self::padding(data.position(), format);
        if data.len() < padding || signature.len() < 3 {
            return Err(VariantError::InsufficientData);
        }
        Self::ensure_correct_signature(signature)?;

        let encoding = data.tail(padding);
        let fields = variants_from_struct_data(&encoding, signature, format)?;

        Ok(Self(fields))
    }

    fn ensure_correct_signature(signature: &str) -> Result<(), VariantError> {
        if !signature.starts_with("(") || !signature.ends_with(")") {
            return Err(VariantError::IncorrectType);
        }

        let mut i = 1;
        while i < signature.len() - 1 {
            // Ensure we've only valid child signatures
            let child_signature = crate::variant_type::slice_signature(&signature[i..])?;
            i += child_signature.len();
        }

        Ok(())
    }

    fn signature<'b>(&'b self) -> Cow<'b, str> {
        let mut signature = String::from("(");
        for field in &self.0 {
            signature.push_str(&field.value_signature());
        }
        signature.push_str(")");
        Cow::from(signature)
    }

    fn slice_signature(signature: &str) -> Result<&str, VariantError> {
        if !signature.starts_with("(") {
            return Err(VariantError::IncorrectType);
        }

        let mut open_braces = 1;
        let mut i = 1;
        while i < signature.len() {
            if &signature[i..i + 1] == ")" {
                open_braces -= 1;

                if open_braces == 0 {
                    break;
                }
            } else if &signature[i..i + 1] == "(" {
                open_braces += 1;
            }

            i += 1;
        }
        if &signature[i..i + 1] != ")" {
            return Err(VariantError::IncorrectType);
        }

        Ok(&signature[0..i + 1])
    }

    fn is(variant: &Variant) -> bool {
        if let Variant::Structure(_) = variant {
            true
        } else {
            false
        }
    }

    fn take_from_variant(variant: Variant) -> Result<Self, VariantError> {
        if let Variant::Structure(value) = variant {
            Ok(value)
        } else {
            Err(VariantError::IncorrectType)
        }
    }

    fn from_variant(variant: &Variant) -> Result<&Self, VariantError> {
        if let Variant::Structure(value) = variant {
            Ok(value)
        } else {
            Err(VariantError::IncorrectType)
        }
    }

    fn to_variant(self) -> Variant {
        Variant::Structure(self)
    }
}

fn variants_from_struct_data(
    data: &SharedData,
    signature: &str,
    format: EncodingFormat,
) -> Result<Vec<Variant>, VariantError> {
    // Assuming simple types here but it's OK to have more capacity than needed
    let mut fields = Vec::with_capacity(signature.len());
    let mut extracted = 0;
    let mut i = 1;
    let last_index = signature.len() - 1;
    while i < last_index {
        let child_signature = crate::slice_signature(&signature[i..last_index])?;

        // FIXME: Redundant slicing since Variant::from_data() does slicing too (maybe that function should return the
        // len or slice as well?)
        let child_slice =
            crate::variant_type::slice_data(&data.tail(extracted), child_signature, format)?;
        extracted += child_slice.len();
        if extracted > data.len() {
            return Err(VariantError::InsufficientData);
        }
        let variant = Variant::from_data(&child_slice, child_signature, format)?;
        fields.push(variant);

        i += child_signature.len();
    }
    if extracted == 0 {
        return Err(VariantError::ExcessData);
    }

    Ok(fields)
}
