// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! core/'s `com.legend.protocol.ConnectionEmitters` and `AuthSpecEmitter`, ported (docs/rust-port/PHASE2.md §4 step 5):
//! a relational connection's three flavor switches -- its authentication strategy, its mapper post-processors' mappers
//! and its datasource specification -- and the authentication-module islands (MongoDB, Elasticsearch: user/password,
//! API key, Kerberos, PSK, GCP workload identity with an AWS IdP, encrypted private key) with their AWS credentials.
//! Fields alphabetical, nulls omitted, each byte Java's.

use crate::emitter::{Buf, Em, illegal_state, mongo_secret, src_info, str, strs_each, vault_secret};
use crate::jcat;
use crate::jtypes::JStr;
use crate::records::*;

/// `ConnectionEmitters.authStrategy`.
pub fn auth_strategy(b: &mut Buf, a: &PAuthStrategy) -> Em {
    match a {
        PAuthStrategy::PH2Default(h) => {
            b.a("{\"_type\":\"h2Default\",\"sourceInformation\":");
            src_info(b, h.source_information.as_deref());
            b.ch(b'}');
        }
        PAuthStrategy::PTestAuth(t) => {
            b.a("{\"_type\":\"test\",\"sourceInformation\":");
            src_info(b, t.source_information.as_deref());
            b.ch(b'}');
        }
        PAuthStrategy::PUserNamePassword(u) => {
            b.a("{\"_type\":\"userNamePassword\"");
            if let Some(base) = &u.base_vault_reference {
                b.a(",\"baseVaultReference\":");
                str(b, base);
            }
            b.a(",\"passwordVaultReference\":");
            str(b, &u.password_vault_reference);
            b.a(",\"sourceInformation\":");
            src_info(b, u.source_information.as_deref());
            b.a(",\"userNameVaultReference\":");
            str(b, &u.user_name_vault_reference);
            b.ch(b'}');
        }
        PAuthStrategy::POAuth(o) => {
            b.a("{\"_type\":\"oauth\",\"oauthKey\":");
            str(b, &o.oauth_key);
            b.a(",\"scopeName\":");
            str(b, &o.scope_name);
            b.a(",\"sourceInformation\":");
            src_info(b, o.source_information.as_deref());
            b.ch(b'}');
        }
        PAuthStrategy::PDelegatedKerberos(k) => {
            b.a("{\"_type\":\"delegatedKerberos\"");
            if let Some(p) = &k.server_principal {
                b.a(",\"serverPrincipal\":");
                str(b, p);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, k.source_information.as_deref());
            b.ch(b'}');
        }
        PAuthStrategy::PSnowflakePublic(s) => {
            b.a("{\"_type\":\"snowflakePublic\",\"passPhraseVaultReference\":");
            str(b, &s.pass_phrase_vault_reference);
            b.a(",\"privateKeyVaultReference\":");
            str(b, &s.private_key_vault_reference);
            b.a(",\"publicUserName\":");
            str(b, &s.public_user_name);
            if let Some(si) = &s.source_information {
                // a mode:local synthesized auth is spanless
                b.a(",\"sourceInformation\":");
                src_info(b, Some(si));
            }
            b.ch(b'}');
        }
        PAuthStrategy::PGCPApplicationDefaultCredentials(g) => {
            b.a("{\"_type\":\"gcpApplicationDefaultCredentials\",\"sourceInformation\":");
            src_info(b, g.source_information.as_deref());
            b.ch(b'}');
        }
        PAuthStrategy::PApiToken(a2) => {
            b.a("{\"_type\":\"apiToken\",\"apiToken\":");
            str(b, &a2.api_token);
            b.a(",\"sourceInformation\":");
            src_info(b, a2.source_information.as_deref());
            b.ch(b'}');
        }
        PAuthStrategy::PMiddleTierUserNamePassword(m) => {
            b.a("{\"_type\":\"middleTierUserNamePassword\",\"sourceInformation\":");
            src_info(b, m.source_information.as_deref());
            b.a(",\"vaultReference\":");
            str(b, &m.vault_reference);
            b.ch(b'}');
        }
        PAuthStrategy::PTrinoKerberosAuth(k) => {
            b.a("{\"_type\":\"TrinoDelegatedKerberosAuth\"");
            if let Some(n) = &k.kerberos_remote_service_name {
                b.a(",\"kerberosRemoteServiceName\":");
                str(b, n);
            }
            if let Some(c) = k.kerberos_use_canonical_hostname {
                b.a(",\"kerberosUseCanonicalHostname\":");
                b.boolean(c);
            }
            if let Some(p) = &k.server_principal {
                b.a(",\"serverPrincipal\":");
                str(b, p);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, k.source_information.as_deref());
            b.ch(b'}');
        }
        PAuthStrategy::PGcpWifAuth(w) => {
            b.a("{\"_type\":\"gcpWorkloadIdentityFederation\"");
            if let Some(scopes) = &w.additional_gcp_scopes {
                b.a(",\"additionalGcpScopes\":[");
                strs_each(b, scopes);
                b.ch(b']');
            }
            b.a(",\"serviceAccountEmail\":");
            str(b, &w.service_account_email);
            b.a(",\"sourceInformation\":");
            src_info(b, w.source_information.as_deref());
            b.ch(b'}');
        }
    }
    Ok(())
}

/// `ConnectionEmitters.mapper`: a mapper post-processor's table or schema mapper.
pub fn mapper(b: &mut Buf, m: &PMapper) -> Em {
    match m {
        PMapper::PTableMapper(t) => {
            b.a("{\"_type\":\"table\",\"from\":");
            str(b, &t.from);
            b.a(",\"schema\":{\"_type\":\"schema\",\"from\":");
            str(b, &t.schema_from);
            b.a(",\"to\":");
            str(b, &t.schema_to);
            b.a("},\"to\":");
            str(b, &t.to);
            b.ch(b'}');
        }
        PMapper::PSchemaMapper(sm) => {
            b.a("{\"_type\":\"schema\",\"from\":");
            str(b, &sm.from);
            b.a(",\"to\":");
            str(b, &sm.to);
            b.ch(b'}');
        }
    }
    Ok(())
}

/// `ConnectionEmitters.datasourceSpec`.
pub fn datasource_spec(b: &mut Buf, d: &PDatasourceSpec) -> Em {
    match d {
        PDatasourceSpec::PH2Local(h) => {
            b.a("{\"_type\":\"h2Local\",\"sourceInformation\":");
            src_info(b, h.source_information.as_deref());
            if let Some(csv) = &h.test_data_setup_csv {
                b.a(",\"testDataSetupCsv\":");
                str(b, csv);
            }
            if let Some(sqls) = &h.test_data_setup_sqls {
                b.a(",\"testDataSetupSqls\":[");
                strs_each(b, sqls);
                b.ch(b']');
            }
            b.ch(b'}');
        }
        PDatasourceSpec::PDuckDBSpec(dd) => {
            b.a("{\"_type\":\"duckDB\"");
            if let Some(p) = &dd.path {
                b.a(",\"path\":");
                str(b, p);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, dd.source_information.as_deref());
            b.ch(b'}');
        }
        // a lite backend with no engine wire shape: refused, never an invented _type
        PDatasourceSpec::PSQLiteSpec(_) => {
            return Err(illegal_state(JStr::from("SQLite datasource specs have no engine wire shape (lite backend)")));
        }
        PDatasourceSpec::PH2EmbeddedSpec(eh) => {
            b.a("{\"_type\":\"h2Embedded\",\"autoServerMode\":");
            b.boolean(eh.auto_server_mode);
            b.a(",\"databaseName\":");
            str(b, &eh.database_name);
            b.a(",\"directory\":");
            str(b, &eh.directory);
            b.a(",\"sourceInformation\":");
            src_info(b, eh.source_information.as_deref());
            b.ch(b'}');
        }
        PDatasourceSpec::PStaticSpec(st) => {
            b.a("{\"_type\":\"static\",\"databaseName\":");
            str(b, &st.database_name);
            b.a(",\"host\":");
            str(b, &st.host);
            b.a(",\"port\":");
            b.int(st.port);
            b.a(",\"sourceInformation\":");
            src_info(b, st.source_information.as_deref());
            b.ch(b'}');
        }
        PDatasourceSpec::PSnowflakeSpec(s) => snowflake_spec(b, s),
        PDatasourceSpec::PSpannerSpec(s) => {
            b.a("{\"_type\":\"spanner\",\"databaseId\":");
            str(b, &s.database_id);
            b.a(",\"instanceId\":");
            str(b, &s.instance_id);
            b.a(",\"projectId\":");
            str(b, &s.project_id);
            if let Some(h) = &s.proxy_host {
                b.a(",\"proxyHost\":");
                str(b, h);
            }
            if let Some(p) = s.proxy_port {
                b.a(",\"proxyPort\":");
                b.int(p);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, s.source_information.as_deref());
            b.ch(b'}');
        }
        PDatasourceSpec::PRedshiftSpec(s) => {
            b.a("{\"_type\":\"redshift\",\"clusterID\":");
            str(b, &s.cluster_id);
            b.a(",\"databaseName\":");
            str(b, &s.database_name);
            if let Some(e) = &s.endpoint_url {
                b.a(",\"endpointURL\":");
                str(b, e);
            }
            b.a(",\"host\":");
            str(b, &s.host);
            b.a(",\"port\":");
            b.int(s.port);
            b.a(",\"region\":");
            str(b, &s.region);
            // the redshift extension's wire omits sourceInformation
            b.ch(b'}');
        }
        PDatasourceSpec::PAthenaSpec(_)
        | PDatasourceSpec::PAuroraSpec(_)
        | PDatasourceSpec::PGlobalAuroraSpec(_)
        | PDatasourceSpec::PMemSqlSpec(_)
        | PDatasourceSpec::POracleSpec(_) => vendor_datasource_spec(b, d)?,
        PDatasourceSpec::PTrinoSpec(s) => {
            b.a("{\"_type\":\"Trino\"");
            if let Some(c) = &s.catalog {
                b.a(",\"catalog\":");
                str(b, c);
            }
            if let Some(t) = &s.client_tags {
                b.a(",\"clientTags\":");
                str(b, t);
            }
            b.a(",\"host\":");
            str(b, &s.host);
            b.a(",\"port\":");
            b.int(s.port);
            if let Some(sc) = &s.schema {
                b.a(",\"schema\":");
                str(b, sc);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, s.source_information.as_deref());
            if let Some(ssl) = &s.ssl_specification {
                b.a(",\"sslSpecification\":{\"ssl\":");
                b.boolean(ssl.ssl);
                if let Some(p) = &ssl.trust_store_password_vault_reference {
                    b.a(",\"trustStorePasswordVaultReference\":");
                    str(b, p);
                }
                if let Some(p) = &ssl.trust_store_path_vault_reference {
                    b.a(",\"trustStorePathVaultReference\":");
                    str(b, p);
                }
                b.ch(b'}');
            }
            b.ch(b'}');
        }
        PDatasourceSpec::PDatabricksSpec(s) => {
            b.a("{\"_type\":\"databricks\",\"hostname\":");
            str(b, &s.hostname);
            b.a(",\"httpPath\":");
            str(b, &s.http_path);
            b.a(",\"port\":");
            str(b, &s.port);
            b.a(",\"protocol\":");
            str(b, &s.protocol);
            b.a(",\"sourceInformation\":");
            src_info(b, s.source_information.as_deref());
            b.ch(b'}');
        }
        PDatasourceSpec::PBigQuerySpec(s) => {
            b.a("{\"_type\":\"bigQuery\",\"defaultDataset\":");
            str(b, &s.default_dataset);
            b.a(",\"projectId\":");
            str(b, &s.project_id);
            if let Some(h) = &s.proxy_host {
                b.a(",\"proxyHost\":");
                str(b, h);
            }
            if let Some(p) = &s.proxy_port {
                b.a(",\"proxyPort\":");
                str(b, p);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, s.source_information.as_deref());
            b.ch(b'}');
        }
    }
    Ok(())
}

fn snowflake_spec(b: &mut Buf, s: &PSnowflakeSpec) {
    b.a("{\"_type\":\"snowflake\",\"accountName\":");
    str(b, &s.account_name);
    if let Some(t) = &s.account_type {
        b.a(",\"accountType\":");
        str(b, t);
    }
    if let Some(c) = &s.cloud_type {
        b.a(",\"cloudType\":");
        str(b, c);
    }
    b.a(",\"databaseName\":");
    str(b, &s.database_name);
    if let Some(e) = s.enable_query_tags {
        b.a(",\"enableQueryTags\":");
        b.boolean(e);
    }
    if let Some(n) = &s.non_proxy_hosts {
        b.a(",\"nonProxyHosts\":");
        str(b, n);
    }
    if let Some(o) = &s.organization {
        b.a(",\"organization\":");
        str(b, o);
    }
    if let Some(h) = &s.proxy_host {
        b.a(",\"proxyHost\":");
        str(b, h);
    }
    if let Some(p) = &s.proxy_port {
        b.a(",\"proxyPort\":");
        str(b, p);
    }
    if let Some(q) = s.quoted_identifiers_ignore_case {
        b.a(",\"quotedIdentifiersIgnoreCase\":");
        b.boolean(q);
    }
    b.a(",\"region\":");
    str(b, &s.region);
    if let Some(r) = &s.role {
        b.a(",\"role\":");
        str(b, r);
    }
    if let Some(si) = &s.source_information {
        // mode:local synthesized specs are spanless
        b.a(",\"sourceInformation\":");
        src_info(b, Some(si));
    }
    if let Some(t) = &s.temp_table_db {
        b.a(",\"tempTableDb\":");
        str(b, t);
    }
    if let Some(t) = &s.temp_table_schema {
        b.a(",\"tempTableSchema\":");
        str(b, t);
    }
    b.a(",\"warehouseName\":");
    str(b, &s.warehouse_name);
    b.ch(b'}');
}

/// `PDatasourceSpec`'s Java class's `getSimpleName()`.
fn datasource_simple_name(d: &PDatasourceSpec) -> &'static str {
    match d {
        PDatasourceSpec::PH2Local(_) => "PH2Local",
        PDatasourceSpec::PStaticSpec(_) => "PStaticSpec",
        PDatasourceSpec::PH2EmbeddedSpec(_) => "PH2EmbeddedSpec",
        PDatasourceSpec::PDuckDBSpec(_) => "PDuckDBSpec",
        PDatasourceSpec::PSQLiteSpec(_) => "PSQLiteSpec",
        PDatasourceSpec::PSnowflakeSpec(_) => "PSnowflakeSpec",
        PDatasourceSpec::PSpannerSpec(_) => "PSpannerSpec",
        PDatasourceSpec::PDatabricksSpec(_) => "PDatabricksSpec",
        PDatasourceSpec::PBigQuerySpec(_) => "PBigQuerySpec",
        PDatasourceSpec::PRedshiftSpec(_) => "PRedshiftSpec",
        PDatasourceSpec::PTrinoSpec(_) => "PTrinoSpec",
        PDatasourceSpec::PAthenaSpec(_) => "PAthenaSpec",
        PDatasourceSpec::PAuroraSpec(_) => "PAuroraSpec",
        PDatasourceSpec::PGlobalAuroraSpec(_) => "PGlobalAuroraSpec",
        PDatasourceSpec::PMemSqlSpec(_) => "PMemSqlSpec",
        PDatasourceSpec::POracleSpec(_) => "POracleSpec",
    }
}

/// `ConnectionEmitters.vendorDatasourceSpec`: Athena, Aurora, GlobalAurora, MemSql, Oracle.
fn vendor_datasource_spec(b: &mut Buf, d: &PDatasourceSpec) -> Em {
    match d {
        PDatasourceSpec::PAthenaSpec(s) => {
            b.a("{\"_type\":\"athena\"");
            if let Some(e) = &s.athena_endpoint {
                b.a(",\"athenaEndpoint\":");
                str(b, e);
            }
            if let Some(c) = &s.catalog {
                b.a(",\"catalog\":");
                str(b, c);
            }
            if let Some(db) = &s.database {
                b.a(",\"database\":");
                str(b, db);
            }
            if let Some(o) = &s.output_location {
                b.a(",\"outputLocation\":");
                str(b, o);
            }
            b.a(",\"region\":");
            str(b, &s.region);
            b.a(",\"sourceInformation\":");
            src_info(b, s.source_information.as_deref());
            if let Some(w) = &s.work_group {
                b.a(",\"workGroup\":");
                str(b, w);
            }
            b.ch(b'}');
        }
        PDatasourceSpec::PAuroraSpec(s) => {
            b.a("{\"_type\":\"aurora\"");
            if let Some(p) = &s.cluster_instance_host_pattern {
                b.a(",\"clusterInstanceHostPattern\":");
                str(b, p);
            }
            b.a(",\"host\":");
            str(b, &s.host);
            b.a(",\"name\":");
            str(b, &s.name);
            b.a(",\"port\":");
            b.int(s.port);
            b.a(",\"sourceInformation\":");
            src_info(b, s.source_information.as_deref());
            b.ch(b'}');
        }
        PDatasourceSpec::PGlobalAuroraSpec(s) => {
            b.a("{\"_type\":\"globalAurora\",\"globalClusterInstanceHostPatterns\":[");
            strs_each(b, &s.global_cluster_instance_host_patterns);
            b.a("],\"host\":");
            str(b, &s.host);
            b.a(",\"name\":");
            str(b, &s.name);
            b.a(",\"port\":");
            b.int(s.port);
            b.a(",\"region\":");
            str(b, &s.region);
            b.a(",\"sourceInformation\":");
            src_info(b, s.source_information.as_deref());
            b.ch(b'}');
        }
        PDatasourceSpec::PMemSqlSpec(s) => {
            b.a("{\"_type\":\"memSql\"");
            if let Some(db) = &s.database_name {
                b.a(",\"databaseName\":");
                str(b, db);
            }
            b.a(",\"host\":");
            str(b, &s.host);
            b.a(",\"port\":");
            b.int(s.port);
            b.a(",\"sourceInformation\":");
            src_info(b, s.source_information.as_deref());
            if let Some(ssl) = s.use_ssl {
                b.a(",\"useSsl\":");
                b.boolean(ssl);
            }
            b.ch(b'}');
        }
        PDatasourceSpec::POracleSpec(s) => {
            b.a("{\"_type\":\"oracle\",\"host\":");
            str(b, &s.host);
            b.a(",\"port\":");
            b.int(s.port);
            if let Some(n) = &s.service_name {
                b.a(",\"serviceName\":");
                str(b, n);
            }
            b.a(",\"sourceInformation\":");
            src_info(b, s.source_information.as_deref());
            b.ch(b'}');
        }
        other => {
            return Err(illegal_state(jcat!("not a vendor datasource spec: ", datasource_simple_name(other))));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// AuthSpecEmitter
// ---------------------------------------------------------------------------------------------------------------------

/// `AuthSpecEmitter.awsCredentials`: awsDefault spanless; awsStatic and awsSTSAssumeRole spanning their block.
fn aws_credentials(b: &mut Buf, creds: &PAwsCredentials) {
    match creds {
        PAwsCredentials::PAwsDefault(_) => b.a("{\"_type\":\"awsDefault\"}"),
        PAwsCredentials::PAwsStatic(st) => {
            b.a("{\"_type\":\"awsStatic\",\"accessKeyId\":");
            mongo_secret(b, &st.access_key_id);
            b.a(",\"secretAccessKey\":");
            mongo_secret(b, &st.secret_access_key);
            b.a(",\"sourceInformation\":");
            src_info(b, st.source_information.as_deref());
            b.ch(b'}');
        }
        PAwsCredentials::PAwsStsRole(sts) => {
            b.a("{\"_type\":\"awsSTSAssumeRole\",\"awsCredentials\":");
            aws_credentials(b, &sts.aws_credentials);
            b.a(",\"roleArn\":");
            str(b, &sts.role_arn);
            b.a(",\"roleSessionName\":");
            str(b, &sts.role_session_name);
            b.a(",\"sourceInformation\":");
            src_info(b, sts.source_information.as_deref());
            b.ch(b'}');
        }
    }
}

/// `AuthSpecEmitter.esAuthSpec`: the authentication-module island wire.
pub fn es_auth_spec(b: &mut Buf, auth: &PAuthSpecValue) -> Em {
    match auth {
        PAuthSpecValue::PMongoAuth(up) => {
            b.a("{\"_type\":\"userPassword\",\"password\":");
            vault_secret(b, &up.password);
            b.a(",\"sourceInformation\":");
            src_info(b, up.source_information.as_deref());
            b.a(",\"username\":");
            str(b, &up.username);
            b.ch(b'}');
        }
        PAuthSpecValue::PApiKeyAuth(ak) => {
            b.a("{\"_type\":\"apiKey\",\"keyName\":");
            str(b, &ak.key_name);
            b.a(",\"location\":");
            str(b, &ak.location);
            b.a(",\"sourceInformation\":");
            src_info(b, ak.source_information.as_deref());
            b.a(",\"value\":");
            vault_secret(b, &ak.value);
            b.ch(b'}');
        }
        PAuthSpecValue::PKerberosAuth(k) => {
            b.a("{\"_type\":\"kerberos\",\"sourceInformation\":");
            src_info(b, k.source_information.as_deref());
            b.ch(b'}');
        }
        PAuthSpecValue::PPskAuth(psk) => {
            b.a("{\"_type\":\"PSK\",\"psk\":");
            str(b, &psk.psk);
            b.ch(b'}');
        }
        PAuthSpecValue::PGcpWifIslandAuth(g) => {
            b.a("{\"_type\":\"gcpWithAWSIdP\",\"additionalGcpScopes\":[],\"idPConfiguration\":{\"accountId\":");
            str(b, &g.account_id);
            b.a(",\"awsCredentials\":");
            aws_credentials(b, &g.aws_credentials);
            b.a(",\"region\":");
            str(b, &g.region);
            b.a(",\"role\":");
            str(b, &g.role);
            b.a("},\"serviceAccountEmail\":");
            str(b, &g.service_account_email);
            b.a(",\"sourceInformation\":");
            src_info(b, g.source_information.as_deref());
            b.a(",\"workloadConfiguration\":{\"poolId\":");
            str(b, &g.pool_id);
            b.a(",\"projectNumber\":");
            str(b, &g.project_number);
            b.a(",\"providerId\":");
            str(b, &g.provider_id);
            b.a("}}");
        }
        PAuthSpecValue::PEpkAuth(ek) => {
            b.a("{\"_type\":\"encryptedPrivateKey\",\"passphrase\":");
            vault_secret(b, &ek.passphrase);
            b.a(",\"privateKey\":");
            vault_secret(b, &ek.private_key);
            b.a(",\"sourceInformation\":");
            src_info(b, ek.source_information.as_deref());
            b.a(",\"userName\":");
            str(b, &ek.user_name);
            b.ch(b'}');
        }
    }
    Ok(())
}
