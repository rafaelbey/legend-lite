// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

package com.legend.sql.dialect;

/**
 * The MySQL EXECUTION dialect -- since 2026-11-04 the Rust library's (docs/rust-port/PHASE1.md §4 step 6; rust/lite-dialect
 * src/mariadb.rs, its MySQL profile): {@link MariaDb}'s with MySQL's own spellings, held to this class's Java on every
 * oracle input as MariaDB is. It extends MariaDb, as its Java did: the dialect a MySQL database is MariaDB's family.
 */
public final class MySql extends MariaDb {

    public MySql() {
        super("MySql");
    }
}
