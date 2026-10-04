//! BER → DER normalization for CMS input.
//!
//! The RustCrypto `der` crate reads DER only, but S/MIME producers stream
//! CMS in BER: indefinite lengths and constructed (chunked) OCTET STRING values
//! for the content (Thunderbird / NSS, Outlook, `openssl cms -stream`).
//! This module rewrites such input into DER without changing any value:
//!
//! * indefinite lengths become definite (minimal) lengths;
//! * a constructed OCTET STRING becomes one primitive OCTET STRING;
//! * the `encryptedContent [0] IMPLICIT OCTET STRING` of an
//!   `EncryptedContentInfo` (a SEQUENCE of OID, `AlgorithmIdentifier`,
//!   constructed `[0]` of OCTET STRING chunks) becomes primitive `[0]`.
//!
//! DER input comes out byte for byte identical, so signed attributes and
//! embedded certificates keep their exact encodings.

use crate::error::{Error, Result};

const MAX_DEPTH: usize = 64;

enum Node<'a> {
    Primitive {
        tag: &'a [u8],
        value: &'a [u8],
    },
    Constructed {
        tag: &'a [u8],
        children: Vec<Node<'a>>,
    },
}

/// Rewrites BER into DER (see the module docs). Trailing zero bytes after
/// the outermost element are ignored; anything else after it is an error.
pub fn normalize(input: &[u8]) -> Result<Vec<u8>> {
    let (node, rest) = parse(input, 0)?;
    if rest.iter().any(|b| *b != 0) {
        return Err(Error::malformed("trailing data after the ASN.1 element"));
    }
    let mut out = Vec::with_capacity(input.len());
    encode(&node, &mut out)?;
    Ok(out)
}

fn parse(input: &[u8], depth: usize) -> Result<(Node<'_>, &[u8])> {
    if depth > MAX_DEPTH {
        return Err(Error::malformed("ASN.1 nesting too deep"));
    }
    let short = || Error::malformed("truncated ASN.1 data");
    let first = *input.first().ok_or_else(short)?;
    let mut tag_len = 1;
    if first & 0x1f == 0x1f {
        loop {
            let b = *input.get(tag_len).ok_or_else(short)?;
            tag_len += 1;
            if b & 0x80 == 0 {
                break;
            }
            if tag_len > 5 {
                return Err(Error::malformed("ASN.1 tag too long"));
            }
        }
    }
    let tag = &input[..tag_len];
    let constructed = first & 0x20 != 0;
    let len_byte = *input.get(tag_len).ok_or_else(short)?;
    let mut pos = tag_len + 1;
    if len_byte == 0x80 {
        if !constructed {
            return Err(Error::malformed("indefinite length on a primitive"));
        }
        let mut children = Vec::new();
        let mut rest = &input[pos..];
        loop {
            if rest.len() >= 2 && rest[0] == 0 && rest[1] == 0 {
                rest = &rest[2..];
                break;
            }
            if rest.is_empty() {
                return Err(short());
            }
            let (child, after) = parse(rest, depth + 1)?;
            children.push(child);
            rest = after;
        }
        return Ok((Node::Constructed { tag, children }, rest));
    }
    let len = if len_byte & 0x80 == 0 {
        usize::from(len_byte)
    } else {
        let n = usize::from(len_byte & 0x7f);
        if n > 4 {
            return Err(Error::malformed("ASN.1 length too large"));
        }
        let bytes = input.get(pos..pos + n).ok_or_else(short)?;
        pos += n;
        bytes
            .iter()
            .fold(0usize, |acc, b| (acc << 8) | usize::from(*b))
    };
    let end = pos.checked_add(len).ok_or_else(short)?;
    let value = input.get(pos..end).ok_or_else(short)?;
    let rest = &input[end..];
    if !constructed {
        return Ok((Node::Primitive { tag, value }, rest));
    }
    let mut children = Vec::new();
    let mut inner = value;
    while !inner.is_empty() {
        let (child, after) = parse(inner, depth + 1)?;
        children.push(child);
        inner = after;
    }
    Ok((Node::Constructed { tag, children }, rest))
}

/// One DER element: its first tag byte, the whole encoding and the value.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tlv<'a> {
    /// The first identifier octet.
    pub tag: u8,
    /// Tag, length and value.
    pub full: &'a [u8],
    /// The value.
    pub value: &'a [u8],
}

/// The first DER element of `input` (definite lengths, single-byte tags:
/// enough to walk the CMS structure after [`normalize`]).
pub(crate) fn tlv(input: &[u8]) -> Result<(Tlv<'_>, &[u8])> {
    let first = *input
        .first()
        .ok_or_else(|| Error::malformed("truncated DER data"))?;
    if first & 0x1f == 0x1f {
        return Err(Error::malformed("unexpected multi-byte tag"));
    }
    let value = shallow_value(input)?;
    let start = value.as_ptr() as usize - input.as_ptr() as usize;
    let end = start + value.len();
    Ok((
        Tlv {
            tag: first,
            full: &input[..end],
            value,
        },
        &input[end..],
    ))
}

/// The value of a definite-length element (single-byte tag).
fn shallow_value(input: &[u8]) -> Result<&[u8]> {
    let short = || Error::malformed("truncated DER data");
    let len_byte = *input.get(1).ok_or_else(short)?;
    let (len, start) = if len_byte & 0x80 == 0 {
        (usize::from(len_byte), 2)
    } else {
        let n = usize::from(len_byte & 0x7f);
        if n == 0 || n > 4 {
            return Err(Error::malformed("unsupported DER length"));
        }
        let bytes = input.get(2..2 + n).ok_or_else(short)?;
        (
            bytes
                .iter()
                .fold(0usize, |acc, b| (acc << 8) | usize::from(*b)),
            2 + n,
        )
    };
    input.get(start..start + len).ok_or_else(short)
}

/// The DER elements inside a constructed value.
pub(crate) fn children(mut value: &[u8]) -> Result<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    while !value.is_empty() {
        let (child, rest) = tlv(value)?;
        out.push(child);
        value = rest;
    }
    Ok(out)
}

fn write_len(len: usize, out: &mut Vec<u8>) {
    let bytes = len.to_be_bytes();
    let skip = bytes.iter().take_while(|b| **b == 0).count();
    let significant = &bytes[skip..];
    match significant {
        [] => out.push(0),
        [short] if *short < 0x80 => out.push(*short),
        _ => {
            // Long form: the count of length bytes (at most
            // `size_of::<usize>()`), then the bytes.
            let count = u8::try_from(significant.len()).unwrap_or(u8::MAX);
            out.push(0x80 | count);
            out.extend_from_slice(significant);
        }
    }
}

/// Concatenates the contents of (possibly nested) OCTET STRING chunks.
fn octets(children: &[Node<'_>], out: &mut Vec<u8>) -> Result<()> {
    for child in children {
        match child {
            Node::Primitive { tag: [0x04], value } => out.extend_from_slice(value),
            Node::Constructed {
                tag: [0x24],
                children,
            } => octets(children, out)?,
            _ => {
                return Err(Error::malformed(
                    "a chunked OCTET STRING holds something else",
                ));
            }
        }
    }
    Ok(())
}

fn is_octet_chunks(children: &[Node<'_>]) -> bool {
    children.iter().all(|c| {
        matches!(
            c,
            Node::Primitive { tag: [0x04], .. } | Node::Constructed { tag: [0x24], .. }
        )
    })
}

fn encode(node: &Node<'_>, out: &mut Vec<u8>) -> Result<()> {
    match node {
        Node::Primitive { tag, value } => {
            out.extend_from_slice(tag);
            write_len(value.len(), out);
            out.extend_from_slice(value);
        }
        Node::Constructed {
            tag: [0x24],
            children,
        } => {
            let mut value = Vec::new();
            octets(children, &mut value)?;
            out.push(0x04);
            write_len(value.len(), out);
            out.extend_from_slice(&value);
        }
        Node::Constructed { tag: [0x23], .. } => {
            return Err(Error::Unsupported("constructed BIT STRING".into()));
        }
        Node::Constructed { tag, children } => {
            let mut value = Vec::new();
            let encrypted_content_info = tag == &[0x30]
                && children.len() == 3
                && matches!(children[0], Node::Primitive { tag: [0x06], .. })
                && matches!(children[1], Node::Constructed { tag: [0x30], .. });
            for (i, child) in children.iter().enumerate() {
                match child {
                    Node::Constructed {
                        tag: [0xa0],
                        children: chunks,
                    } if encrypted_content_info && i == 2 && is_octet_chunks(chunks) => {
                        let mut content = Vec::new();
                        octets(chunks, &mut content)?;
                        value.push(0x80);
                        write_len(content.len(), &mut value);
                        value.extend_from_slice(&content);
                    }
                    _ => encode(child, &mut value)?,
                }
            }
            out.extend_from_slice(tag);
            write_len(value.len(), out);
            out.extend_from_slice(&value);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_passes_through_unchanged() {
        let der = [
            0x30, 0x0a, 0x06, 0x03, 0x2a, 0x03, 0x04, 0x04, 0x03, 0x01, 0x02, 0x03,
        ];
        assert_eq!(normalize(&der).unwrap(), der);
        // Long-form lengths stay long when needed.
        let mut big = vec![0x04, 0x81, 0x80];
        big.extend(std::iter::repeat_n(7u8, 128));
        assert_eq!(normalize(&big).unwrap(), big);
    }

    #[test]
    fn indefinite_lengths_and_chunks_become_der() {
        // SEQUENCE (indefinite) { OCTET STRING constructed (indefinite)
        // { "ab", "c" } }
        let ber = [
            0x30, 0x80, 0x24, 0x80, 0x04, 0x02, b'a', b'b', 0x04, 0x01, b'c', 0x00, 0x00, 0x00,
            0x00,
        ];
        assert_eq!(
            normalize(&ber).unwrap(),
            [0x30, 0x05, 0x04, 0x03, b'a', b'b', b'c']
        );
    }

    #[test]
    fn encrypted_content_becomes_primitive_implicit_octets() {
        // SEQUENCE { OID, SEQUENCE { OID }, [0] constructed { "xy", "z" } }
        let ber = [
            0x30, 0x80, 0x06, 0x01, 0x2a, 0x30, 0x03, 0x06, 0x01, 0x2b, 0xa0, 0x80, 0x04, 0x02,
            b'x', b'y', 0x04, 0x01, b'z', 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(
            normalize(&ber).unwrap(),
            [
                0x30, 0x0d, 0x06, 0x01, 0x2a, 0x30, 0x03, 0x06, 0x01, 0x2b, 0x80, 0x03, b'x', b'y',
                b'z'
            ]
        );
        // An explicit [0] (second position, as in EncapsulatedContentInfo)
        // is left alone.
        let explicit = [0x30, 0x08, 0x06, 0x01, 0x2a, 0xa0, 0x03, 0x04, 0x01, b'q'];
        assert_eq!(normalize(&explicit).unwrap(), explicit);
    }

    #[test]
    fn malformed_input_is_rejected() {
        assert!(normalize(&[]).is_err());
        assert!(normalize(&[0x04, 0x05, 1, 2]).is_err()); // truncated
        assert!(normalize(&[0x04, 0x80, 0, 0]).is_err()); // indefinite primitive
        assert!(normalize(&[0x30, 0x80, 0x04, 0x01, 1]).is_err()); // no end-of-contents
        assert!(normalize(&[0x04, 0x01, 1, 0x05]).is_err()); // trailing data
        assert_eq!(normalize(&[0x04, 0x01, 1, 0, 0]).unwrap(), [0x04, 0x01, 1]);
        assert!(normalize(&[0x04, 0x85, 1, 1, 1, 1, 1]).is_err()); // huge length
        let mut deep = Vec::new();
        for _ in 0..100 {
            deep.extend_from_slice(&[0x30, 0x80]);
        }
        assert!(normalize(&deep).is_err());
        // A multi-byte tag parses.
        assert_eq!(
            normalize(&[0x9f, 0x81, 0x01, 0x01, 0x07]).unwrap(),
            [0x9f, 0x81, 0x01, 0x01, 0x07]
        );
        // A constructed OCTET STRING with a non-OCTET child fails.
        assert!(normalize(&[0x24, 0x03, 0x02, 0x01, 0x01]).is_err());
    }
}
