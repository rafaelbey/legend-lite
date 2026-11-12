// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MongoDeclarations` (docs/MONGODB_PLAN_2026_10_16.md, D3, D4 and D6): a `###MongoDB` store's collections as tables
//! whose columns are the declared JSON schema's, and the `MongoDBConnection` a runtime binds such a store to.

use std::collections::HashSet;
use std::sync::Arc;

use lite_front::jtypes::JStr;
use lite_front::records as p;

use crate::error::{Thrown, illegal_argument, illegal_state, unsupported_operation};
use crate::linked::{empty, list};
use crate::records::*;
use crate::text::is;

/// A string column the schema bounds by no `maxLength`: the widest text the plan's types hold.
const UNBOUNDED_TEXT: i32 = 16_777_216;

/// `MongoDeclarations.toConnection`: one server (a replica set's list is not read yet), its credentials as a vault
/// reference, a database type the model knows.
pub fn to_connection(qualified_name: JStr, c: &p::PMongoDbConnection) -> Result<ConnectionDefinition, Thrown> {
    if c.server_urls.len() != 1 {
        return Err(unsupported_operation(crate::cat!(
            "MongoDBConnection '",
            qualified_name,
            "' names ",
            c.server_urls.len().to_string(),
            " servers: one server is supported (a replica set's list is not yet)"
        )));
    }
    let url = &c.server_urls[0];
    let spec = ConnectionSpecification::ConnectionSpecificationStaticDatasource(Arc::new(
        ConnectionSpecificationStaticDatasource {
            host: url.base_url.clone(),
            port: url.port as i32,
            database: c.database_name.clone(),
        },
    ));
    let auth = match &c.auth {
        p::PAuthSpecValue::PMongoAuth(up) => {
            AuthenticationSpec::AuthenticationSpecUsernamePassword(Arc::new(AuthenticationSpecUsernamePassword {
                username: up.username.clone(),
                password_vault_ref: vault_reference(&qualified_name, &up.password)?,
            }))
        }
        p::PAuthSpecValue::PKerberosAuth(_) => {
            AuthenticationSpec::AuthenticationSpecDelegatedKerberos(Arc::new(AuthenticationSpecDelegatedKerberos {
                server_principal: None,
                kerberos_remote_service_name: None,
                kerberos_use_canonical_hostname: None,
            }))
        }
        p::PAuthSpecValue::PApiKeyAuth(_) => return Err(refused_authentication(&qualified_name, "an API key")),
        p::PAuthSpecValue::PEpkAuth(_) => {
            return Err(refused_authentication(&qualified_name, "an encrypted private key"));
        }
        p::PAuthSpecValue::PPskAuth(_) => return Err(refused_authentication(&qualified_name, "a pre-shared key")),
        p::PAuthSpecValue::PGcpWifIslandAuth(_) => {
            return Err(refused_authentication(&qualified_name, "GCP workload identity"));
        }
    };
    // DatabaseType.valueOf: an IllegalArgumentException for a name the model does not know
    let Some(database_type) = ConnectionDefinitionDatabaseType::value_of(&mongo_database_type(c).0) else {
        return Err(illegal_argument(crate::cat!(
            "No enum constant com.legend.model.ConnectionDefinition.DatabaseType.",
            mongo_database_type(c)
        )));
    };
    Ok(ConnectionDefinition {
        qualified_name,
        store_name: c.element.clone(),
        database_type,
        specification: spec,
        authentication: auth,
    })
}

/// `PMongoDbConnection.databaseType()`: the database type a MongoDB connection declares.
fn mongo_database_type(_c: &p::PMongoDbConnection) -> JStr {
    JStr::from("MongoDB")
}

fn vault_reference(qualified_name: &JStr, secret: &p::PVaultSecret) -> Result<JStr, Thrown> {
    match secret {
        p::PVaultSecret::PMongoSecret(s) => Ok(crate::cat!(s.kind, ":", s.value)),
        p::PVaultSecret::PAwsSecret(_) => Err(unsupported_operation(crate::cat!(
            "MongoDBConnection '",
            qualified_name,
            "' keeps its password in AWS Secrets Manager, which is not read yet"
        ))),
    }
}

fn refused_authentication(qualified_name: &JStr, what: &str) -> Thrown {
    unsupported_operation(crate::cat!(
        "MongoDBConnection '",
        qualified_name,
        "' authenticates with ",
        what,
        ", which is not implemented"
    ))
}

/// `MongoDeclarations.toDatabaseDefinition`: a table per collection, in the default schema.
pub fn to_database_definition(db: &p::PMongoDatabase) -> Result<DatabaseDefinition, Thrown> {
    let name = JStr(lite_front::protocol::qualified_name(&db.pkg.0, &db.name.0));
    let mut tables = Vec::with_capacity(db.collections.len());
    for collection in &db.collections {
        tables.push(Arc::new(DatabaseDefinitionTableDefinition {
            name: collection.name.clone(),
            columns: columns(&name, collection)?,
            milestoning: None,
            function: false,
            quoted: false,
            schema_quoted: false,
        }));
    }
    Ok(DatabaseDefinition {
        qualified_name: name,
        includes: empty(),
        schemas: empty(),
        tables: list(tables),
        views: empty(),
        tabular_functions: empty(),
        joins: empty(),
        filters: empty(),
        multi_grain_filters: empty(),
    })
}

fn columns(
    store: &JStr,
    collection: &p::PMongoDatabasePMongoCollection,
) -> Result<Arc<[Arc<DatabaseDefinitionColumnDefinition>]>, Thrown> {
    let schema = &collection.schema;
    let required: HashSet<&JStr> = schema.required.iter().collect();
    let mut out = Vec::with_capacity(schema.properties.len());
    for (name, property) in &schema.properties {
        let key = is(&name.0, "_id");
        out.push(Arc::new(DatabaseDefinitionColumnDefinition {
            name: name.clone(),
            data_type: column_type(store, &collection.name, name, property)?,
            primary_key: key,
            not_null: key || required.contains(name),
            quoted: false,
        }));
    }
    Ok(list(out))
}

fn column_type(
    store: &JStr,
    collection: &JStr,
    property: &JStr,
    s: &p::PBsonSchema,
) -> Result<RelationalDataType, Thrown> {
    use RelationalDataType as T;
    let w = &s.wire_type.0;
    Ok(if is(w, "stringType") {
        T::RelationalDataTypeVarchar(Arc::new(RelationalDataTypeVarchar {
            size: s.max_length.map_or(UNBOUNDED_TEXT, |n| n as i32),
        }))
    } else if is(w, "longType") {
        T::RelationalDataTypeBigInt(Arc::new(RelationalDataTypeBigInt {}))
    } else if is(w, "intType") {
        T::RelationalDataTypeInteger_(Arc::new(RelationalDataTypeInteger_ {}))
    } else if is(w, "boolType") {
        T::RelationalDataTypeBit(Arc::new(RelationalDataTypeBit {}))
    } else if is(w, "doubleType") {
        T::RelationalDataTypeDouble_(Arc::new(RelationalDataTypeDouble_ {}))
    } else if is(w, "schema") || is(w, "objectType") || is(w, "arrayType") {
        // a nested object or array is a Variant, as a relational SEMISTRUCTURED column is
        T::RelationalDataTypeSemiStructured(Arc::new(RelationalDataTypeSemiStructured {}))
    } else if is(w, "objectIdType") {
        // an objectId is read as its hex text: a type Pure cannot name, so OTHER, a String read as text
        T::RelationalDataTypeOther(Arc::new(RelationalDataTypeOther {}))
    } else if is(w, "decimalType") {
        return Err(unsupported_operation(crate::cat!(
            "collection '",
            collection,
            "' of ",
            store,
            ": property '",
            property,
            "' is a decimal, whose scale the schema does not declare: not typed yet (docs/MONGODB_PLAN_2026_10_16.md §11)"
        )));
    } else {
        return Err(illegal_state(crate::cat!(
            "collection '",
            collection,
            "' of ",
            store,
            ": property '",
            property,
            "' has the BSON schema type '",
            s.wire_type,
            "', which the parser does not make"
        )));
    })
}
