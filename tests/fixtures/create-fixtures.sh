#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

create_fixture() {
  name=$1
  query=$2

  db="$SCRIPT_DIR/$name.db"
  sql="$SCRIPT_DIR/$name.sql"
  expected="$SCRIPT_DIR/$name.expected"

  rm -f "$db" "$expected"

  sqlite3 "$db" < "$sql"
  sqlite3 "$db" "$query" > "$expected"
}

create_fixture simple "SELECT rowid, a, b FROM t ORDER BY rowid;"
create_fixture multipage "SELECT rowid, a, b FROM big ORDER BY rowid;"
create_fixture overflow "SELECT rowid, a, b FROM large ORDER BY rowid;"
create_fixture indexed "SELECT rowid, a, b FROM items ORDER BY rowid;"
