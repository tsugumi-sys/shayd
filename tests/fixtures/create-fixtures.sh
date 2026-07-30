#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
DB="$SCRIPT_DIR/simple.db"
SQL="$SCRIPT_DIR/simple.sql"
EXPECTED="$SCRIPT_DIR/simple.expected"

rm -f "$DB" "$EXPECTED"

sqlite3 "$DB" < "$SQL"
sqlite3 "$DB" "SELECT rowid, a, b FROM t ORDER BY rowid;" > "$EXPECTED"
