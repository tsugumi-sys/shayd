PRAGMA page_size = 4096;
PRAGMA encoding = 'UTF-8';

CREATE TABLE t (
  a INTEGER,
  b TEXT
);

INSERT INTO t (a, b) VALUES
  (10, 'alpha'),
  (20, 'beta');
