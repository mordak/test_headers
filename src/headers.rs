use nom::{
    branch::alt,
    bytes::complete::tag as complete_tag,
    bytes::streaming::{tag, take_till},
    character::streaming::{space0, space1},
    combinator::{map, not, peek},
    sequence::tuple,
    IResult,
};

#[derive(Debug, PartialEq)]
pub struct Name {
    pub name: Vec<u8>,
    pub flags: u8,
}

#[derive(Debug, PartialEq)]
pub struct Value {
    pub value: Vec<u8>,
    pub flags: u8,
}

#[derive(Debug, PartialEq)]
pub struct Header {
    pub name: Name,
    pub value: Value,
}

/// Parse one header name up to the :
fn name(input: &[u8]) -> IResult<&[u8], Name> {
    map(
        tuple((not(space1), take_till(|c| c == b':'))),
        |(_, n): (_, &[u8])| Name {
            name: n.into(),
            flags: 0,
        },
    )(input)
}

/// Parse one complete end of line character or character set
fn complete_eol(input: &[u8]) -> IResult<&[u8], &[u8]> {
    alt((
        complete_tag(b"\n\r\r\n"),
        complete_tag(b"\r\n"),
        complete_tag(b"\n"),
        complete_tag(b"\r"),
    ))(input)
}

/// Parse one header end of line, and guarantee that it is not folding
fn eol(input: &[u8]) -> IResult<&[u8], &[u8]> {
    map(tuple((complete_eol, peek(not(space1)))), |(end, _)| end)(input)
}

/// Test if the byte is CR or LF
fn is_eol(c: u8) -> bool {
    c == b'\r' || c == b'\n'
}

/// Parse header folding bytes (eol + whitespace)
fn folding(input: &[u8]) -> IResult<&[u8], (&[u8], &[u8])> {
    tuple((complete_eol, space1))(input)
}

/// Parse folding bytes or an eol
fn folding_or_eol(input: &[u8]) -> IResult<&[u8], (&[u8], Option<&[u8]>)> {
    if let Ok((rest, (end, fold))) = folding(input) {
        Ok((rest, (end, Some(fold))))
    } else {
        map(eol, |end| (end, None))(input)
    }
}

/// Parse a header value.
/// Returns the bytes and the value terminator, either eol or folding
/// eg. (bytes, (eol_bytes, Option<fold_bytes>))
fn value_bytes(input: &[u8]) -> IResult<&[u8], (&[u8], (&[u8], Option<&[u8]>))> {
    tuple((take_till(is_eol), folding_or_eol))(input)
}

/// Parse a complete header value, including any folded headers
fn value(input: &[u8]) -> IResult<&[u8], Value> {
    let (rest, (val_bytes, (_eol, fold))) = value_bytes(input)?;

    let mut value = val_bytes.to_vec();
    if fold.is_none() {
        Ok((rest, Value { value, flags: 0 }))
    } else {
        let mut i = rest;
        loop {
            match value_bytes(i) {
                Ok((rest, (val_bytes, (_eol, fold)))) => {
                    i = rest;
                    value.push(b' ');
                    value.extend(val_bytes);
                    if fold.is_none() {
                        return Ok((rest, Value { value, flags: 0 }));
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Parse a separator (colon + space) between header name and value
fn separator(input: &[u8]) -> IResult<&[u8], (&[u8], &[u8])> {
    tuple((tag(b":"), space0))(input)
}

/// Parse a header name: value
fn header(input: &[u8]) -> IResult<&[u8], Header> {
    map(tuple((name, separator, value)), |(name, _, value)| Header {
        name,
        value,
    })(input)
}

/// Parse multiple headers and indicate if end of headers was found
pub fn headers(input: &[u8]) -> IResult<&[u8], (Vec<Header>, bool)> {
    let (rest, head) = header(input)?;
    let mut out = Vec::with_capacity(16);
    out.push(head);
    if let Ok((rest, _eoh)) = complete_eol(rest) {
        return Ok((rest, (out, true)));
    }
    let mut i = rest;
    loop {
        match header(i) {
            Ok((rest, head)) => {
                i = rest;
                out.push(head);
                if let Ok((rest, _eoh)) = complete_eol(rest) {
                    return Ok((rest, (out, true)));
                }
            }
            Err(nom::Err::Incomplete(_)) => return Ok((rest, (out, false))),
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    macro_rules! b {
        ($b: literal) => {
            $b.as_bytes()
        };
    }

    #[test]
    fn test_headers() {
        assert_eq!(
            headers(b"k1:v1\r\n:v2\r\n v2+\r\nk3: v3\r\n\r\n"),
            Ok((
                b!(""),
                (
                    vec![
                        Header {
                            name: Name {
                                name: b"k1".to_vec(),
                                flags: 0
                            },
                            value: Value {
                                value: b"v1".to_vec(),
                                flags: 0
                            },
                        },
                        Header {
                            name: Name {
                                name: b"".to_vec(),
                                flags: 0
                            },
                            value: Value {
                                value: b"v2 v2+".to_vec(),
                                flags: 0
                            },
                        },
                        Header {
                            name: Name {
                                name: b"k3".to_vec(),
                                flags: 0
                            },
                            value: Value {
                                value: b"v3".to_vec(),
                                flags: 0
                            },
                        }
                    ],
                    true
                ),
            ))
        );
        assert_eq!(
            headers(b"k1:v1\r\nk2:v2\r"),
            Ok((
                b!("k2:v2\r"),
                (
                    vec![Header {
                        name: Name {
                            name: b"k1".to_vec(),
                            flags: 0
                        },
                        value: Value {
                            value: b"v1".to_vec(),
                            flags: 0
                        },
                    },],
                    false
                ),
            ))
        );
    }

    #[test]
    fn test_header() {
        assert!(header(b"K: V").is_err());
        assert!(header(b"K: V\r\n").is_err());
        assert_eq!(
            header(b"K: V\r\n\r\n"),
            Ok((
                b!("\r\n"),
                Header {
                    name: Name {
                        name: b"K".to_vec(),
                        flags: 0
                    },
                    value: Value {
                        value: b"V".to_vec(),
                        flags: 0
                    },
                }
            ))
        );
        assert_eq!(
            header(b"K: V\r\n a\r\n l\r\n u\r\n\te\r\n\r\n"),
            Ok((
                b!("\r\n"),
                Header {
                    name: Name {
                        name: b"K".to_vec(),
                        flags: 0
                    },
                    value: Value {
                        value: b"V a l u e".to_vec(),
                        flags: 0
                    },
                }
            ))
        );
    }

    #[test]
    fn test_separator() {
        assert!(separator(b" : ").is_err());
        assert!(separator(b" ").is_err());
        assert!(separator(b": value").is_ok());
        assert!(separator(b":value").is_ok());
        assert!(separator(b": value").is_ok());
        assert_eq!(separator(b":value"), Ok((b!("value"), (b!(":"), b!("")))));
        assert_eq!(separator(b": value"), Ok((b!("value"), (b!(":"), b!(" ")))));
        assert_eq!(
            separator(b":\t value"),
            Ok((b!("value"), (b!(":"), b!("\t "))))
        );
    }

    #[test]
    fn test_name() {
        assert_eq!(name(b"Hello: world").unwrap().1.name, b"Hello".to_vec());
        assert_eq!(name(b": world").unwrap().1.name, b"".to_vec());
        assert!(name(b" Hello: world").is_err());
        assert!(name(b"Hello").is_err());
    }

    #[test]
    fn test_eol() {
        assert!(eol(b"test").is_err());
        assert!(eol(b"\r\n").is_err());
        assert!(eol(b"\n").is_err());
        assert!(eol(b"\r\n ").is_err());
        assert!(eol(b"\r\n\t").is_err());
        assert!(eol(b"\r\n\t ").is_err());
        assert_eq!(eol(b"\ra"), Ok((b!("a"), b!("\r"))));
        assert_eq!(eol(b"\na"), Ok((b!("a"), b!("\n"))));
        assert_eq!(eol(b"\n\r\r\na"), Ok((b!("a"), b!("\n\r\r\n"))));
        assert_eq!(eol(b"\r\n\r\na"), Ok((b!("\r\na"), b!("\r\n"))));

        assert!(complete_eol(b"test").is_err());
        assert!(complete_eol(b"\r\n").is_ok());
        assert!(complete_eol(b"\n").is_ok());
        assert_eq!(complete_eol(b"\r\n"), Ok((b!(""), b!("\r\n"))));
        assert_eq!(complete_eol(b"\r"), Ok((b!(""), b!("\r"))));
        assert_eq!(complete_eol(b"\n"), Ok((b!(""), b!("\n"))));
        assert_eq!(complete_eol(b"\n\r\r\n"), Ok((b!(""), b!("\n\r\r\n"))));
        assert_eq!(complete_eol(b"\r\n\r\n"), Ok((b!("\r\n"), b!("\r\n"))));
    }

    #[test]
    fn test_is_eol() {
        assert!(is_eol(b'\r'));
        assert!(is_eol(b'\n'));
        assert!(!is_eol(b'\t'));
        assert!(!is_eol(b' '));
    }

    #[test]
    fn test_folding() {
        assert!(folding(b"test").is_err());
        assert!(folding(b"\r\n").is_err());
        assert!(folding(b"\r\n ").is_err());
        assert!(folding(b"\r\n\t").is_err());
        assert!(folding(b"\r\n\t ").is_err());
        assert!(folding(b"\r\n \t").is_err());
        assert_eq!(
            folding(b"\r\n next"),
            Ok((b!("next"), (b!("\r\n"), b!(" "))))
        );
        assert_eq!(
            folding(b"\r\n\tnext"),
            Ok((b!("next"), (b!("\r\n"), b!("\t"))))
        );
        assert_eq!(
            folding(b"\r\n\t next"),
            Ok((b!("next"), (b!("\r\n"), b!("\t "))))
        );
        assert_eq!(
            folding(b"\r\n\t\t\r\n"),
            Ok((b!("\r\n"), (b!("\r\n"), b!("\t\t"))))
        );
        assert_eq!(
            folding(b"\r\n\t \t\r"),
            Ok((b!("\r"), (b!("\r\n"), b!("\t \t"))))
        );
        assert_eq!(
            folding(b"\r\n     \n"),
            Ok((b!("\n"), (b!("\r\n"), b!("     "))))
        );
    }

    #[test]
    fn test_folding_or_eol() {
        // All of these fail because they are incomplete.
        // We need more bytes before we can get the full fold
        // or decide there is no fold.
        assert!(folding_or_eol(b"\r\n").is_err());
        assert!(folding_or_eol(b"\r\n\t").is_err());
        assert!(folding_or_eol(b"\r\n ").is_err());

        assert_eq!(
            folding_or_eol(b"\r\n\ta"),
            Ok((b!("a"), (b!("\r\n"), Some(b!("\t")))))
        );
        assert_eq!(
            folding_or_eol(b"\r\n a"),
            Ok((b!("a"), (b!("\r\n"), Some(b!(" ")))))
        );
        assert_eq!(folding_or_eol(b"\r\na"), Ok((b!("a"), (b!("\r\n"), None))));
        assert_eq!(folding_or_eol(b"\n\na"), Ok((b!("\na"), (b!("\n"), None))));
        assert_eq!(
            folding_or_eol(b"\r\n\r\na"),
            Ok((b!("\r\na"), (b!("\r\n"), None)))
        );
    }

    #[test]
    fn test_value_bytes() {
        // Expect fail because we need to see EOL
        assert!(value_bytes(b" ").is_err());
        assert!(value_bytes(b"value").is_err());
        assert!(value_bytes(b"\tvalue").is_err());
        assert!(value_bytes(b" value").is_err());
        // Expect fail because we need to see past EOL to check for folding
        assert!(value_bytes(b"value\r\n").is_err());

        assert_eq!(
            value_bytes(b"\r\nnext"),
            Ok((b!("next"), (b!(""), (b!("\r\n"), None))))
        );
        assert_eq!(
            value_bytes(b"value\r\nname2"),
            Ok((b!("name2"), (b!("value"), (b!("\r\n"), None))))
        );
        assert_eq!(
            value_bytes(b"value\n more"),
            Ok((b!("more"), (b!("value"), (b!("\n"), Some(b!(" "))))))
        );
        assert_eq!(
            value_bytes(b"value\r\n\t more"),
            Ok((b!("more"), (b!("value"), (b!("\r\n"), Some(b!("\t "))))))
        );
        assert_eq!(
            value_bytes(b"value\n\rname2"),
            Ok((b!("\rname2"), (b!("value"), (b!("\n"), None))))
        );
    }

    #[test]
    fn test_value() {
        assert!(value(b"value\r\n more\r\n").is_err());
        assert!(value(b"value\r\n ").is_err());
        assert!(value(b"value\r\n more").is_err());
        assert!(value(b"value\r\n more\n").is_err());
        assert!(value(b"value\n more\r\n").is_err());

        assert_eq!(
            value(b"value\r\nnext:"),
            Ok((
                b!("next:"),
                Value {
                    value: b"value".to_vec(),
                    flags: 0
                }
            ))
        );
        assert_eq!(
            value(b"value\r\n more\r\n\r\n"),
            Ok((
                b!("\r\n"),
                Value {
                    value: b"value more".to_vec(),
                    flags: 0
                }
            ))
        );
        assert_eq!(
            value(b"value\r\n more\r\n\tand more\r\nnext:"),
            Ok((
                b!("next:"),
                Value {
                    value: b"value more and more".to_vec(),
                    flags: 0
                }
            ))
        );
        assert_eq!(
            value(b"value\n more\n\r\r\n\tand more\r\n\r\n"),
            Ok((
                b!("\r\n"),
                Value {
                    value: b"value more and more".to_vec(),
                    flags: 0
                }
            ))
        );
        assert_eq!(
            value(b"value\n\t\tmore\r\n  and\r\n more\r\nnext:"),
            Ok((
                b!("next:"),
                Value {
                    value: b"value more and more".to_vec(),
                    flags: 0
                }
            ))
        );
    }
}
