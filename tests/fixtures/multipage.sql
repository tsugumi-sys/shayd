PRAGMA page_size = 512;
PRAGMA encoding = 'UTF-8';

CREATE TABLE big (
  a INTEGER,
  b TEXT
);

WITH RECURSIVE rows(value) AS (
  SELECT 1
  UNION ALL
  SELECT value + 1 FROM rows WHERE value < 120
)
INSERT INTO big (a, b)
SELECT value, printf('row-%03d-abcdefghijklmnopqrstuvwxyz', value)
FROM rows;
