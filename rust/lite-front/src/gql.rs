// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The `#GQL{ ... }#` island's GraphQL document (core/'s `parser.GqlParser`): scannerless over the island's raw
//! source text, as Java's is, its errors `ParseException`s placed by counting the text's lines and units from the
//! island's content start (`fail`).

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::doors::JavaError;
use crate::jcat;
use crate::jnum::{parse_double, parse_long};
use crate::jtypes::JStr;
use crate::records::{
    GqlArgument, GqlBooleanValue, GqlDefinition, GqlDirective, GqlDirectiveDef, GqlDocument, GqlEnumType, GqlEnumValue,
    GqlEnumValueDef, GqlField, GqlFieldDef, GqlFloatValue, GqlFragment, GqlFragmentSpread, GqlInputObjectType,
    GqlInputValueDef, GqlIntValue, GqlInterfaceType, GqlListType, GqlListValue, GqlNamedType, GqlNullValue,
    GqlObjectType, GqlObjectValue, GqlOperation, GqlRootOp, GqlScalarType, GqlSchemaDef, GqlSelection, GqlStringValue,
    GqlTypeRef, GqlUnionType, GqlValue, GqlVariableDef, GqlVariableRef,
};
use lite_javacompat::character::is_digit;

type R<T> = Result<T, JavaError>;

/// `GqlParser.parseDocument(content, baseLine, baseCol)`.
pub fn parse_document(content: &[u16], base_line: i32, base_col: i32) -> R<GqlDocument> {
    let mut p = Gql { src: content, pos: 0, base_line, base_col };
    let mut defs = Vec::new();
    p.ws();
    while !p.at_end() {
        defs.push(p.definition()?);
        p.ws();
    }
    if defs.is_empty() {
        return Err(p.fail("empty GraphQL document"));
    }
    Ok(GqlDocument { definitions: defs })
}

struct Gql<'a> {
    src: &'a [u16],
    pos: usize,
    base_line: i32,
    base_col: i32,
}

const EXEC_LOCATIONS: &[&str] =
    &["QUERY", "MUTATION", "SUBSCRIPTION", "FIELD", "FRAGMENT_DEFINITION", "FRAGMENT_SPREAD", "INLINE_FRAGMENT"];
const TYPE_SYSTEM_LOCATIONS: &[&str] = &[
    "SCHEMA",
    "SCALAR",
    "OBJECT",
    "FIELD_DEFINITION",
    "ARGUMENT_DEFINITION",
    "INTERFACE",
    "UNION",
    "ENUM",
    "ENUM_VALUE",
    "INPUT_OBJECT",
    "INPUT_FIELD_DEFINITION",
];

fn is_name_start(c: u16) -> bool {
    c == u16::from(b'_')
        || (u16::from(b'a')..=u16::from(b'z')).contains(&c)
        || (u16::from(b'A')..=u16::from(b'Z')).contains(&c)
}

fn is_name_part(c: u16) -> bool {
    is_name_start(c) || (u16::from(b'0')..=u16::from(b'9')).contains(&c)
}

fn c(b: u8) -> u16 {
    u16::from(b)
}

impl Gql<'_> {
    fn definition(&mut self) -> R<GqlDefinition> {
        if self.peekc() == c(b'{') {
            // a bare selection set: an operation whose type the wire omits
            let selection_set = self.selection_set()?;
            return Ok(GqlDefinition::GqlOperation(Box::new(GqlOperation {
                r#type: None,
                name: None,
                variables: Vec::new(),
                directives: Vec::new(),
                selection_set,
            })));
        }
        let kw = self.name("definition keyword")?;
        Ok(match kw.lossy().as_str() {
            "query" | "mutation" | "subscription" => GqlDefinition::GqlOperation(Box::new(self.operation(kw)?)),
            "fragment" => GqlDefinition::GqlFragment(Box::new(self.fragment_definition()?)),
            "type" => GqlDefinition::GqlObjectType(Box::new(self.object_type_definition()?)),
            "schema" => GqlDefinition::GqlSchemaDef(Box::new(self.schema_definition()?)),
            "directive" => GqlDefinition::GqlDirectiveDef(Box::new(self.directive_definition()?)),
            "scalar" => GqlDefinition::GqlScalarType(Box::new(self.scalar_definition()?)),
            "interface" => GqlDefinition::GqlInterfaceType(Box::new(self.interface_definition()?)),
            "union" => GqlDefinition::GqlUnionType(Box::new(self.union_definition()?)),
            "enum" => GqlDefinition::GqlEnumType(Box::new(self.enum_definition()?)),
            "input" => GqlDefinition::GqlInputObjectType(Box::new(self.input_definition()?)),
            _ => return Err(self.fail_j(jcat!("unbuilt GraphQL definition kind '", kw, "'"))),
        })
    }

    fn schema_definition(&mut self) -> R<GqlSchemaDef> {
        let directives = self.directives()?;
        self.expect(b'{')?;
        let mut roots = Vec::new();
        self.ws();
        while self.peekc() != c(b'}') {
            let op = self.name("root operation type")?;
            if !(op == "query" || op == "mutation" || op == "subscription") {
                return Err(self.fail_j(jcat!("unknown root operation '", op, "'")));
            }
            self.ws();
            self.expect(b':')?;
            self.ws();
            let r#type = self.ident("root type name")?;
            roots.push(Box::new(GqlRootOp { operation_type: op, r#type }));
            self.ws();
        }
        self.expect(b'}')?;
        if roots.is_empty() {
            return Err(self.fail("empty schema body"));
        }
        Ok(GqlSchemaDef { directives, root_ops: roots })
    }

    fn directive_definition(&mut self) -> R<GqlDirectiveDef> {
        self.ws();
        self.expect(b'@')?;
        let name = self.name("directive name")?;
        self.ws();
        let args = if self.peekc() == c(b'(') { self.input_value_list(c(b')'))? } else { Vec::new() };
        self.ws();
        if self.name("'on'")? != "on" {
            return Err(self.fail("directive definition needs 'on <locations>'"));
        }
        let mut exec = Vec::new();
        let mut type_system = Vec::new();
        self.ws();
        if self.peekc() == c(b'|') {
            self.pos += 1;
            self.ws();
        }
        loop {
            let loc = self.name("directive location")?;
            let l = loc.lossy();
            if EXEC_LOCATIONS.contains(&l.as_str()) {
                exec.push(loc);
            } else if TYPE_SYSTEM_LOCATIONS.contains(&l.as_str()) {
                type_system.push(loc);
            } else {
                return Err(self.fail_j(jcat!("unknown directive location '", loc, "'")));
            }
            self.ws();
            if self.peekc() != c(b'|') {
                break;
            }
            self.pos += 1;
            self.ws();
        }
        Ok(GqlDirectiveDef { name, args, exec_locations: exec, type_system_locations: type_system })
    }

    fn scalar_definition(&mut self) -> R<GqlScalarType> {
        self.ws();
        let name = self.name("scalar name")?;
        Ok(GqlScalarType { name, directives: self.directives()? })
    }

    fn interface_definition(&mut self) -> R<GqlInterfaceType> {
        self.ws();
        let name = self.name("interface name")?;
        let implements_list = self.implements_clause()?;
        let directives = self.directives()?;
        let fields = self.field_definitions()?;
        Ok(GqlInterfaceType { name, directives, fields, implements_list })
    }

    fn union_definition(&mut self) -> R<GqlUnionType> {
        self.ws();
        let name = self.name("union name")?;
        let directives = self.directives()?;
        self.ws();
        self.expect(b'=')?;
        let mut members = Vec::new();
        self.ws();
        if self.peekc() == c(b'|') {
            self.pos += 1;
            self.ws();
        }
        loop {
            members.push(self.name("union member")?);
            self.ws();
            if self.peekc() != c(b'|') {
                break;
            }
            self.pos += 1;
            self.ws();
        }
        Ok(GqlUnionType { name, directives, members })
    }

    fn enum_definition(&mut self) -> R<GqlEnumType> {
        self.ws();
        let name = self.name("enum name")?;
        let directives = self.directives()?;
        self.expect(b'{')?;
        let mut values = Vec::new();
        self.ws();
        while self.peekc() != c(b'}') {
            let value = self.name("enum value")?;
            let directives = self.directives()?;
            values.push(Box::new(GqlEnumValueDef { value, directives }));
            self.ws();
        }
        self.expect(b'}')?;
        if values.is_empty() {
            return Err(self.fail("empty enum body"));
        }
        Ok(GqlEnumType { name, directives, values })
    }

    fn input_definition(&mut self) -> R<GqlInputObjectType> {
        self.ws();
        let name = self.name("input name")?;
        let directives = self.directives()?;
        self.ws();
        self.expect(b'{')?;
        let fields = self.input_value_body()?;
        Ok(GqlInputObjectType { name, directives, fields })
    }

    /// `implements A & B` or `implements A, B`.
    fn implements_clause(&mut self) -> R<Vec<JStr>> {
        self.ws();
        if !is_name_start(self.peekc()) {
            return Ok(Vec::new());
        }
        let save = self.pos;
        let kw = self.name("clause")?;
        if kw != "implements" {
            self.pos = save;
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        self.ws();
        if self.peekc() == c(b'&') {
            self.pos += 1;
            self.ws();
        }
        loop {
            out.push(self.name("implemented interface")?);
            self.ws();
            if self.peekc() == c(b'&') || self.peekc() == c(b',') {
                self.pos += 1;
                self.ws();
                continue;
            }
            break;
        }
        Ok(out)
    }

    fn input_value_list(&mut self, close: u16) -> R<Vec<Box<GqlInputValueDef>>> {
        self.expect(if close == c(b')') { b'(' } else { b'{' })?;
        let mut out = Vec::new();
        self.ws();
        while self.peekc() != close {
            out.push(Box::new(self.input_value()?));
            self.ws();
            if self.peekc() == c(b',') {
                self.pos += 1;
                self.ws();
            }
        }
        self.pos += 1; // the close char
        if out.is_empty() {
            return Err(self.fail("empty input value list"));
        }
        Ok(out)
    }

    fn input_value_body(&mut self) -> R<Vec<Box<GqlInputValueDef>>> {
        let mut out = Vec::new();
        self.ws();
        while self.peekc() != c(b'}') {
            out.push(Box::new(self.input_value()?));
            self.ws();
            if self.peekc() == c(b',') {
                self.pos += 1;
                self.ws();
            }
        }
        self.pos += 1;
        if out.is_empty() {
            return Err(self.fail("empty input body"));
        }
        Ok(out)
    }

    fn input_value(&mut self) -> R<GqlInputValueDef> {
        let name = self.name("input value name")?;
        self.ws();
        self.expect(b':')?;
        let r#type = self.type_reference()?;
        self.ws();
        let mut default_value = None;
        if self.peekc() == c(b'=') {
            self.pos += 1;
            self.ws();
            default_value = Some(self.value()?);
            self.ws();
        }
        let directives = self.directives()?;
        Ok(GqlInputValueDef { name, r#type, default_value, directives })
    }

    fn field_definitions(&mut self) -> R<Vec<Box<GqlFieldDef>>> {
        self.ws();
        self.expect(b'{')?;
        let mut fields = Vec::new();
        self.ws();
        while self.peekc() != c(b'}') {
            let name = self.name("field name")?;
            self.ws();
            let args = if self.peekc() == c(b'(') { self.input_value_list(c(b')'))? } else { Vec::new() };
            self.ws();
            self.expect(b':')?;
            let r#type = self.type_reference()?;
            let directives = self.directives()?;
            fields.push(Box::new(GqlFieldDef { name, r#type, args, directives }));
            self.ws();
        }
        self.expect(b'}')?;
        if fields.is_empty() {
            return Err(self.fail("empty type body"));
        }
        Ok(fields)
    }

    fn operation(&mut self, op_type: JStr) -> R<GqlOperation> {
        self.ws();
        let name = if is_name_start(self.peekc()) { Some(self.ident("operation name")?) } else { None };
        self.ws();
        let variables = if self.peekc() == c(b'(') { self.variable_definitions()? } else { Vec::new() };
        let directives = self.directives()?;
        let selection_set = self.selection_set()?;
        Ok(GqlOperation { r#type: Some(op_type), name, variables, directives, selection_set })
    }

    fn fragment_definition(&mut self) -> R<GqlFragment> {
        self.ws();
        let name = self.ident("fragment name")?;
        self.ws();
        if self.name("'on'")? != "on" {
            return Err(self.fail("fragment needs 'on <Type>'"));
        }
        self.ws();
        let type_condition = self.ident("fragment type condition")?;
        // the engine's walker never reads fragment-level directives: parsed and dropped
        self.directives()?;
        let selection_set = self.selection_set()?;
        Ok(GqlFragment { name, type_condition, directives: Vec::new(), selection_set })
    }

    fn object_type_definition(&mut self) -> R<GqlObjectType> {
        self.ws();
        let name = self.name("type name")?;
        let implements_list = self.implements_clause()?;
        let fields = self.field_definitions()?;
        Ok(GqlObjectType { name, fields, implements_list })
    }

    fn selection_set(&mut self) -> R<Vec<GqlSelection>> {
        self.expect(b'{')?;
        let mut out = Vec::new();
        self.ws();
        while self.peekc() != c(b'}') {
            out.push(self.selection()?);
            self.ws();
        }
        self.expect(b'}')?;
        if out.is_empty() {
            return Err(self.fail("empty selection set"));
        }
        Ok(out)
    }

    fn selection(&mut self) -> R<GqlSelection> {
        if self.peekc() == c(b'.') {
            self.expect(b'.')?;
            self.expect(b'.')?;
            self.expect(b'.')?;
            self.ws();
            let spread = self.name("fragment spread name")?;
            if spread == "on" {
                return Err(self.fail("inline fragments are refused by the reference GraphQL grammar"));
            }
            // spread directives: parsed, dropped by the engine's walker
            self.directives()?;
            return Ok(GqlSelection::GqlFragmentSpread(Box::new(GqlFragmentSpread {
                name: spread,
                directives: Vec::new(),
            })));
        }
        let first = self.ident("field name")?;
        self.ws();
        let mut alias = None;
        let mut field_name = first.clone();
        if self.peekc() == c(b':') {
            self.pos += 1;
            self.ws();
            // the engine keeps the colon inside the alias
            alias = Some(jcat!(first, ":"));
            field_name = self.ident("aliased field name")?;
            self.ws();
        }
        let arguments = if self.peekc() == c(b'(') { self.arguments()? } else { Vec::new() };
        let directives = self.directives()?;
        self.ws();
        let selection_set = if self.peekc() == c(b'{') { self.selection_set()? } else { Vec::new() };
        Ok(GqlSelection::GqlField(Box::new(GqlField { alias, name: field_name, arguments, directives, selection_set })))
    }

    fn arguments(&mut self) -> R<Vec<Box<GqlArgument>>> {
        self.expect(b'(')?;
        let mut out = Vec::new();
        self.ws();
        while self.peekc() != c(b')') {
            let name = self.name("argument name")?;
            self.ws();
            self.expect(b':')?;
            let value = self.value()?;
            out.push(Box::new(GqlArgument { name, value }));
            self.ws();
        }
        self.expect(b')')?;
        if out.is_empty() {
            return Err(self.fail("empty argument list"));
        }
        Ok(out)
    }

    fn directives(&mut self) -> R<Vec<Box<GqlDirective>>> {
        self.ws();
        let mut out = Vec::new();
        while self.peekc() == c(b'@') {
            self.pos += 1;
            let name = self.name("directive name")?;
            self.ws();
            let arguments = if self.peekc() == c(b'(') { self.arguments()? } else { Vec::new() };
            out.push(Box::new(GqlDirective { name, arguments }));
            self.ws();
        }
        Ok(out)
    }

    fn variable_definitions(&mut self) -> R<Vec<Box<GqlVariableDef>>> {
        self.expect(b'(')?;
        let mut out = Vec::new();
        self.ws();
        while self.peekc() != c(b')') {
            self.expect(b'$')?;
            let name = self.name("variable name")?;
            self.ws();
            self.expect(b':')?;
            let r#type = self.type_reference()?;
            self.ws();
            let mut default_value = None;
            if self.peekc() == c(b'=') {
                self.pos += 1;
                default_value = Some(self.value()?);
                self.ws();
            }
            out.push(Box::new(GqlVariableDef { dollar_name: jcat!("$", name), r#type, default_value }));
            self.ws();
        }
        self.expect(b')')?;
        if out.is_empty() {
            return Err(self.fail("empty variable definitions"));
        }
        Ok(out)
    }

    fn type_reference(&mut self) -> R<GqlTypeRef> {
        self.ws();
        if self.peekc() == c(b'[') {
            self.pos += 1;
            let item_type = self.type_reference()?;
            self.ws();
            self.expect(b']')?;
            let nullable = !self.bang();
            return Ok(GqlTypeRef::GqlListType(Box::new(GqlListType { item_type, nullable })));
        }
        let name = self.name("type name")?;
        let nullable = !self.bang();
        Ok(GqlTypeRef::GqlNamedType(Box::new(GqlNamedType { name, nullable })))
    }

    fn bang(&mut self) -> bool {
        self.ws();
        if self.peekc() == c(b'!') {
            self.pos += 1;
            return true;
        }
        false
    }

    fn value(&mut self) -> R<GqlValue> {
        self.ws();
        let ch = self.peekc();
        if ch == c(b'$') {
            self.pos += 1;
            let name = self.name("variable name")?;
            return Ok(GqlValue::GqlVariableRef(Box::new(GqlVariableRef { name })));
        }
        if ch == c(b'"') {
            let value = self.string_literal()?;
            return Ok(GqlValue::GqlStringValue(Box::new(GqlStringValue { value })));
        }
        if ch == c(b'[') {
            self.pos += 1;
            let mut values = Vec::new();
            self.ws();
            while self.peekc() != c(b']') {
                values.push(self.value()?);
                self.ws();
            }
            self.pos += 1;
            return Ok(GqlValue::GqlListValue(Box::new(GqlListValue { values })));
        }
        if ch == c(b'{') {
            self.pos += 1;
            let mut fields = Vec::new();
            self.ws();
            while self.peekc() != c(b'}') {
                let name = self.name("object field name")?;
                self.ws();
                self.expect(b':')?;
                let value = self.value()?;
                fields.push(Box::new(GqlArgument { name, value }));
                self.ws();
            }
            self.pos += 1;
            return Ok(GqlValue::GqlObjectValue(Box::new(GqlObjectValue { fields })));
        }
        if ch == c(b'-') || (c(b'0')..=c(b'9')).contains(&ch) {
            // the .g4 INT/FLOAT shape, enforced
            let s = self.pos;
            if ch == c(b'-') {
                self.pos += 1;
            }
            let digits_start = self.pos;
            while !self.at_end() && is_digit(self.peekc()) {
                self.pos += 1;
            }
            let int_digits = self.pos - digits_start;
            if int_digits == 0 {
                return Err(self.fail("malformed number"));
            }
            if int_digits > 1 && self.src[digits_start] == c(b'0') {
                return Err(self.fail("malformed number (leading zero)"));
            }
            let mut is_float = false;
            if !self.at_end() && self.peekc() == c(b'.') {
                is_float = true;
                self.pos += 1;
                let fs = self.pos;
                while !self.at_end() && is_digit(self.peekc()) {
                    self.pos += 1;
                }
                if self.pos == fs {
                    return Err(self.fail("malformed number (bare fraction point)"));
                }
            }
            if !self.at_end() && (self.peekc() == c(b'e') || self.peekc() == c(b'E')) {
                is_float = true;
                self.pos += 1;
                if !self.at_end() && (self.peekc() == c(b'+') || self.peekc() == c(b'-')) {
                    self.pos += 1;
                }
                let es = self.pos;
                while !self.at_end() && is_digit(self.peekc()) {
                    self.pos += 1;
                }
                if self.pos == es {
                    return Err(self.fail("malformed number (empty exponent)"));
                }
            }
            let num = &self.src[s..self.pos];
            return if is_float {
                match parse_double(num) {
                    Ok(value) => Ok(GqlValue::GqlFloatValue(Box::new(GqlFloatValue { value }))),
                    Err(_) => Err(self.fail_j(jcat!("number out of range: ", num))),
                }
            } else {
                match parse_long(num) {
                    Ok(value) => Ok(GqlValue::GqlIntValue(Box::new(GqlIntValue { value }))),
                    Err(_) => Err(self.fail_j(jcat!("number out of range: ", num))),
                }
            };
        }
        let word = self.name("value")?;
        Ok(match word.lossy().as_str() {
            "true" => GqlValue::GqlBooleanValue(Box::new(GqlBooleanValue { value: true })),
            "false" => GqlValue::GqlBooleanValue(Box::new(GqlBooleanValue { value: false })),
            "null" => GqlValue::GqlNullValue(Box::new(GqlNullValue {})),
            _ => GqlValue::GqlEnumValue(Box::new(GqlEnumValue { value: word })),
        })
    }

    /// GraphQL's insignificants: whitespace, commas and `#` comments.
    fn ws(&mut self) {
        while !self.at_end() {
            let ch = self.src[self.pos];
            if ch == c(b' ') || ch == c(b'\t') || ch == c(b'\n') || ch == c(b'\r') || ch == c(b',') {
                self.pos += 1;
            } else if ch == c(b'#') {
                while !self.at_end() && self.src[self.pos] != c(b'\n') {
                    self.pos += 1;
                }
            } else {
                return;
            }
        }
    }

    /// A name that is not one of the grammar's implicit keywords.
    fn ident(&mut self, what: &str) -> R<JStr> {
        let n = self.name(what)?;
        if n == "null" || n == "on" || n == "implements" || n == "directive" {
            return Err(self.fail_j(jcat!("reserved word '", n, "' cannot be a ", what)));
        }
        Ok(n)
    }

    fn name(&mut self, what: &str) -> R<JStr> {
        self.ws();
        if self.at_end() || !is_name_start(self.src[self.pos]) {
            return Err(self.fail_j(jcat!("expected ", what)));
        }
        let s = self.pos;
        while !self.at_end() && is_name_part(self.src[self.pos]) {
            self.pos += 1;
        }
        Ok(JStr::from(&self.src[s..self.pos]))
    }

    fn string_literal(&mut self) -> R<JStr> {
        self.expect(b'"')?;
        if self.pos + 1 < self.src.len() && self.src[self.pos] == c(b'"') && self.src[self.pos + 1] == c(b'"') {
            // a block string: the engine's grammar has no rule for it
            return Err(self.fail("block strings are not in the reference grammar"));
        }
        let mut b: Vec<u16> = Vec::new();
        while !self.at_end() && self.src[self.pos] != c(b'"') {
            let ch = self.src[self.pos];
            self.pos += 1;
            b.push(ch);
            if ch == c(b'\\') {
                // escapes ride the wire raw; the .g4 escape set validated
                if self.at_end() {
                    return Err(self.fail("dangling escape"));
                }
                let e = self.src[self.pos];
                self.pos += 1;
                if !"\"\\/bfnrt".encode_utf16().any(|x| x == e) && e != c(b'u') {
                    return Err(self.fail_j(jcat!("unbuilt string escape '\\", &[e][..], "'")));
                }
                b.push(e);
            }
        }
        self.expect(b'"')?;
        Ok(JStr(b))
    }

    fn expect(&mut self, ch: u8) -> R<()> {
        self.ws();
        if self.at_end() || self.src[self.pos] != c(ch) {
            return Err(self.fail_j(jcat!("expected '", &[c(ch)][..], "'")));
        }
        self.pos += 1;
        Ok(())
    }

    fn peekc(&self) -> u16 {
        if self.at_end() { 0 } else { self.src[self.pos] }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn fail(&self, message: &str) -> JavaError {
        self.fail_j(JStr::from(message))
    }

    /// `fail`: a `ParseException` at the cursor, its line and column counted over the text from the base.
    fn fail_j(&self, message: JStr) -> JavaError {
        let mut line = self.base_line;
        let mut col = self.base_col;
        for &ch in &self.src[..self.pos.min(self.src.len())] {
            if ch == c(b'\n') {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        JavaError::parse(jcat!("GQL: ", message), line, col)
    }
}
