// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `TypeClassifier` (TypeClassifier.java at 18feff3fe): F's shared type kernel -- a name to its kind (`find_type`:
//! primitive, primitive extension, the `Enum` metaclass, class, enum, by existence; no structure built) and the one
//! `TypeExpression -> Type` boundary (`classify`), with the declared-type checks (`require_arguments`: a class's
//! type and multiplicity argument counts, a primitive's type-variable values, in legend-engine's words). An unknown
//! name is refused ("Unknown type: ..."), never defaulted (AGENTS.md invariant 4).
//!
//! A view over the index, `Copy`, made per call (the model owns the index, so it cannot hold one: `Model::classifier`).
//! Every method that makes a `Type` takes the arena it allocates in; [`TypeClassifier::require_arguments`] makes none
//! (where it needs a name's kind it may classify in a scratch arena, or test the kind without one).
//!
//! RE-EXPRESSED (EF_CENSUS §4): TC:47's `InferenceKernel.ENUM_METACLASS_FQN` is [`lite_types::platform::ENUM`], the
//! same string, so the element-to-spec edge goes; TC:111 (`val : P(8)[1]`, a primitive with its constraint values)
//! classifies the name directly, where Java built a protocol `NameRef` to classify (`ArchitectureTest`'s
//! `PROTOCOL_DESUGAR_DEBT`).
//!
//! The fill (2026-11-15, the element slice, helper A): a name goes into the arena as Rust text, its UTF-16 read as
//! UTF-8 (`JStr::lossy`: a lone surrogate would not survive, as `Multiplicity::from` and lite-names' `ViewSignatures`
//! put names; the parser admits none in a name); a name is compared to a platform FQN unit for unit. The kind of a
//! name without an arena is [`Kind`] (`find_type` builds its type from it), so `require_arguments` makes no type.
//! Java's literal spellings in a type variable's refusal: `String.valueOf(Number)` and `String.valueOf(double)`
//! through lite-model's `string_value_of` (`lite_javacompat::double_to_string` is not on this crate's line of
//! rust/lite-guards/crate-layers.txt), `BigDecimal.toPlainString` here ([`plain_string`]).

use std::sync::Arc;

use lite_base::Alloc;
use lite_builtin::pure::catalog;
use lite_error::Phase;
use lite_front::records as p;
use lite_model::mapping_from_protocol::string_value_of;
use lite_model::{BigDecimal, ClassDefinition, EnumDefinition, JStr, JValue, cat};
use lite_names::{ModelIndex, Thrown};
use lite_types::{
    Column, FunctionType, GenericType, Multiplicity, Op, Param, Primitive, RelationType, SchemaAlgebra, Type,
    TypeVariable, platform,
};

/// `TypeClassifier` (TypeClassifier.java:23): the kind manifest and the classification, over the index.
#[derive(Clone, Copy)]
pub struct TypeClassifier<'i> {
    model: &'i ModelIndex,
}

/// The kind `findType` (:32) answers for a name, before any type is built: what [`TypeClassifier::find_type`] makes
/// its type from, and what an arena-free caller tests (`findType(x).orElse(null) instanceof Type.Primitive`, :228,
/// :231; `findType(x).isPresent()`, :114).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// a platform primitive (:33), or a primitive extension's base primitive (:39)
    Primitive(Primitive),
    /// the m3 `Enum` metaclass (:47): an enumeration-value type
    EnumMetaclass,
    /// a class of the model or of the native catalog (:50)
    Class,
    /// an enum of the model or of the native catalog (:53)
    Enum,
}

/// One declared type variable of a primitive (TypeClassifier.java:213, `DeclaredVariable`): its name, its type's name
/// as the engine prints it, and that type when it is a primitive.
struct DeclaredVariable {
    name: JStr,
    type_name: JStr,
    r#type: Option<Primitive>,
}

impl<'i> TypeClassifier<'i> {
    /// `new TypeClassifier(model)` (:27).
    pub fn new(model: &'i ModelIndex) -> TypeClassifier<'i> {
        TypeClassifier { model }
    }

    /// The index the classifier reads.
    pub fn model(&self) -> &'i ModelIndex {
        self.model
    }

    /// `findType(fqn)` (:32): the kind an FQN names -- a platform primitive, a primitive extension (its base
    /// primitive), the `Enum` metaclass (an enumeration-value type), a class, an enum -- or none. A class's or an
    /// enum's FQN is put in `arena`.
    pub fn find_type<'a, A: Alloc>(&self, arena: &'a A, fqn: &JStr) -> Option<Type<'a>> {
        Some(match self.kind(fqn)? {
            Kind::Primitive(prim) => Type::Primitive(prim),
            // RE-EXPRESSED (TC:47): `InferenceKernel.ENUM_METACLASS_FQN` is `platform::ENUM`, the string `fqn` equals
            Kind::EnumMetaclass => Type::EnumType(platform::ENUM),
            Kind::Class => Type::ClassType(arena.alloc_str(&fqn.lossy())),
            Kind::Enum => Type::EnumType(arena.alloc_str(&fqn.lossy())),
        })
    }

    /// `findType(fqn)`'s order (:32-56), its answer's kind alone: a platform primitive (`Type.Primitive.findByFqn`),
    /// a primitive extension's base (`model.findPrimitiveExtension`: a PRECISE PRIMITIVE is its base primitive in the
    /// query path), the `Enum` metaclass in a value position ("some enumeration value", never a class navigation),
    /// a class, an enum.
    fn kind(&self, fqn: &JStr) -> Option<Kind> {
        if let Some(prim) = self.find_primitive(fqn) {
            return Some(Kind::Primitive(prim));
        }
        if fqn.eq_str(platform::ENUM) {
            return Some(Kind::EnumMetaclass);
        }
        if self.is_class_fqn(fqn) {
            return Some(Kind::Class);
        }
        if self.is_enum_fqn(fqn) {
            return Some(Kind::Enum);
        }
        None
    }

    /// `findType(fqn)`'s first two steps (:33-42), the only ones that answer a `Type.Primitive`: so
    /// `findType(fqn).orElse(null) instanceof Type.Primitive p ? p : null` (:228) is this, with no type built.
    fn find_primitive(&self, fqn: &JStr) -> Option<Primitive> {
        if let Some((_, prim)) = platform_primitive(fqn) {
            return Some(prim);
        }
        self.model.find_primitive_extension(fqn)
    }

    /// `isClassFqn(fqn)` (:59): a class of the model or of the native catalog.
    pub fn is_class_fqn(&self, fqn: &JStr) -> bool {
        self.model.find_class(fqn).is_some() || catalog().find_native_class(fqn).is_some()
    }

    /// `isEnumFqn(fqn)` (:63): an enum of the model or of the native catalog.
    pub fn is_enum_fqn(&self, fqn: &JStr) -> bool {
        self.model.find_enum(fqn).is_some() || catalog().find_native_enum(fqn).is_some()
    }

    /// `classDef(fqn)` (:76): the definition behind a class FQN, the native catalog first (`KnowledgeLayer.classDef`).
    pub fn class_def(&self, fqn: &JStr) -> Option<&'i Arc<ClassDefinition>> {
        self.model.knowledge().class_def(fqn)
    }

    /// `enumDef(fqn)` (:81): the definition behind an enum FQN, the user model first, then the native catalog.
    pub fn enum_def(&self, fqn: &JStr) -> Option<&'i Arc<EnumDefinition>> {
        self.model.find_enum(fqn).or_else(|| catalog().find_native_enum(fqn))
    }

    /// `classify(te, typeParams)` (:91): a type expression as a kinded type in `arena` -- a type parameter is a
    /// `TypeVar`, `?` the anonymous one, a name its kind (erased by [`lite_types::platform::erase_tds_row`]) or
    /// "Unknown type"; a generic's head must exist, its multiplicity arguments read by `Multiplicity::of_argument`;
    /// function, relation and schema-algebra types structurally.
    pub fn classify<'a, A: Alloc>(
        &self,
        arena: &'a A,
        te: &p::TypeExpression,
        type_params: &[JStr],
    ) -> Result<Type<'a>, Thrown> {
        match te {
            p::TypeExpression::TypeExpressionNameRef(nr) => self.classify_name(arena, &nr.name, type_params),
            p::TypeExpression::TypeExpressionGeneric(g) => {
                if g.arguments.is_empty() && !g.type_variable_values.is_empty() {
                    // `val : P(8)[1]` -- a primitive with its constraint VALUES (Primitive P(x:Integer[1]) extends
                    // Integer): the type is P. RE-EXPRESSED (TC:111): the name classified directly, as the NameRef
                    // arm classifies it, where Java built a protocol `NameRef` to classify
                    return self.classify_name(arena, &g.name, type_params);
                }
                // the head is a type that exists (its arguments' count is a declaration's: requireArguments)
                if self.kind(&g.name).is_none() {
                    return Err(unknown_type(&g.name));
                }
                let mut args = Vec::with_capacity(g.arguments.len());
                for arg in &g.arguments {
                    args.push(self.classify(arena, arg, type_params)?);
                }
                // Result<T|m>-style MULTIPLICITY arguments (engine parity, leg 2): a name spells a variable, digits
                // and * spell bounds
                let mut margs = Vec::with_capacity(g.multiplicity_arguments.len());
                for ma in &g.multiplicity_arguments {
                    margs.push(
                        Multiplicity::of_argument(arena.alloc_str(&ma.lossy())).map_err(Thrown::from_type_error)?,
                    );
                }
                Ok(Type::GenericType(GenericType::new(
                    arena,
                    arena.alloc_str(&g.name.lossy()),
                    &args,
                    &margs,
                )))
            }
            p::TypeExpression::TypeExpressionFunctionType(ft) => {
                let mut ps = Vec::with_capacity(ft.parameters.len());
                for tp in &ft.parameters {
                    ps.push(self.param(arena, tp, type_params)?);
                }
                let result = self.param(arena, &ft.result, type_params)?;
                Ok(Type::FunctionType(FunctionType::new(arena, &ps, result)))
            }
            p::TypeExpression::TypeExpressionRelationType(rt) => {
                let mut cols = Vec::with_capacity(rt.columns.len());
                for c in &rt.columns {
                    let r#type = self.classify(arena, &c.r#type, type_params)?;
                    let multiplicity = Self::multiplicity(arena, &c.multiplicity)?;
                    cols.push(Column {
                        name: arena.alloc_str(&c.name.lossy()),
                        r#type,
                        multiplicity,
                    });
                }
                // `new Type.RelationType(cols)`: two columns of one name its raw IllegalArgumentException
                RelationType::of(arena, &cols)
                    .map(Type::RelationType)
                    .map_err(Thrown::from_type_error)
            }
            p::TypeExpression::TypeExpressionSchemaAlgebra(sa) => {
                let left = self.classify(arena, &sa.left, type_params)?;
                let op = op(sa.op);
                let right = self.classify(arena, &sa.right, type_params)?;
                Ok(Type::SchemaAlgebra(SchemaAlgebra::new(arena, left, op, right)))
            }
        }
    }

    /// `classify`'s `NameRef` arm (:93-106), shared with the precise primitive's (:108-112, RE-EXPRESSED TC:111): a
    /// type parameter in scope is a `TypeVar`; `?` is m3's schema-algebra column-pattern wildcard (`(?:?)⊆T`,
    /// over.pure's signatures), the anonymous type variable (`InferenceKernel.UNKNOWN_COLUMN_TYPE`); else the name's
    /// kind, a row-accessor owner erased (`PlatformTypes.eraseTdsRow`), or "Unknown type".
    fn classify_name<'a, A: Alloc>(&self, arena: &'a A, name: &JStr, type_params: &[JStr]) -> Result<Type<'a>, Thrown> {
        if type_params.contains(name) {
            return Ok(Type::TypeVar(arena.alloc_str(&name.lossy())));
        }
        if name.eq_str("?") {
            return Ok(Type::TypeVar("?"));
        }
        match self.find_type(arena, name) {
            Some(t) => Ok(platform::erase_tds_row(arena, t)),
            None => Err(unknown_type(name)),
        }
    }

    /// `new Type.Param(classify(tp.type(), typeParams), multiplicity(tp.multiplicity()))` (:133, :135-136): the type,
    /// then the multiplicity (its bounds' raw `IllegalArgumentException`: a nested function type's `[2..1]` meets no
    /// bounds check before this, Java's as it is).
    fn param<'a, A: Alloc>(
        &self,
        arena: &'a A,
        tp: &p::TypeExpressionTypedParameter,
        type_params: &[JStr],
    ) -> Result<Param<'a>, Thrown> {
        let r#type = self.classify(arena, &tp.r#type, type_params)?;
        let multiplicity = Self::multiplicity(arena, &tp.multiplicity)?;
        Ok(Param { r#type, multiplicity })
    }

    /// `requireArguments(te, typeParams)` (:162): a declared type names each class with as many type and
    /// multiplicity arguments as it declares parameters, and each primitive with its type variables' values
    /// (legend-engine's `CompileContext.newGenericType` texts); a bare `Result` is the engine's `Result<Any|1..*>`.
    pub fn require_arguments(&self, te: &p::TypeExpression, type_params: &[JStr]) -> Result<(), Thrown> {
        match te {
            p::TypeExpression::TypeExpressionNameRef(nr) => {
                if !type_params.contains(&nr.name) {
                    self.require_type_variables(&nr.name, &[])?;
                }
                // a bare Result is the engine's Result<Any|1..*>, its backward compatibility (DomainParseTreeWalker
                // rewrites a written `Result`; ProtocolReader reads it back bare). Resolved, the written `Result` and
                // a bare full path are one name here, so the second is accepted too, where the engine counts its
                // arguments (S95)
                if !type_params.contains(&nr.name) && self.is_class_fqn(&nr.name) && !nr.name.eq_str(platform::RESULT) {
                    self.require_arity(&nr.name, 0, 0)?;
                }
                Ok(())
            }
            p::TypeExpression::TypeExpressionGeneric(g) => {
                self.require_type_variables(&g.name, &g.type_variable_values)?;
                if g.arguments.is_empty() && !g.type_variable_values.is_empty() {
                    return Ok(()); // a precise primitive's values (P(8)), no type arguments
                }
                if self.is_class_fqn(&g.name) {
                    self.require_arity(&g.name, g.arguments.len(), g.multiplicity_arguments.len())?;
                }
                for arg in &g.arguments {
                    self.require_arguments(arg, type_params)?;
                }
                Ok(())
            }
            p::TypeExpression::TypeExpressionFunctionType(ft) => {
                for tp in &ft.parameters {
                    self.require_arguments(&tp.r#type, type_params)?;
                }
                self.require_arguments(&ft.result.r#type, type_params)
            }
            p::TypeExpression::TypeExpressionRelationType(rt) => {
                for c in &rt.columns {
                    self.require_arguments(&c.r#type, type_params)?;
                }
                Ok(())
            }
            p::TypeExpression::TypeExpressionSchemaAlgebra(sa) => {
                self.require_arguments(&sa.left, type_params)?;
                self.require_arguments(&sa.right, type_params)
            }
        }
    }

    /// `requireTypeVariables(fqn, values)` (:223): a primitive's type-variable values against its declaration --
    /// their count, each a literal of its variable's type ("Wrong type variables count (N) for type: ...", "Error for
    /// type: ..., 'v' is not an instance of T", Java's literal spellings).
    fn require_type_variables(&self, fqn: &JStr, values: &[p::ValueSpecification]) -> Result<(), Thrown> {
        let mut declared: Vec<DeclaredVariable> = Vec::new();
        if self.model.is_primitive_extension(fqn) {
            for tv in self.model.primitive_type_variables(fqn).iter() {
                let head = Self::head_fqn(&tv.r#type)?;
                // `findType(head).orElse(null) instanceof Type.Primitive p ? p : null`
                let prim = self.find_primitive(head);
                declared.push(DeclaredVariable {
                    name: tv.name.clone(),
                    type_name: simple_name(head)?,
                    r#type: prim,
                });
            }
        } else if self.find_primitive(fqn).is_some() {
            // a platform primitive: an m3 one (by its full path or its reserved bare name) declares none
            let variables = platform_type_variables(fqn)
                .ok_or_else(|| Thrown::illegal_state(cat!(fqn, " classifies as a platform primitive but is none")))?;
            for tv in variables {
                declared.push(DeclaredVariable {
                    name: JStr::from(tv.name),
                    type_name: JStr::from(tv.r#type.type_name()),
                    r#type: Some(tv.r#type),
                });
            }
        } else {
            return Ok(());
        }
        let mut printed = simple_name(fqn)?;
        if !declared.is_empty() {
            let mut joined: Vec<u16> = Vec::new();
            for (i, d) in declared.iter().enumerate() {
                if i > 0 {
                    joined.push(u16::from(b','));
                }
                joined.extend_from_slice(&cat!(&d.name, ":", &d.type_name).0);
            }
            printed = cat!(printed, "(", joined, ")");
        }
        if declared.len() != values.len() {
            return Err(Thrown::model(
                Phase::Model,
                cat!(
                    "Wrong type variables count (",
                    values.len().to_string(),
                    ") for type: ",
                    printed
                ),
                None,
            ));
        }
        for (d, v) in declared.iter().zip(values) {
            // the value's type and its text (`String.valueOf(c.value())`, `toPlainString()`, the string itself) in
            // one match: Java's second switch (:263-270) and its unreachable `IllegalStateException` ("a literal was
            // classified above") have no arm of their own
            let (value_type, name) = match v {
                p::ValueSpecification::CInteger(c) => (Primitive::Integer, string_value_of(&c.value)),
                p::ValueSpecification::CFloat(c) => (Primitive::Float, string_value_of(&JValue::Double(c.value))),
                p::ValueSpecification::CDecimal(c) => (Primitive::Decimal, JStr::from(plain_string(&c.value))),
                p::ValueSpecification::CString(c) => (Primitive::String, c.value.clone()),
                p::ValueSpecification::CBoolean(c) => (Primitive::Boolean, JStr::from(c.value.to_string())),
                _ => {
                    return Err(Thrown::model(
                        Phase::Model,
                        cat!(
                            "a type variable's value must be a literal: ",
                            printed,
                            ", variable '",
                            &d.name,
                            "'"
                        ),
                        None,
                    ));
                }
            };
            let instance = d
                .r#type
                .is_some_and(|t| t == value_type || (t == Primitive::Number && value_type.is_numeric()));
            if !instance {
                return Err(Thrown::model(
                    Phase::Model,
                    cat!(
                        "Error for type: ",
                        printed,
                        ", '",
                        name,
                        "' is not an instance of ",
                        &d.type_name
                    ),
                    None,
                ));
            }
        }
        Ok(())
    }

    /// `requireArity(fqn, types, multiplicities)` (:277): a class's argument counts ("Wrong type|multiplicity
    /// arguments count (N) for type: C<T|m>", `arityMismatch` :288); a class with no definition is Java's
    /// `IllegalStateException`.
    fn require_arity(&self, fqn: &JStr, types: usize, multiplicities: usize) -> Result<(), Thrown> {
        let def = self
            .class_def(fqn)
            .ok_or_else(|| Thrown::illegal_state(cat!(fqn, " classifies as a class but has no definition")))?;
        if def.type_params.len() != types {
            return Err(arity_mismatch(def, "type", types));
        }
        if def.multiplicity_params.len() != multiplicities {
            return Err(arity_mismatch(def, "multiplicity", multiplicities));
        }
        Ok(())
    }

    /// `headFqn(te)` (:299): the FQN heading a supertype or generic reference; a function, relation or
    /// schema-algebra type cannot head one (a `ModelException` each, in its words).
    pub fn head_fqn(te: &p::TypeExpression) -> Result<&JStr, Thrown> {
        // EXHAUSTIVE (no default): a head reference is nominal by construction; the structural forms each say why
        // they cannot be
        let refused = |message: &str| Err(Thrown::model(Phase::Model, JStr::from(message), None));
        match te {
            p::TypeExpression::TypeExpressionNameRef(nr) => Ok(&nr.name),
            p::TypeExpression::TypeExpressionGeneric(g) => Ok(&g.name),
            p::TypeExpression::TypeExpressionFunctionType(_) => {
                refused("a function type cannot head a supertype/generic reference")
            }
            p::TypeExpression::TypeExpressionRelationType(_) => {
                refused("a relation type cannot head a supertype/generic reference")
            }
            p::TypeExpression::TypeExpressionSchemaAlgebra(_) => {
                refused("a schema-algebra expression cannot head a supertype/generic reference")
            }
        }
    }

    /// `multiplicity(m)` (:314): `Multiplicity.from`, its `IllegalArgumentException` a raw Java throw, as Java lets it
    /// escape ([`Thrown::from_type_error`]).
    pub fn multiplicity<'a, A: Alloc>(arena: &'a A, m: &p::Multiplicity) -> Result<Multiplicity<'a>, Thrown> {
        Multiplicity::from(arena, m).map_err(Thrown::from_type_error)
    }
}

/// `Type.Primitive.findByFqn(fqn)` (Type.java:174 at 18feff3fe) over the units: the row of `Type.Primitive.byFqn()`
/// whose FQN `fqn` is, unit for unit.
fn platform_primitive(fqn: &JStr) -> Option<(&'static str, Primitive)> {
    Primitive::by_fqn().iter().copied().find(|(f, _)| fqn.eq_str(f))
}

/// `Type.Primitive.typeVariables(fqn)` (Type.java:166 at 18feff3fe) over the units: the platform primitive's declared
/// type variables, `None` when `fqn` is no platform primitive.
fn platform_type_variables(fqn: &JStr) -> Option<&'static [TypeVariable]> {
    let (f, _) = platform_primitive(fqn)?;
    Primitive::type_variables(f)
}

/// `unknownType(name)` (:151): the refusal of a name that is no primitive, class or enum.
fn unknown_type(name: &JStr) -> Thrown {
    Thrown::model(
        Phase::Model,
        cat!("Unknown type: '", name, "' is not a known primitive, class, or enum"),
        None,
    )
}

/// `arityMismatch(def, kind, got)` (:288): "Wrong <kind> arguments count (<got>) for type: <name><params>", the class
/// printed as legend-pure's `_Class.print` prints it (its simple name, then `<T,U|m,n>` when it declares any). The
/// simple name's own refusal (`Protocol.splitFqn`'s, raised while the message is built) is the throw instead.
fn arity_mismatch(def: &ClassDefinition, kind: &str, got: usize) -> Thrown {
    let params = if def.type_params.is_empty() && def.multiplicity_params.is_empty() {
        JStr::default()
    } else {
        let mut out = cat!("<", join(&def.type_params, ","));
        if !def.multiplicity_params.is_empty() {
            out = cat!(out, "|", join(&def.multiplicity_params, ","));
        }
        cat!(out, ">")
    };
    match simple_name(&def.qualified_name) {
        Ok(name) => Thrown::model(
            Phase::Model,
            cat!(
                "Wrong ",
                kind,
                " arguments count (",
                got.to_string(),
                ") for type: ",
                name,
                params
            ),
            None,
        ),
        Err(e) => e,
    }
}

/// `String.join(separator, parts)`, over units.
fn join(parts: &[JStr], separator: &str) -> JStr {
    let mut out: Vec<u16> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.extend(separator.encode_utf16());
        }
        out.extend_from_slice(&part.0);
    }
    JStr(out)
}

/// `Protocol.splitFqn(fqn)[1]` (Protocol.java:3443 at 18feff3fe, lite-front's `protocol::split_fqn`): the name after
/// the last `::` outside a quoted segment, its quoted segments unquoted and unescaped. A unicode escape of fewer than
/// four hex digits (in either half: Java unquotes the package first) is `Escapes.unescapeJavaLike`'s raw
/// `IllegalArgumentException`, its text Java's (lite-front's own refusal of it is the parser's, not this one's).
fn simple_name(fqn: &JStr) -> Result<JStr, Thrown> {
    lite_front::protocol::split_fqn(&fqn.0)
        .map(|(_, name)| JStr(name))
        .map_err(|_| Thrown::illegal_argument(JStr::from(lite_front::escapes::SHORT_UNICODE_ESCAPE)))
}

/// `BigDecimal.toPlainString()` (JDK 25): the unscaled digits with the point placed by the scale, never an exponent --
/// a scale of 0 the digits signed, a negative scale trailing zeros (a zero is "0"), a positive one the point inside the
/// digits or `0.` and leading zeros (`getValueString`).
fn plain_string(d: &BigDecimal) -> String {
    let sign = if d.negative { "-" } else { "" };
    let digits = d.digits.as_str();
    let scale = i64::from(d.scale);
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    if scale < 0 {
        if digits == "0" {
            return "0".to_owned();
        }
        return format!("{sign}{digits}{}", "0".repeat((-scale) as usize));
    }
    let insertion_point = digits.len() as i64 - scale;
    if insertion_point == 0 {
        format!("{sign}0.{digits}")
    } else if insertion_point > 0 {
        let at = insertion_point as usize;
        format!("{sign}{}.{}", &digits[..at], &digits[at..])
    } else {
        format!("{sign}0.{}{digits}", "0".repeat((-insertion_point) as usize))
    }
}

/// `op(op)` (:318): a schema-algebra operator, constant for constant.
fn op(op: p::TypeExpressionOp) -> Op {
    match op {
        p::TypeExpressionOp::EQUAL => Op::Equal,
        p::TypeExpressionOp::UNION => Op::Union,
        p::TypeExpressionOp::DIFFERENCE => Op::Difference,
        p::TypeExpressionOp::SUBSET => Op::Subset,
    }
}

#[cfg(test)]
mod tests {
    use lite_base::QueryArena;
    use lite_front::doors::Dialect;
    use lite_front::records as p;
    use lite_model::text::j;
    use lite_model::{BigDecimal, ClassDefinition, JValue, PackageableElement, ParsedModel};
    use lite_names::{ModelIndex, Thrown};
    use lite_types::{Bounded, Multiplicity, Op, Primitive, Type, platform};

    use super::{TypeClassifier, plain_string};

    const M3: &str = "meta::pure::metamodel::type::";
    const PRECISE: &str = "meta::pure::precisePrimitives::";

    /// A model text parsed and its names resolved (strict), as `rust/lite-normalizer/src/test_support.rs` makes it.
    fn parsed(text: &str, dialect: Dialect) -> ParsedModel {
        let units: Vec<u16> = text.encode_utf16().collect();
        let parsed = lite_builtin::door::parse(&units, dialect).expect("the model parses");
        lite_names::resolver::resolve(&parsed).expect("the model's names resolve")
    }

    fn index(text: &str) -> ModelIndex {
        ModelIndex::from(&parsed(text, Dialect::LegendLite)).expect("the model indexes")
    }

    fn class<'x>(index: &'x ModelIndex, fqn: &str) -> &'x ClassDefinition {
        index.find_class(&j(fqn)).expect("the class")
    }

    /// The declared type of `fqn`'s property `name`.
    fn declared<'x>(index: &'x ModelIndex, fqn: &str, name: &str) -> &'x p::TypeExpression {
        &class(index, fqn)
            .properties
            .iter()
            .find(|pd| pd.name == j(name))
            .expect("the property")
            .r#type
    }

    /// `Class x::A { v: <declaration>[1]; }`, after `prefix`: the declared type's argument check.
    fn arguments(prefix: &str, declaration: &str) -> Result<(), Thrown> {
        arguments_in(Dialect::LegendLite, prefix, declaration)
    }

    /// [`arguments`] at `dialect` (a class's multiplicity parameters are the platform's grammar).
    fn arguments_in(dialect: Dialect, prefix: &str, declaration: &str) -> Result<(), Thrown> {
        let text = format!("###Pure\n{prefix}Class x::A\n{{\n  v: {declaration}[1];\n}}\n");
        let index = ModelIndex::from(&parsed(&text, dialect)).expect("the model indexes");
        let cd = class(&index, "x::A");
        TypeClassifier::new(&index).require_arguments(declared(&index, "x::A", "v"), &cd.type_params)
    }

    /// A `ModelException` of phase MODEL, no element, its message exactly `message`.
    fn assert_model(thrown: Thrown, message: &str) {
        match &thrown {
            Thrown::Compile(e) => {
                assert_eq!(e.kind, lite_error::ErrorKind::Model, "{thrown:?}");
                assert_eq!(e.phase, Some(lite_error::Phase::Model), "{thrown:?}");
                assert_eq!(e.element, None, "{thrown:?}");
            }
            Thrown::Java { .. } => panic!("a ModelException, not {thrown:?}"),
        }
        assert_eq!(thrown.message().expect("a message").lossy(), message);
    }

    /// A raw JDK exception of `class`, its message exactly `message`.
    fn assert_java(thrown: Thrown, class: &str, message: &str) {
        assert_eq!(thrown.java_class(), class, "{thrown:?}");
        assert!(matches!(thrown, Thrown::Java { .. }), "{thrown:?}");
        assert_eq!(thrown.message().expect("a message").lossy(), message);
    }

    fn name_ref(name: &str) -> p::TypeExpression {
        p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
            name: j(name),
            pos: None,
        }))
    }

    fn generic(name: &str, arguments: Vec<p::TypeExpression>, mults: &[&str]) -> p::TypeExpression {
        generic_with(name, arguments, mults, vec![])
    }

    fn generic_with(
        name: &str,
        arguments: Vec<p::TypeExpression>,
        mults: &[&str],
        values: Vec<p::ValueSpecification>,
    ) -> p::TypeExpression {
        p::TypeExpression::TypeExpressionGeneric(Box::new(p::TypeExpressionGeneric {
            name: j(name),
            arguments,
            multiplicity_arguments: mults.iter().map(|m| j(m)).collect(),
            type_variable_values: values,
            pos: None,
        }))
    }

    fn concrete(lower_bound: i32, upper_bound: Option<i32>) -> p::Multiplicity {
        p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
            lower_bound,
            upper_bound,
        }))
    }

    fn column(name: &str, te: p::TypeExpression) -> p::TypeExpressionColumn {
        p::TypeExpressionColumn {
            name: j(name),
            r#type: te,
            multiplicity: concrete(1, Some(1)),
            pos: None,
        }
    }

    fn relation(columns: Vec<p::TypeExpressionColumn>) -> p::TypeExpression {
        p::TypeExpression::TypeExpressionRelationType(Box::new(p::TypeExpressionRelationType {
            columns: columns.into_iter().map(Box::new).collect(),
        }))
    }

    fn typed(te: p::TypeExpression, m: p::Multiplicity) -> p::TypeExpressionTypedParameter {
        p::TypeExpressionTypedParameter {
            r#type: te,
            multiplicity: m,
        }
    }

    fn function(parameters: Vec<p::TypeExpressionTypedParameter>, result: p::TypeExpression) -> p::TypeExpression {
        p::TypeExpression::TypeExpressionFunctionType(Box::new(p::TypeExpressionFunctionType {
            parameters: parameters.into_iter().map(Box::new).collect(),
            result: Box::new(typed(result, concrete(1, Some(1)))),
        }))
    }

    fn integer(n: i64) -> p::ValueSpecification {
        p::ValueSpecification::CInteger(std::sync::Arc::new(p::CInteger {
            value: JValue::Long(n),
            pos: None,
        }))
    }

    fn float(value: f64) -> p::ValueSpecification {
        p::ValueSpecification::CFloat(std::sync::Arc::new(p::CFloat {
            value,
            exact: None,
            pos: None,
        }))
    }

    fn decimal(negative: bool, digits: &str, scale: i32) -> BigDecimal {
        BigDecimal {
            negative,
            digits: digits.to_owned(),
            scale,
        }
    }

    fn m3(name: &str) -> String {
        format!("{M3}{name}")
    }

    fn precise(name: &str) -> String {
        format!("{PRECISE}{name}")
    }

    #[test]
    fn find_type_answers_in_javas_order() {
        let index = index("###Pure\nClass x::A\n{\n}\nEnum x::E\n{\n  ONE\n}\nClass meta::pure::tds::TDSRow\n{\n}\n");
        let tc = TypeClassifier::new(&index);
        let cx = QueryArena::new();
        let find = |fqn: &str| tc.find_type(&cx, &j(fqn));
        // a platform primitive first: Integer is also a native class
        assert!(tc.is_class_fqn(&j(&m3("Integer"))));
        assert_eq!(find(&m3("Integer")), Some(Type::Primitive(Primitive::Integer)));
        // a precise primitive is its declared parent
        assert_eq!(find(&precise("Varchar")), Some(Type::Primitive(Primitive::String)));
        assert_eq!(find(&precise("Numeric")), Some(Type::Primitive(Primitive::Decimal)));
        // the Enum metaclass, an enumeration-value type
        assert_eq!(find(platform::ENUM), Some(Type::EnumType(platform::ENUM)));
        // a class and an enum of the model, of the native catalog
        assert_eq!(find("x::A"), Some(Type::ClassType("x::A")));
        assert_eq!(find("x::E"), Some(Type::EnumType("x::E")));
        assert_eq!(
            find("meta::pure::functions::relation::SortType"),
            Some(Type::EnumType("meta::pure::functions::relation::SortType"))
        );
        // findType itself erases nothing: a row owner is its class here, classify's NameRef arm erases it
        assert_eq!(
            find("meta::pure::tds::TDSRow"),
            Some(Type::ClassType("meta::pure::tds::TDSRow"))
        );
        // a bare name is no primitive's (exact FQNs only), and an unknown FQN none
        assert_eq!(find("Integer"), None);
        assert_eq!(find("x::Missing"), None);
        assert_eq!(
            tc.class_def(&j("x::A")).map(|c| c.qualified_name.lossy()),
            Some("x::A".to_owned())
        );
        assert!(tc.enum_def(&j("meta::pure::functions::relation::SortType")).is_some());
    }

    #[test]
    fn a_primitive_extension_is_its_base_primitive() {
        // the Java test's own construction (NameResolverTest:2083): the extension declared as a record, as the
        // platform's prelude declares one
        let mut model = parsed(
            "###Pure\nClass x::A\n{\n  v : test::IntCap(2)[1];\n  w : test::IntCap()[1];\n  n : test::NumCap(2)[1];\n}\n",
            Dialect::LegendLite,
        );
        let extension_of = |fqn: &str, variables: Vec<(&str, p::TypeExpression)>| {
            PackageableElement::PrimitiveExtensionDefinition(std::sync::Arc::new(
                lite_model::PrimitiveExtensionDefinition {
                    qualified_name: j(fqn),
                    base_type_name: j("Integer"),
                    type_variables: variables
                        .into_iter()
                        .map(|(name, ty)| {
                            Box::new(p::ParameterDefinition {
                                name: j(name),
                                r#type: ty,
                                multiplicity: concrete(1, Some(1)),
                                pos: None,
                            })
                        })
                        .collect::<Vec<_>>()
                        .into(),
                },
            ))
        };
        let extension = |fqn: &str, variable: &str, ty: &str| extension_of(fqn, vec![(variable, name_ref(ty))]);
        let mut elements = vec![
            extension("test::IntCap", "x", &m3("Integer")),
            extension("test::NumCap", "x", &m3("Number")),
        ];
        elements.extend(model.elements.iter().cloned());
        model.elements = elements.into();
        let index = ModelIndex::from(&model).expect("the model indexes");
        let tc = TypeClassifier::new(&index);
        let cx = QueryArena::new();
        assert_eq!(
            tc.find_type(&cx, &j("test::IntCap")),
            Some(Type::Primitive(Primitive::Integer))
        );
        // `v : test::IntCap(2)[1]`: classified as its name, its value of its variable's type
        let v = declared(&index, "x::A", "v");
        assert_eq!(tc.classify(&cx, v, &[]).unwrap(), Type::Primitive(Primitive::Integer));
        tc.require_arguments(v, &[]).unwrap();
        // NameResolverTest:2107, the engine's words, the extension printed with its declared variable
        assert_model(
            tc.require_arguments(declared(&index, "x::A", "w"), &[]).unwrap_err(),
            "Wrong type variables count (0) for type: IntCap(x:Integer)",
        );
        // a Number variable takes any numeric literal (Type.Primitive.isNumeric)
        tc.require_arguments(declared(&index, "x::A", "n"), &[]).unwrap();
        // a variable whose type heads no primitive accepts no value: its type is null in Java's record
        let mut model = parsed(
            "###Pure\nClass x::B\n{\n  v : test::Odd(1)[1];\n}\n",
            Dialect::LegendLite,
        );
        let mut elements = vec![extension("test::Odd", "x", "x::B")];
        elements.extend(model.elements.iter().cloned());
        model.elements = elements.into();
        let index = ModelIndex::from(&model).expect("the model indexes");
        let tc = TypeClassifier::new(&index);
        assert_model(
            tc.require_arguments(declared(&index, "x::B", "v"), &[]).unwrap_err(),
            "Error for type: Odd(x:B), '1' is not an instance of B",
        );
        // two declared variables, printed joined by `,` (the oracle's `IntCap(x:Integer,z:String)`); a variable's type
        // that is no nominal is headFqn's refusal, met while the declaration is read, before the count
        let mut model = parsed(
            "###Pure\nClass x::C\n{\n  v : test::Two(1)[1];\n  w : test::Bad(1)[1];\n}\n",
            Dialect::LegendLite,
        );
        let mut elements = vec![
            extension_of(
                "test::Two",
                vec![("x", name_ref(&m3("Integer"))), ("z", name_ref(&m3("String")))],
            ),
            extension_of("test::Bad", vec![("x", relation(vec![]))]),
        ];
        elements.extend(model.elements.iter().cloned());
        model.elements = elements.into();
        let index = ModelIndex::from(&model).expect("the model indexes");
        let tc = TypeClassifier::new(&index);
        assert_model(
            tc.require_arguments(declared(&index, "x::C", "v"), &[]).unwrap_err(),
            "Wrong type variables count (1) for type: Two(x:Integer,z:String)",
        );
        assert_model(
            tc.require_arguments(declared(&index, "x::C", "w"), &[]).unwrap_err(),
            "a relation type cannot head a supertype/generic reference",
        );
    }

    #[test]
    fn classify_answers_every_form() {
        let index = index(
            "###Pure\nClass x::G<T>\n{\n  v: T[1];\n}\nClass meta::pure::tds::TDSRow\n{\n}\n\
             Class x::H\n{\n  f: {String[2..1]->String[1]}[1];\n}\n",
        );
        let tc = TypeClassifier::new(&index);
        let cx = QueryArena::new();
        let params = [j("T")];
        // a type parameter in scope, the anonymous `?`, a name's kind
        assert_eq!(tc.classify(&cx, &name_ref("T"), &params).unwrap(), Type::TypeVar("T"));
        assert_eq!(tc.classify(&cx, &name_ref("?"), &[]).unwrap(), Type::TypeVar("?"));
        assert_eq!(
            tc.classify(&cx, &name_ref(&m3("String")), &params).unwrap(),
            Type::Primitive(Primitive::String)
        );
        // a row owner in a type position is the erased row, its class kept in the template
        match tc.classify(&cx, &name_ref("meta::pure::tds::TDSRow"), &[]).unwrap() {
            Type::RelationType(r) => assert!(r.is_erased_row()),
            other => panic!("the erased row, not {other:?}"),
        }
        // an unknown name, in its words
        assert_model(
            tc.classify(&cx, &name_ref("x::Missing"), &[]).unwrap_err(),
            "Unknown type: 'x::Missing' is not a known primitive, class, or enum",
        );
        // a generic: its head exists, its arguments classified, its multiplicity arguments read by their spelling
        let g = tc
            .classify(
                &cx,
                &generic("x::G", vec![name_ref("T")], &["m", " 0..1 ", "*", "3"]),
                &params,
            )
            .unwrap();
        match g {
            Type::GenericType(g) => {
                assert_eq!(g.raw_fqn, "x::G");
                assert_eq!(g.arguments, &[Type::TypeVar("T")][..]);
                assert_eq!(g.mult_arguments.len(), 4);
                assert_eq!(g.mult_arguments[0].text(), "[m]");
                assert_eq!(g.mult_arguments[1], Multiplicity::ZERO_ONE);
                assert_eq!(g.mult_arguments[2], Multiplicity::ZERO_MANY);
                assert_eq!(
                    g.mult_arguments[3],
                    Multiplicity::Bounded(Bounded::new(3, Some(3)).unwrap())
                );
            }
            other => panic!("a generic, not {other:?}"),
        }
        // a generic's head is any type that exists (its arguments are requireArguments'): a primitive's too
        assert!(matches!(
            tc.classify(&cx, &generic(&m3("String"), vec![name_ref(&m3("Integer"))], &[]), &[]).unwrap(),
            Type::GenericType(g) if g.raw_fqn == m3("String")
        ));
        assert_model(
            tc.classify(&cx, &generic("x::Missing", vec![name_ref("x::Other")], &[]), &[])
                .unwrap_err(),
            "Unknown type: 'x::Missing' is not a known primitive, class, or enum",
        );
        assert_model(
            tc.classify(&cx, &generic("x::G", vec![name_ref("x::Other")], &[]), &[])
                .unwrap_err(),
            "Unknown type: 'x::Other' is not a known primitive, class, or enum",
        );
        // a multiplicity argument past an int: Integer.parseInt's raw NumberFormatException, as ofArgument lets it
        assert_java(
            tc.classify(&cx, &generic("x::G", vec![name_ref("T")], &["99999999999"]), &params)
                .unwrap_err(),
            lite_names::error::NUMBER_FORMAT,
            "For input string: \"99999999999\"",
        );
        // a precise primitive's values: its name classified as the NameRef arm classifies it (TC:111), a type
        // parameter's name first
        let varchar = generic_with(&precise("Varchar"), vec![], &[], vec![integer(200)]);
        assert_eq!(
            tc.classify(&cx, &varchar, &[]).unwrap(),
            Type::Primitive(Primitive::String)
        );
        let shadowed = generic_with("T", vec![], &[], vec![integer(1)]);
        assert_eq!(tc.classify(&cx, &shadowed, &params).unwrap(), Type::TypeVar("T"));
        // the values a generic with arguments carries are not classify's
        let both = generic_with("x::G", vec![name_ref("T")], &[], vec![integer(1)]);
        assert!(matches!(
            tc.classify(&cx, &both, &params).unwrap(),
            Type::GenericType(_)
        ));
        // a function type: each parameter's type then its multiplicity, then the result's
        let ft = function(
            vec![typed(name_ref(&m3("Integer")), concrete(0, None))],
            name_ref(&m3("String")),
        );
        match tc.classify(&cx, &ft, &[]).unwrap() {
            Type::FunctionType(f) => {
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0].to_string(), "Integer[*]");
                assert_eq!(f.result.to_string(), "String[1]");
            }
            other => panic!("a function type, not {other:?}"),
        }
        // a nested function type's [2..1] (the oracle's case `w::H`): Bounded's raw IllegalArgumentException, which
        // classify meets before any bounds check
        assert_java(
            tc.classify(&cx, declared(&index, "x::H", "f"), &[]).unwrap_err(),
            lite_names::error::ILLEGAL_ARGUMENT,
            "upper (1) must be >= lower (2)",
        );
        let unknown_param = function(
            vec![typed(name_ref("x::Missing"), concrete(2, Some(1)))],
            name_ref(&m3("String")),
        );
        assert_model(
            tc.classify(&cx, &unknown_param, &[]).unwrap_err(),
            "Unknown type: 'x::Missing' is not a known primitive, class, or enum",
        );
        // a relation type, its columns by name; two of one name the relation type's raw IllegalArgumentException
        match tc
            .classify(
                &cx,
                &relation(vec![column("a", name_ref(&m3("Integer"))), column("b", name_ref("T"))]),
                &params,
            )
            .unwrap()
        {
            Type::RelationType(r) => assert_eq!(
                r.columns().iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                vec!["a:Integer[1]", "b:T[1]"]
            ),
            other => panic!("a relation type, not {other:?}"),
        }
        assert_java(
            tc.classify(
                &cx,
                &relation(vec![
                    column("a", name_ref(&m3("Integer"))),
                    column("a", name_ref(&m3("String"))),
                ]),
                &[],
            )
            .unwrap_err(),
            lite_names::error::ILLEGAL_ARGUMENT,
            "duplicate column 'a' in relation type",
        );
        // the column's own type is classified before the duplicate is seen
        assert_model(
            tc.classify(
                &cx,
                &relation(vec![
                    column("a", name_ref(&m3("Integer"))),
                    column("a", name_ref("x::Missing")),
                ]),
                &[],
            )
            .unwrap_err(),
            "Unknown type: 'x::Missing' is not a known primitive, class, or enum",
        );
        // schema algebra: left, the operator, right
        let sa = p::TypeExpression::TypeExpressionSchemaAlgebra(Box::new(p::TypeExpressionSchemaAlgebra {
            left: name_ref("T"),
            op: p::TypeExpressionOp::SUBSET,
            right: relation(vec![column("a", name_ref("?"))]),
        }));
        match tc.classify(&cx, &sa, &params).unwrap() {
            Type::SchemaAlgebra(s) => {
                assert_eq!(s.left, Type::TypeVar("T"));
                assert_eq!(s.op, Op::Subset);
                assert_eq!(s.right.to_string(), "(a:?[1])");
            }
            other => panic!("schema algebra, not {other:?}"),
        }
        for (op, expected) in [
            (p::TypeExpressionOp::EQUAL, Op::Equal),
            (p::TypeExpressionOp::UNION, Op::Union),
            (p::TypeExpressionOp::DIFFERENCE, Op::Difference),
            (p::TypeExpressionOp::SUBSET, Op::Subset),
        ] {
            assert_eq!(super::op(op), expected);
        }
    }

    #[test]
    fn a_primitives_type_variable_values_are_checked_in_the_engines_words() {
        // DecimalKindsTest.typeVariableValuesAreChecked: Java asserts `endsWith`, behind the integrity pass's prefix;
        // the classifier's own message is the whole text
        let refused = |declaration: &str, message: &str| assert_model(arguments("", declaration).unwrap_err(), message);
        refused("Decimal(1)", "Wrong type variables count (1) for type: Decimal");
        refused("Float(1)", "Wrong type variables count (1) for type: Float");
        refused(
            "Numeric",
            "Wrong type variables count (0) for type: Numeric(precision:Integer,scale:Integer)",
        );
        refused(
            "Numeric()",
            "Wrong type variables count (0) for type: Numeric(precision:Integer,scale:Integer)",
        );
        refused(
            "Numeric(10)",
            "Wrong type variables count (1) for type: Numeric(precision:Integer,scale:Integer)",
        );
        refused("Varchar", "Wrong type variables count (0) for type: Varchar(x:Integer)");
        refused(
            "Numeric('a', 2)",
            "Error for type: Numeric(precision:Integer,scale:Integer), 'a' is not an instance of Integer",
        );
        // the front oracle's text (28c68c0d...): a string where the variable is an Integer, its text the string itself
        assert_model(
            arguments("", "meta::pure::precisePrimitives::Varchar('200')").unwrap_err(),
            "Error for type: Varchar(x:Integer), '200' is not an instance of Integer",
        );
        refused("Integer(1)", "Wrong type variables count (1) for type: Integer");
        // DecimalKindsTest.precisePrimitivesResolveByShortName
        for declaration in [
            "Numeric(10, 2)",
            "Varchar(200)",
            "Int",
            "meta::pure::precisePrimitives::Numeric(10, 2)",
        ] {
            arguments("", declaration).unwrap();
        }
        arguments("import meta::pure::precisePrimitives::*;\n", "Numeric(10, 2)").unwrap();
    }

    #[test]
    fn a_rejected_values_text_is_javas_literal_spelling() {
        let index = index("###Pure\nClass x::A\n{\n}\n");
        let tc = TypeClassifier::new(&index);
        let varchar = |value: p::ValueSpecification| {
            tc.require_arguments(&generic_with(&precise("Varchar"), vec![], &[], vec![value]), &[])
        };
        let not_integer =
            |text: &str| format!("Error for type: Varchar(x:Integer), '{text}' is not an instance of Integer");
        varchar(integer(8)).unwrap();
        // String.valueOf(double): Double.toString's spellings
        assert_model(varchar(float(1.5)).unwrap_err(), &not_integer("1.5"));
        assert_model(varchar(float(1e10)).unwrap_err(), &not_integer("1.0E10"));
        assert_model(varchar(float(-0.0001)).unwrap_err(), &not_integer("-1.0E-4"));
        // BigDecimal.toPlainString, never toString's exponent
        let dec =
            |d: BigDecimal| p::ValueSpecification::CDecimal(std::sync::Arc::new(p::CDecimal { value: d, pos: None }));
        assert_model(
            varchar(dec(decimal(false, "1", 7))).unwrap_err(),
            &not_integer("0.0000001"),
        );
        assert_model(
            varchar(dec(decimal(false, "15", -2))).unwrap_err(),
            &not_integer("1500"),
        );
        assert_model(varchar(dec(decimal(true, "25", 1))).unwrap_err(), &not_integer("-2.5"));
        // String.valueOf(boolean), String.valueOf(Number) of a BigInteger
        let boolean = p::ValueSpecification::CBoolean(std::sync::Arc::new(p::CBoolean { value: true, pos: None }));
        assert_model(varchar(boolean).unwrap_err(), &not_integer("true"));
        let big = p::ValueSpecification::CInteger(std::sync::Arc::new(p::CInteger {
            value: JValue::BigInteger("123456789012345678901234567890".to_owned()),
            pos: None,
        }));
        varchar(big).unwrap();
        // a value that is no literal, in its words
        let variable = p::ValueSpecification::Variable(std::sync::Arc::new(p::Variable {
            name: j("x"),
            r#type: None,
            multiplicity: None,
            pos: None,
        }));
        assert_model(
            varchar(variable).unwrap_err(),
            "a type variable's value must be a literal: Varchar(x:Integer), variable 'x'",
        );
        // the count is checked before any value
        assert_model(
            tc.require_arguments(
                &generic_with(&precise("Varchar"), vec![], &[], vec![float(1.5), float(2.5)]),
                &[],
            )
            .unwrap_err(),
            "Wrong type variables count (2) for type: Varchar(x:Integer)",
        );
    }

    #[test]
    fn a_classs_argument_counts_are_its_parameters() {
        // ReferenceIntegrityTest's arity cases (the engine's words, the class printed by its simple name and
        // parameters); Pair and Result are the boot's, declared here as the prelude declares them
        let generic = "Class a::G<T>\n{\n  v: T[1];\n}\n";
        let pair = "Class meta::pure::functions::collection::Pair<U,V>\n{\n  first: U[1];\n  second: V[1];\n}\n";
        let result = "Class meta::pure::mapping::Result<T|m>\n{\n  values: T[m];\n}\n";
        let refused = |prefix: &str, declaration: &str, message: &str| {
            assert_model(
                arguments_in(Dialect::LegendPlatform, prefix, declaration).unwrap_err(),
                message,
            )
        };
        let accepted =
            |prefix: &str, declaration: &str| arguments_in(Dialect::LegendPlatform, prefix, declaration).unwrap();
        refused(generic, "a::G", "Wrong type arguments count (0) for type: G<T>");
        refused(
            generic,
            "a::G<String, Integer>",
            "Wrong type arguments count (2) for type: G<T>",
        );
        refused(
            pair,
            "meta::pure::functions::collection::Pair<String>",
            "Wrong type arguments count (1) for type: Pair<U,V>",
        );
        refused(
            "Class a::C\n{\n}\n",
            "a::C<String>",
            "Wrong type arguments count (1) for type: C",
        );
        accepted(generic, "a::G<String>");
        accepted(pair, "meta::pure::functions::collection::Pair<String, Integer>");
        // a type argument's own arguments are checked in turn
        refused(generic, "a::G<a::G>", "Wrong type arguments count (0) for type: G<T>");
        // a class with multiplicity parameters: the type count first, then the multiplicity count, printed with `|`
        let r = "Class x::R<T,U|m,n>\n{\n  values: T[m];\n  others: U[n];\n}\n";
        refused(
            r,
            "x::R<String|1>",
            "Wrong type arguments count (1) for type: R<T,U|m,n>",
        );
        refused(
            r,
            "x::R<String, Integer|1>",
            "Wrong multiplicity arguments count (1) for type: R<T,U|m,n>",
        );
        accepted(r, "x::R<String, Integer|1, *>");
        // a bare Result is the engine's Result<Any|1..*>; a bare class of the same shape is not
        accepted(result, "meta::pure::mapping::Result");
        refused(
            result,
            "meta::pure::mapping::Result<String>",
            "Wrong multiplicity arguments count (0) for type: Result<T|m>",
        );
        refused(
            "Class x::Q<T|m>\n{\n  values: T[m];\n}\n",
            "x::Q",
            "Wrong type arguments count (0) for type: Q<T|m>",
        );
        // a type parameter in scope names no class, and a primitive (a native class too) takes no arguments
        let index = index(&format!("###Pure\n{generic}"));
        let tc = TypeClassifier::new(&index);
        tc.require_arguments(&name_ref("T"), &[j("T")]).unwrap();
        tc.require_arguments(&name_ref(&m3("Integer")), &[]).unwrap();
        // an unknown name is classify's to refuse, not this check's
        tc.require_arguments(&name_ref("x::Missing"), &[]).unwrap();
        // the structural forms check every type they hold
        let ft = function(
            vec![typed(name_ref("a::G"), concrete(1, Some(1)))],
            name_ref(&m3("String")),
        );
        assert_model(
            tc.require_arguments(&ft, &[]).unwrap_err(),
            "Wrong type arguments count (0) for type: G<T>",
        );
        let ret = function(vec![], name_ref("a::G"));
        assert_model(
            tc.require_arguments(&ret, &[]).unwrap_err(),
            "Wrong type arguments count (0) for type: G<T>",
        );
        assert_model(
            tc.require_arguments(&relation(vec![column("a", name_ref("a::G"))]), &[])
                .unwrap_err(),
            "Wrong type arguments count (0) for type: G<T>",
        );
        let sa = p::TypeExpression::TypeExpressionSchemaAlgebra(Box::new(p::TypeExpressionSchemaAlgebra {
            left: name_ref("T"),
            op: p::TypeExpressionOp::UNION,
            right: name_ref("a::G"),
        }));
        assert_model(
            tc.require_arguments(&sa, &[j("T")]).unwrap_err(),
            "Wrong type arguments count (0) for type: G<T>",
        );
    }

    #[test]
    fn a_head_is_nominal() {
        let index = index("###Pure\nClass a::C\n{\n}\n");
        let _ = TypeClassifier::new(&index);
        assert_eq!(TypeClassifier::head_fqn(&name_ref("a::C")).unwrap(), &j("a::C"));
        assert_eq!(
            TypeClassifier::head_fqn(&generic("a::G", vec![name_ref("a::C")], &[])).unwrap(),
            &j("a::G")
        );
        assert_model(
            TypeClassifier::head_fqn(&function(vec![], name_ref("a::C"))).unwrap_err(),
            "a function type cannot head a supertype/generic reference",
        );
        assert_model(
            TypeClassifier::head_fqn(&relation(vec![])).unwrap_err(),
            "a relation type cannot head a supertype/generic reference",
        );
        let sa = p::TypeExpression::TypeExpressionSchemaAlgebra(Box::new(p::TypeExpressionSchemaAlgebra {
            left: name_ref("a::C"),
            op: p::TypeExpressionOp::EQUAL,
            right: name_ref("a::C"),
        }));
        assert_model(
            TypeClassifier::head_fqn(&sa).unwrap_err(),
            "a schema-algebra expression cannot head a supertype/generic reference",
        );
    }

    #[test]
    fn a_multiplicity_is_the_type_familys() {
        let cx = QueryArena::new();
        assert_eq!(
            TypeClassifier::multiplicity(&cx, &concrete(0, Some(1))).unwrap(),
            Multiplicity::ZERO_ONE
        );
        assert_java(
            TypeClassifier::multiplicity(&cx, &concrete(2, Some(1))).unwrap_err(),
            lite_names::error::ILLEGAL_ARGUMENT,
            "upper (1) must be >= lower (2)",
        );
    }

    #[test]
    fn plain_string_is_big_decimals() {
        // JDK's toPlainString for each placement of the point
        assert_eq!(plain_string(&decimal(false, "0", 0)), "0");
        assert_eq!(plain_string(&decimal(true, "12", 0)), "-12");
        assert_eq!(plain_string(&decimal(false, "0", 2)), "0.00");
        assert_eq!(plain_string(&decimal(false, "0", -3)), "0");
        assert_eq!(plain_string(&decimal(true, "15", -2)), "-1500");
        assert_eq!(plain_string(&decimal(false, "125", 2)), "1.25");
        assert_eq!(plain_string(&decimal(true, "125", 3)), "-0.125");
        assert_eq!(plain_string(&decimal(true, "125", 5)), "-0.00125");
        assert_eq!(plain_string(&decimal(false, "1", 7)), "0.0000001");
        // lite-front's BigDecimal as Java parses it
        let parsed = |s: &str| plain_string(&BigDecimal::parse(s).unwrap());
        assert_eq!(parsed("1E+3"), "1000");
        assert_eq!(parsed("1.50"), "1.50");
        assert_eq!(parsed("-1.5E-7"), "-0.00000015");
    }

    #[test]
    fn a_simple_name_is_split_fqns() {
        assert_eq!(super::simple_name(&j("a::b::C")).unwrap(), j("C"));
        assert_eq!(super::simple_name(&j("C")).unwrap(), j("C"));
        // quote-aware, unquoted and unescaped
        assert_eq!(super::simple_name(&j("a::'b::c'")).unwrap(), j("b::c"));
        assert_eq!(super::simple_name(&j("a::'it\\'s'")).unwrap(), j("it's"));
        // a short unicode escape: Escapes.unescapeJavaLike's raw IllegalArgumentException, in either half
        assert_java(
            super::simple_name(&j("a::'\\u12'")).unwrap_err(),
            lite_names::error::ILLEGAL_ARGUMENT,
            "Less than 4 hex digits in unicode escape",
        );
        assert_java(
            super::simple_name(&j("'\\u1'::C")).unwrap_err(),
            lite_names::error::ILLEGAL_ARGUMENT,
            "Less than 4 hex digits in unicode escape",
        );
    }
}
