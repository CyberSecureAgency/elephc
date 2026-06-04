---
title: "PDO (Databases)"
description: "PDO database access with the SQLite and PostgreSQL drivers: connections, prepared statements, fetch modes, and transactions."
sidebar:
  order: 16
---

elephc supports a practical subset of PHP's PDO database layer, with the
**SQLite** and **PostgreSQL** drivers. `PDO`, `PDOStatement`, and `PDOException`
behave like their PHP counterparts for everyday use: connect, execute,
prepare/bind, fetch, and run transactions. The DSN prefix selects the driver, so
the same code works against either database.

Both drivers are linked statically (SQLite is bundled; PostgreSQL uses a
pure-Rust client), so a compiled PDO binary has **no system database-client
dependency** — it runs anywhere the elephc binary runs. SQLite runs in-process;
PostgreSQL connects to a running server over the network.

## Connecting

```php
<?php
// SQLite — file-backed (created if missing) or in-memory.
$db = new PDO("sqlite:/path/to/app.db");
$mem = new PDO("sqlite::memory:");

// PostgreSQL — credentials in the DSN or as constructor arguments.
$pg = new PDO("pgsql:host=localhost;port=5432;dbname=app;user=me;password=secret");
$pg = new PDO("pgsql:host=localhost;dbname=app", "me", "secret");
```

The DSN must start with `sqlite:` or `pgsql:`. For SQLite, the `$username`,
`$password`, and `$options` arguments are accepted for signature compatibility
but ignored. For PostgreSQL, `$username` / `$password` are folded into the
connection (other keys like `host`, `port`, `dbname`, `sslmode` come from the
`key=value;…` DSN). A failed connection throws a `PDOException`.

## Executing statements

```php
<?php
// exec() runs a statement with no result set and returns the affected row count.
$db->exec("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, score REAL)");
$n = $db->exec("INSERT INTO users (name, score) VALUES ('Ada', 9.5)");

echo $db->lastInsertId();   // "1"
```

## Prepared statements and binding

`execute()` accepts an array of parameters. Positional (`?`) placeholders bind by
position; named (`:name`) placeholders bind by key (with or without the leading
colon). Bound values are typed automatically (int, float, string, null, bool).

```php
<?php
// Positional
$stmt = $db->prepare("SELECT name FROM users WHERE id = ?");
$stmt->execute([1]);

// Named
$ins = $db->prepare("INSERT INTO users (name, score) VALUES (:name, :score)");
$ins->execute([":name" => "Bob", ":score" => 7.25]);
$ins->execute(["name" => "Cyd", "score" => 3.0]);  // colon optional
```

`query()` prepares and immediately executes a statement, returning the
`PDOStatement` ready to fetch.

Parameters can also be bound individually with `bindValue()` (and `bindParam()`),
then applied by an argument-less `execute()`:

```php
<?php
$stmt = $db->prepare("INSERT INTO users (name, score) VALUES (:name, :score)");
$stmt->bindValue(":name", "Dee");
$stmt->bindValue(":score", 5, PDO::PARAM_INT);
$stmt->execute();
```

`bindParam()` binds the variable's *current* value (it does not defer a
by-reference read to `execute()` time), so bind immediately before `execute()`.

Prefer prepared statements over interpolation. When you must embed a string,
`PDO::quote()` wraps it in single quotes and escapes embedded quotes:

```php
<?php
$db->quote("O'Brien");  // 'O''Brien'
```

## Fetching results

```php
<?php
$stmt = $db->query("SELECT id, name FROM users");

$stmt->fetch(PDO::FETCH_ASSOC);  // ["id" => 1, "name" => "Ada"]
$stmt->fetch(PDO::FETCH_NUM);    // [0 => 1, 1 => "Ada"]
$stmt->fetch(PDO::FETCH_BOTH);   // both numeric and string keys
$stmt->fetch(PDO::FETCH_OBJ);    // stdClass { id: 1, name: "Ada" }

$all = $db->query("SELECT id FROM users")->fetchAll(PDO::FETCH_NUM);
$one = $db->query("SELECT name FROM users")->fetchColumn();  // first column of next row

// FETCH_COLUMN yields one column per row as a scalar:
$ids = $db->query("SELECT id FROM users")->fetchAll(PDO::FETCH_COLUMN);  // [1, 2, …]
```

`fetch()` returns `false` when the result set is exhausted. Column values are
returned with their SQLite type: INTEGER → int, REAL → float, TEXT → string,
NULL → null. `FETCH_BOTH` is the default mode.

## Iterating a statement

A `PDOStatement` is Traversable, so `foreach` walks the result set forward with
sequential integer keys, yielding each row in the statement's current fetch mode:

```php
<?php
$stmt = $db->query("SELECT id, name FROM users");
$stmt->setFetchMode(PDO::FETCH_ASSOC);

foreach ($stmt as $i => $row) {
    echo $i, ": ", $row["name"], "\n";
}
```

The cursor is forward-only: each row is consumed as it is yielded, so a statement
can be iterated once.

## PostgreSQL notes

The PostgreSQL driver behaves like the SQLite one, with a few database-specific
points:

- **Placeholders.** PDO `?` and `:name` placeholders are translated to
  PostgreSQL's native `$1, $2, …` at prepare time, so you write the same
  portable SQL for either driver.
- **`lastInsertId()`.** PostgreSQL has no rowid; `lastInsertId()` returns the
  session's last sequence value (`lastval()`), or `lastInsertId($sequence)`
  returns `currval($sequence)`. Use a `SERIAL`/`IDENTITY` column or `RETURNING`.
- **Types.** `integer`/`bigint` → int, `real`/`double precision` → float,
  `boolean` → `0`/`1`, text types → string, `NULL` → null. Columns without a
  direct scalar decoding (e.g. `numeric`, arrays, JSON, date/time) are best read
  with an explicit `::text` (or `::float8`) cast in the query.

## Transactions

```php
<?php
$db->beginTransaction();
try {
    $db->exec("INSERT INTO users (name, score) VALUES ('Dee', 1.0)");
    $db->commit();
} catch (PDOException $e) {
    $db->rollBack();
}
```

## Errors

The default error mode is `PDO::ERRMODE_EXCEPTION`: a failed `exec()`, `prepare()`,
or connection throws a `PDOException` (which extends `RuntimeException`).

```php
<?php
try {
    $db->exec("NOT VALID SQL");
} catch (PDOException $e) {
    echo $e->getMessage();
}
```

`PDO::errorCode()` returns the SQLite result code as a string and
`PDO::errorInfo()` returns `[code, code, message]`.

The error mode is configurable through `ATTR_ERRMODE`:

```php
<?php
$db->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_SILENT);
$rows = $db->exec("UPDATE …");       // false on error instead of throwing
if ($db->exec("BAD SQL") === false) {
    echo $db->errorInfo()[2];
}
```

- `ERRMODE_EXCEPTION` (default) throws a `PDOException`.
- `ERRMODE_SILENT` suppresses it: `exec()`, `query()`, and `prepare()` all return
  `false` on error (check with `=== false`).
- `ERRMODE_WARNING` writes the message to `STDERR` and returns the same failure
  value as `SILENT`.

The mode can also be seeded from the constructor's options array:
`new PDO($dsn, null, null, [PDO::ATTR_ERRMODE => PDO::ERRMODE_SILENT])`.
`getAttribute()` reads it back; `ATTR_DRIVER_NAME` reports `"sqlite"`.

## Supported surface

- **PDO**: `__construct`, `exec`, `query`, `prepare`, `quote`, `lastInsertId`,
  `beginTransaction`, `commit`, `rollBack`, `errorCode`, `errorInfo`,
  `getAttribute`, `setAttribute`.
- **PDOStatement**: `execute`, `bindValue`, `bindParam`, `setFetchMode`, `fetch`,
  `fetchAll`, `fetchColumn`, `rowCount`, `columnCount`; Traversable, so a statement
  can be walked with `foreach`.
- **Fetch modes**: `FETCH_ASSOC`, `FETCH_NUM`, `FETCH_BOTH`, `FETCH_OBJ`,
  `FETCH_COLUMN` (a single column as a scalar; the column index is the second
  argument to `setFetchMode(PDO::FETCH_COLUMN, $col)`).
- **Parameters**: positional `?` and named `:name`; `PARAM_INT` / `PARAM_STR` /
  `PARAM_NULL` / `PARAM_BOOL` constants.
- **Constants**: the fetch-mode, parameter, `ATTR_ERRMODE` / `ATTR_DRIVER_NAME`,
  and `ERRMODE_*` constants used above.

## Limitations

- **SQLite and PostgreSQL only.** The MySQL driver is not yet implemented (the
  bridge is structured to add it as another driver behind the same prelude).
- **`bindParam()`** binds the current value, not a deferred by-reference read.
- **`FETCH_CLASS` / `FETCH_INTO`** are not implemented.
- **`FETCH_OBJ`** materializes the stdClass via a JSON round-trip, so a result
  set whose column names are `0, 1, 2, …` degrades to an array.
- **Binary / BLOB values with embedded NUL bytes** are not round-tripped through
  the text path.
- **`getAttribute` / `setAttribute`** support `ATTR_ERRMODE` and `ATTR_DRIVER_NAME`;
  other attributes are stored and read back but have no effect. Persistent
  connections are not implemented.
- Avoid `new PDOStatement(...)` directly — statements are created by `query()` /
  `prepare()`.
