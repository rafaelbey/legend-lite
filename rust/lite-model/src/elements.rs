// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The sealed parents' shared accessors, as the Java interfaces declare them: `PackageableElement.qualifiedName`,
//! `Function`'s signature, `ClassMapping`'s and `MappingDefinition.ClassBinding`'s set identity.

use std::sync::Arc;

use lite_front::jtypes::JStr;
use lite_front::records as p;

use crate::records::{
    ClassMapping, Function, FunctionDefinitionParameterDefinition, MappingDefinitionClassBinding, PackageableElement,
    StereotypeApplication, TaggedValue,
};

macro_rules! every_element {
    ($e:expr, $x:ident => $body:expr) => {
        match $e {
            PackageableElement::ClassDefinition($x) => $body,
            PackageableElement::CleanSheetMappingDefinition($x) => $body,
            PackageableElement::AssociationDefinition($x) => $body,
            PackageableElement::EnumDefinition($x) => $body,
            PackageableElement::ProfileDefinition($x) => $body,
            PackageableElement::FunctionDefinition($x) => $body,
            PackageableElement::NativeFunctionDefinition($x) => $body,
            PackageableElement::ServiceDefinition($x) => $body,
            PackageableElement::RuntimeDefinition($x) => $body,
            PackageableElement::ConnectionDefinition($x) => $body,
            PackageableElement::ModelConnectionDefinition($x) => $body,
            PackageableElement::ModelChainConnectionDefinition($x) => $body,
            PackageableElement::ExecutionEnvironmentDefinition($x) => $body,
            PackageableElement::DataSpaceDefinition($x) => $body,
            PackageableElement::MeasureDefinition($x) => $body,
            PackageableElement::PersistenceDefinition($x) => $body,
            PackageableElement::PersistenceContextDefinition($x) => $body,
            PackageableElement::FunctionActivatorDefinition($x) => $body,
            PackageableElement::GenericSectionElementDefinition($x) => $body,
            PackageableElement::DatabaseDefinition($x) => $body,
            PackageableElement::MappingDefinition($x) => $body,
            PackageableElement::LegacyMappingDefinition($x) => $body,
            PackageableElement::PrimitiveExtensionDefinition($x) => $body,
            PackageableElement::DataDefinition($x) => $body,
            PackageableElement::OpaqueElementDefinition($x) => $body,
        }
    };
}

impl PackageableElement {
    /// `qualifiedName()`: the only identity.
    pub fn qualified_name(&self) -> &JStr {
        every_element!(self, x => &x.qualified_name)
    }

    /// The element as a function, when it is one (Java's `instanceof Function`).
    pub fn as_function(&self) -> Option<Function> {
        match self {
            PackageableElement::FunctionDefinition(f) => Some(Function::FunctionDefinition(f.clone())),
            PackageableElement::NativeFunctionDefinition(n) => Some(Function::NativeFunctionDefinition(n.clone())),
            _ => None,
        }
    }

    /// The Java class's simple name (`getClass().getSimpleName()`).
    pub fn java_simple_name(&self) -> &'static str {
        match self {
            PackageableElement::ClassDefinition(_) => "ClassDefinition",
            PackageableElement::CleanSheetMappingDefinition(_) => "CleanSheetMappingDefinition",
            PackageableElement::AssociationDefinition(_) => "AssociationDefinition",
            PackageableElement::EnumDefinition(_) => "EnumDefinition",
            PackageableElement::ProfileDefinition(_) => "ProfileDefinition",
            PackageableElement::FunctionDefinition(_) => "FunctionDefinition",
            PackageableElement::NativeFunctionDefinition(_) => "NativeFunctionDefinition",
            PackageableElement::ServiceDefinition(_) => "ServiceDefinition",
            PackageableElement::RuntimeDefinition(_) => "RuntimeDefinition",
            PackageableElement::ConnectionDefinition(_) => "ConnectionDefinition",
            PackageableElement::ModelConnectionDefinition(_) => "ModelConnectionDefinition",
            PackageableElement::ModelChainConnectionDefinition(_) => "ModelChainConnectionDefinition",
            PackageableElement::ExecutionEnvironmentDefinition(_) => "ExecutionEnvironmentDefinition",
            PackageableElement::DataSpaceDefinition(_) => "DataSpaceDefinition",
            PackageableElement::MeasureDefinition(_) => "MeasureDefinition",
            PackageableElement::PersistenceDefinition(_) => "PersistenceDefinition",
            PackageableElement::PersistenceContextDefinition(_) => "PersistenceContextDefinition",
            PackageableElement::FunctionActivatorDefinition(_) => "FunctionActivatorDefinition",
            PackageableElement::GenericSectionElementDefinition(_) => "GenericSectionElementDefinition",
            PackageableElement::DatabaseDefinition(_) => "DatabaseDefinition",
            PackageableElement::MappingDefinition(_) => "MappingDefinition",
            PackageableElement::LegacyMappingDefinition(_) => "LegacyMappingDefinition",
            PackageableElement::PrimitiveExtensionDefinition(_) => "PrimitiveExtensionDefinition",
            PackageableElement::DataDefinition(_) => "DataDefinition",
            PackageableElement::OpaqueElementDefinition(_) => "OpaqueElementDefinition",
        }
    }
}

macro_rules! both_functions {
    ($e:expr, $x:ident => $body:expr) => {
        match $e {
            Function::FunctionDefinition($x) => $body,
            Function::NativeFunctionDefinition($x) => $body,
        }
    };
}

/// `Function`'s shared signature shape.
impl Function {
    pub fn qualified_name(&self) -> &JStr {
        both_functions!(self, x => &x.qualified_name)
    }

    pub fn type_parameters(&self) -> &Arc<[JStr]> {
        both_functions!(self, x => &x.type_parameters)
    }

    pub fn multiplicity_parameters(&self) -> &Arc<[JStr]> {
        both_functions!(self, x => &x.multiplicity_parameters)
    }

    pub fn parameters(&self) -> &Arc<[Arc<FunctionDefinitionParameterDefinition>]> {
        both_functions!(self, x => &x.parameters)
    }

    pub fn return_multiplicity(&self) -> &p::Multiplicity {
        both_functions!(self, x => &x.return_multiplicity)
    }

    pub fn return_type(&self) -> &p::TypeExpression {
        both_functions!(self, x => &x.return_type)
    }

    pub fn stereotypes(&self) -> &Arc<[Arc<StereotypeApplication>]> {
        both_functions!(self, x => &x.stereotypes)
    }

    pub fn tagged_values(&self) -> &Arc<[Arc<TaggedValue>]> {
        both_functions!(self, x => &x.tagged_values)
    }

    /// The function as a packageable element.
    pub fn as_element(&self) -> PackageableElement {
        match self {
            Function::FunctionDefinition(f) => PackageableElement::FunctionDefinition(f.clone()),
            Function::NativeFunctionDefinition(n) => PackageableElement::NativeFunctionDefinition(n.clone()),
        }
    }
}

macro_rules! every_class_mapping {
    ($e:expr, $x:ident => $body:expr) => {
        match $e {
            ClassMapping::ClassMappingRelational($x) => $body,
            ClassMapping::ClassMappingPure($x) => $body,
            ClassMapping::ClassMappingUnion($x) => $body,
            ClassMapping::ClassMappingRelationFunction($x) => $body,
            ClassMapping::ClassMappingInheritance($x) => $body,
        }
    };
}

/// `ClassMapping`'s shared shape.
impl ClassMapping {
    pub fn class_name(&self) -> &JStr {
        every_class_mapping!(self, x => &x.class_name)
    }

    pub fn set_id(&self) -> Option<&JStr> {
        every_class_mapping!(self, x => x.set_id.as_ref())
    }

    pub fn extends_set_id(&self) -> Option<&JStr> {
        every_class_mapping!(self, x => x.extends_set_id.as_ref())
    }

    pub fn root(&self) -> bool {
        every_class_mapping!(self, x => x.root)
    }
}

macro_rules! every_binding {
    ($e:expr, $x:ident => $body:expr) => {
        match $e {
            MappingDefinitionClassBinding::MappingDefinitionClassBindingRelational($x) => $body,
            MappingDefinitionClassBinding::MappingDefinitionClassBindingPure($x) => $body,
            MappingDefinitionClassBinding::MappingDefinitionClassBindingOperation($x) => $body,
        }
    };
}

/// `MappingDefinition.ClassBinding`'s shared shape.
impl MappingDefinitionClassBinding {
    pub fn class_fqn(&self) -> &JStr {
        every_binding!(self, x => &x.class_fqn)
    }

    pub fn set_id(&self) -> Option<&JStr> {
        every_binding!(self, x => x.set_id.as_ref())
    }

    pub fn extends_set_id(&self) -> Option<&JStr> {
        every_binding!(self, x => x.extends_set_id.as_ref())
    }

    pub fn root(&self) -> bool {
        every_binding!(self, x => x.root)
    }

    pub fn function_fqn(&self) -> &JStr {
        every_binding!(self, x => &x.function_fqn)
    }

    pub fn primary_key_columns(&self) -> &Arc<[JStr]> {
        every_binding!(self, x => &x.primary_key_columns)
    }
}
