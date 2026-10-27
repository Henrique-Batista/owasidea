use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeSpec {
    Primitive(String),
    List(Box<TypeSpec>),
    Option(Box<TypeSpec>),
    Result {
        ok: Option<Box<TypeSpec>>,
        err: Option<Box<TypeSpec>>,
    },
    Tuple(Vec<TypeSpec>),
    Named {
        kind: String,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub path: String,
    pub message: String,
    pub position: usize,
    pub snippet: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(
                f,
                "{} at position {}: `{}`",
                self.message, self.position, self.snippet
            )
        } else {
            write!(
                f,
                "{} at {}: position {} `{}`",
                self.message, self.path, self.position, self.snippet
            )
        }
    }
}

impl std::error::Error for ParseError {}

const PRIMITIVES: &[&str] = &[
    "bool", "u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64", "f32", "f64", "char", "string",
    "void", "resource", "handle",
];

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.input.len() {
            let c = self.input.as_bytes()[self.pos] as char;
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.input.len() {
            let c = self.input.as_bytes()[self.pos] as char;
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.input[start..self.pos].to_string()
    }

    fn expect_char(&mut self, ch: char) -> Result<(), ParseError> {
        self.skip_ws();
        match self.peek_char() {
            Some(c) if c == ch => {
                self.pos += ch.len_utf8();
                Ok(())
            }
            _ => Err(self.error(&format!("expected `{}`", ch))),
        }
    }

    fn error(&self, msg: &str) -> ParseError {
        let snippet: String = self.input[self.pos..].chars().take(20).collect();
        ParseError {
            path: String::new(),
            message: msg.to_string(),
            position: self.pos,
            snippet,
        }
    }

    fn parse_type(&mut self) -> Result<TypeSpec, ParseError> {
        self.skip_ws();
        if self.pos >= self.input.len() {
            return Err(self.error("expected type"));
        }
        let first = self.input.as_bytes()[self.pos] as char;
        if !first.is_ascii_alphabetic() && first != '_' {
            return Err(self.error("expected identifier"));
        }
        let ident = self.read_ident();
        match ident.as_str() {
            "list" => {
                self.skip_ws();
                self.expect_char('<')?;
                let inner = self.parse_type()?;
                self.skip_ws();
                self.expect_char('>')?;
                Ok(TypeSpec::List(Box::new(inner)))
            }
            "option" => {
                self.skip_ws();
                self.expect_char('<')?;
                let inner = self.parse_type()?;
                self.skip_ws();
                self.expect_char('>')?;
                Ok(TypeSpec::Option(Box::new(inner)))
            }
            "result" => {
                self.skip_ws();
                if self.peek_char() == Some('<') {
                    self.pos += 1;
                    self.skip_ws();
                    let ok = if self.peek_char() == Some('_') {
                        self.pos += 1;
                        None
                    } else {
                        Some(self.parse_type()?)
                    };
                    self.skip_ws();
                    if self.peek_char() != Some(',') {
                        return Err(self.error("expected `,` in result<T,E>"));
                    }
                    self.pos += 1;
                    self.skip_ws();
                    let err = if self.peek_char() == Some('_') {
                        self.pos += 1;
                        None
                    } else {
                        Some(self.parse_type()?)
                    };
                    self.skip_ws();
                    if self.peek_char() != Some('>') {
                        return Err(self.error("expected `>`"));
                    }
                    self.pos += 1;
                    Ok(TypeSpec::Result {
                        ok: ok.map(Box::new),
                        err: err.map(Box::new),
                    })
                } else {
                    Ok(TypeSpec::Result {
                        ok: None,
                        err: None,
                    })
                }
            }
            "tuple" => {
                self.skip_ws();
                self.expect_char('<')?;
                self.skip_ws();
                if self.peek_char() == Some('>') {
                    return Err(self.error("empty tuple is not allowed"));
                }
                let mut items = vec![self.parse_type()?];
                loop {
                    self.skip_ws();
                    if self.peek_char() == Some(',') {
                        self.pos += 1;
                        self.skip_ws();
                        if self.peek_char() == Some('>') {
                            return Err(self.error("trailing comma in tuple"));
                        }
                        items.push(self.parse_type()?);
                    } else {
                        break;
                    }
                }
                self.skip_ws();
                self.expect_char('>')?;
                Ok(TypeSpec::Tuple(items))
            }
            "record" | "enum" | "flags" | "variant" => {
                let kind = ident;
                self.skip_ws();
                let name = if self.peek_char() == Some('<') {
                    self.pos += 1;
                    self.skip_ws();
                    let n = self.read_ident();
                    self.skip_ws();
                    self.expect_char('>')?;
                    n
                } else {
                    // require at least one whitespace separator when using space form
                    self.read_ident()
                };
                if name.is_empty() {
                    return Err(self.error("expected type name"));
                }
                Ok(TypeSpec::Named { kind, name })
            }
            other => {
                if PRIMITIVES.contains(&other) {
                    Ok(TypeSpec::Primitive(other.to_string()))
                } else {
                    Err(self.error(&format!("unknown type `{}`", other)))
                }
            }
        }
    }
}

pub fn parse_type(input: &str) -> Result<TypeSpec, ParseError> {
    let mut p = Parser::new(input);
    let ty = p.parse_type()?;
    p.skip_ws();
    if p.pos != input.len() {
        return Err(p.error("unexpected trailing characters"));
    }
    Ok(ty)
}

pub fn default_json(ty: &TypeSpec) -> serde_json::Value {
    match ty {
        TypeSpec::Primitive(p) => match p.as_str() {
            "bool" => serde_json::Value::Bool(false),
            "string" | "char" => serde_json::Value::String(String::new()),
            "f32" | "f64" => serde_json::json!(0.0),
            _ => serde_json::json!(0),
        },
        TypeSpec::List(_) => serde_json::json!([]),
        TypeSpec::Option(_) => serde_json::Value::Null,
        TypeSpec::Result { .. } => serde_json::Value::Null,
        TypeSpec::Tuple(items) => {
            serde_json::Value::Array(items.iter().map(default_json).collect())
        }
        TypeSpec::Named { .. } => serde_json::Value::Null,
    }
}

pub fn validate_type(path: &str, input: &str) -> Result<(), ParseError> {
    match parse_type(input) {
        Ok(_) => Ok(()),
        Err(mut e) => {
            e.path = path.to_string();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives() {
        for p in PRIMITIVES {
            assert!(parse_type(p).is_ok(), "failed primitive {}", p);
        }
    }

    #[test]
    fn list_option() {
        assert!(parse_type("list<u32>").is_ok());
        assert!(parse_type("option<string>").is_ok());
        assert!(parse_type("list<list<string>>").is_ok());
    }

    #[test]
    fn result_variants() {
        assert!(parse_type("result").is_ok());
        assert!(parse_type("result<u32,string>").is_ok());
        assert!(parse_type("result<_,string>").is_ok());
        assert!(parse_type("result<u32,_>").is_ok());
        assert!(parse_type("result<list<u32>,option<string>>").is_ok());
    }

    #[test]
    fn tuple() {
        assert!(parse_type("tuple<u32,string>").is_ok());
        assert!(parse_type("tuple<u32>").is_ok());
        assert!(parse_type("tuple<u32,string,bool>").is_ok());
    }

    #[test]
    fn named() {
        assert!(parse_type("record point").is_ok());
        assert!(parse_type("enum color").is_ok());
        assert!(parse_type("flags perms").is_ok());
        assert!(parse_type("variant shape").is_ok());
        assert!(parse_type("record<point>").is_ok());
    }

    #[test]
    fn invalids() {
        assert!(parse_type("list<").is_err());
        assert!(parse_type("result<u32>").is_err());
        assert!(parse_type("record").is_err());
        assert!(parse_type("tuple<>").is_err());
        assert!(parse_type("unknown").is_err());
        assert!(parse_type("").is_err());
        assert!(parse_type("result<u32,>").is_err());
        assert!(parse_type("tuple<u32,>").is_err());
        assert!(parse_type("list<>").is_err());
    }

    #[test]
    fn deep_nesting() {
        let t = "list<option<result<tuple<u32,string>,_>>>";
        assert!(parse_type(t).is_ok());
    }

    #[test]
    fn path_error() {
        let e = validate_type("exports[0].params[1].type", "list<").unwrap_err();
        assert_eq!(e.path, "exports[0].params[1].type");
        assert!(e.to_string().contains("exports[0].params[1].type"));
    }

    #[cfg(test)]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_primitive() -> impl Strategy<Value = String> {
            prop_oneof![
                Just("bool".to_string()),
                Just("u32".to_string()),
                Just("string".to_string()),
                Just("resource".to_string()),
                Just("handle".to_string()),
            ]
        }

        fn arb_type(depth: u32) -> impl Strategy<Value = String> {
            if depth == 0 {
                arb_primitive().boxed()
            } else {
                prop_oneof![
                    arb_primitive(),
                    arb_type(depth - 1).prop_map(|t| format!("list<{}>", t)),
                    arb_type(depth - 1).prop_map(|t| format!("option<{}>", t)),
                    (arb_type(depth - 1), arb_type(depth - 1))
                        .prop_map(|(a, b)| format!("result<{},{}>", a, b)),
                    (arb_type(depth - 1),).prop_map(|(a,)| format!("tuple<{}>", a)),
                    Just("record foo".to_string()),
                    Just("enum bar".to_string()),
                ]
                .boxed()
            }
        }

        proptest! {
            #[test]
            fn valid_types_parse(t in arb_type(3)) {
                prop_assert!(parse_type(&t).is_ok(), "failed to parse {}", t);
            }
        }
    }
}
