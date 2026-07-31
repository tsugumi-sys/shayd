PRAGMA page_size = 512;
PRAGMA encoding = 'UTF-8';

CREATE TABLE items (
  a INTEGER,
  b TEXT
);

CREATE INDEX idx_items_a ON items(a);
CREATE UNIQUE INDEX idx_items_b ON items(b);

INSERT INTO items VALUES (10, 'alpha');
INSERT INTO items VALUES (20, 'beta');
INSERT INTO items VALUES (30, 'gamma');
