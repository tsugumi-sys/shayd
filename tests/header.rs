use oxlite::DatabaseHeader;

#[test]
fn parses_minimal_sqlite_header_bytes() {
    let mut bytes = [0; DatabaseHeader::SIZE];
    bytes[0..16].copy_from_slice(b"SQLite format 3\0");
    bytes[16..18].copy_from_slice(&4096_u16.to_be_bytes());
    bytes[18] = 1;
    bytes[19] = 1;
    bytes[21] = 64;
    bytes[22] = 32;
    bytes[23] = 32;
    bytes[44..48].copy_from_slice(&4_u32.to_be_bytes());
    bytes[56..60].copy_from_slice(&1_u32.to_be_bytes());

    let header = DatabaseHeader::parse(&bytes).unwrap();
    assert_eq!(header.page_size.get(), 4096);
    assert_eq!(header.usable_space(), 4096);
}
