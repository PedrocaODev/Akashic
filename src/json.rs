use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Object(Vec<(String, Value)>),
    Array(Vec<Value>),
}

#[derive(Clone, Debug)]
pub(crate) struct Envelope {
    pub(crate) protocol: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) version_present: bool,
    pub(crate) kind: Option<String>,
    pub(crate) id: Option<String>,
    pub(crate) correlation_id: Option<String>,
    pub(crate) correlation_id_present: bool,
    pub(crate) payload: Option<Value>,
    pub(crate) error: Option<Value>,
}

pub(crate) fn parse(input: &str) -> Option<Value> {
    let mut parser = Parser { input, position: 0 };
    let value = parser.value()?;
    parser.whitespace();
    (parser.position == input.len()).then_some(value)
}

pub(crate) fn envelope(input: &str) -> Option<Envelope> {
    let Value::Object(fields) = parse(input)? else {
        return None;
    };
    if fields.iter().any(|(key, _)| {
        !matches!(
            key.as_str(),
            "protocol" | "version" | "kind" | "id" | "correlation_id" | "payload" | "error"
        )
    }) {
        return None;
    }
    Some(Envelope {
        protocol: string_field(&fields, "protocol"),
        version: number_field(&fields, "version"),
        version_present: field(&fields, "version").is_some(),
        kind: string_field(&fields, "kind"),
        id: string_field(&fields, "id"),
        correlation_id: string_field(&fields, "correlation_id"),
        correlation_id_present: field(&fields, "correlation_id").is_some(),
        payload: field(&fields, "payload").cloned(),
        error: field(&fields, "error").cloned(),
    })
}

pub(crate) fn top_level_string_prefix(input: &str, wanted: &str) -> Option<String> {
    let mut parser = Parser { input, position: 0 };
    parser.whitespace();
    parser.consume('{')?;
    loop {
        parser.whitespace();
        if parser.consume('}').is_some() {
            return None;
        }
        let key = parser.string()?;
        parser.whitespace();
        parser.consume(':')?;
        parser.whitespace();
        let value = parser.value()?;
        if key == wanted {
            if let Value::String(value) = value {
                return Some(value);
            }
            return None;
        }
        parser.whitespace();
        if parser.consume('}').is_some() {
            return None;
        }
        parser.consume(',')?;
    }
}

pub(crate) fn object_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let Value::Object(fields) = value else {
        return None;
    };
    field(fields, key)
}

pub(crate) fn string_value(value: &Value) -> Option<&str> {
    let Value::String(value) = value else {
        return None;
    };
    Some(value)
}

pub(crate) fn number_is(value: &Value, expected: &str) -> bool {
    matches!(value, Value::Number(value) if value == expected)
}

fn field<'a>(fields: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    fields
        .iter()
        .find(|(field, _)| field == key)
        .map(|(_, value)| value)
}

fn string_field(fields: &[(String, Value)], key: &str) -> Option<String> {
    string_value(field(fields, key)?).map(str::to_string)
}

fn number_field(fields: &[(String, Value)], key: &str) -> Option<String> {
    let Value::Number(value) = field(fields, key)? else {
        return None;
    };
    Some(value.clone())
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
}

impl Parser<'_> {
    fn value(&mut self) -> Option<Value> {
        self.whitespace();
        match self.peek()? {
            'n' => self.literal("null", Value::Null),
            't' => self.literal("true", Value::Bool(true)),
            'f' => self.literal("false", Value::Bool(false)),
            '"' => self.string().map(Value::String),
            '{' => self.object(),
            '[' => self.array(),
            '-' | '0'..='9' => self.number().map(Value::Number),
            _ => None,
        }
    }

    fn object(&mut self) -> Option<Value> {
        self.consume('{')?;
        let mut fields = Vec::new();
        let mut names = HashSet::new();
        loop {
            self.whitespace();
            if self.consume('}').is_some() {
                return Some(Value::Object(fields));
            }
            let name = self.string()?;
            if !names.insert(name.clone()) {
                return None;
            }
            self.whitespace();
            self.consume(':')?;
            let value = self.value()?;
            fields.push((name, value));
            self.whitespace();
            if self.consume('}').is_some() {
                return Some(Value::Object(fields));
            }
            self.consume(',')?;
            self.whitespace();
            if self.peek() == Some('}') {
                return None;
            }
        }
    }

    fn array(&mut self) -> Option<Value> {
        self.consume('[')?;
        let mut values = Vec::new();
        loop {
            self.whitespace();
            if self.consume(']').is_some() {
                return Some(Value::Array(values));
            }
            values.push(self.value()?);
            self.whitespace();
            if self.consume(']').is_some() {
                return Some(Value::Array(values));
            }
            self.consume(',')?;
            self.whitespace();
            if self.peek() == Some(']') {
                return None;
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        self.consume('"')?;
        let mut value = String::new();
        let mut escaped = false;
        while let Some(character) = self.next() {
            if escaped {
                value.push(match character {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\u{0008}',
                    'f' => '\u{000c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => self.unicode_escape()?,
                    _ => return None,
                });
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                return Some(value);
            } else if character.is_control() {
                return None;
            } else {
                value.push(character);
            }
        }
        None
    }

    fn unicode_escape(&mut self) -> Option<char> {
        let mut digits = String::new();
        for _ in 0..4 {
            digits.push(self.next()?);
        }
        let code = u32::from_str_radix(&digits, 16).ok()?;
        if (0xd800..=0xdbff).contains(&code) {
            if self.next()? != '\\' || self.next()? != 'u' {
                return None;
            }
            let mut low_digits = String::new();
            for _ in 0..4 {
                low_digits.push(self.next()?);
            }
            let low = u32::from_str_radix(&low_digits, 16).ok()?;
            if !(0xdc00..=0xdfff).contains(&low) {
                return None;
            }
            return char::from_u32(0x1_0000 + ((code - 0xd800) << 10) + (low - 0xdc00));
        }
        if (0xdc00..=0xdfff).contains(&code) {
            return None;
        }
        char::from_u32(code)
    }

    fn number(&mut self) -> Option<String> {
        let start = self.position;
        self.consume('-');
        if self.consume('0').is_some() {
            // A leading zero may only be followed by a fraction or exponent.
        } else {
            let mut digits = 0;
            while self
                .peek()
                .is_some_and(|character| character.is_ascii_digit())
            {
                self.next();
                digits += 1;
            }
            if digits == 0 {
                return None;
            }
        }
        if self.consume('.').is_some() {
            let mut digits = 0;
            while self
                .peek()
                .is_some_and(|character| character.is_ascii_digit())
            {
                self.next();
                digits += 1;
            }
            if digits == 0 {
                return None;
            }
        }
        if self
            .peek()
            .is_some_and(|character| matches!(character, 'e' | 'E'))
        {
            self.next();
            self.consume('+');
            self.consume('-');
            let mut digits = 0;
            while self
                .peek()
                .is_some_and(|character| character.is_ascii_digit())
            {
                self.next();
                digits += 1;
            }
            if digits == 0 {
                return None;
            }
        }
        Some(self.input[start..self.position].to_string())
    }

    fn literal(&mut self, literal: &str, value: Value) -> Option<Value> {
        self.input
            .get(self.position..)?
            .starts_with(literal)
            .then(|| {
                self.position += literal.len();
                value
            })
    }

    fn whitespace(&mut self) {
        while self
            .peek()
            .is_some_and(|character| matches!(character, ' ' | '\t' | '\n' | '\r'))
        {
            self.next();
        }
    }

    fn consume(&mut self, expected: char) -> Option<()> {
        (self.peek()? == expected).then(|| {
            self.next();
        })
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.position..)?.chars().next()
    }

    fn next(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.position += character.len_utf8();
        Some(character)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_valid_utf16_surrogate_pairs() {
        assert_eq!(
            parse(r#""\ud83d\ude00""#),
            Some(Value::String("😀".to_string()))
        );
        assert!(parse(r#""\ud83d""#).is_none());
        assert!(parse(r#""\ude00""#).is_none());
    }
}
