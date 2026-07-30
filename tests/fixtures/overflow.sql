PRAGMA page_size = 512;
PRAGMA encoding = 'UTF-8';

CREATE TABLE large (
  a INTEGER,
  b TEXT
);

INSERT INTO large (a, b)
VALUES (1, replace(hex(zeroblob(900)), '0', 'x'));
