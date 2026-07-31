use crate::error::{Error, Result};
use crate::header::{MAX_EMBEDDED_PAYLOAD_FRACTION, MIN_EMBEDDED_PAYLOAD_FRACTION};

const PAYLOAD_FRACTION_DENOMINATOR: usize = 255;
const LOCAL_PAYLOAD_ADJUSTMENT: usize = 23;
const INDEX_INTERIOR_HEADER_SIZE: usize = 12;
const OVERFLOW_POINTER_SIZE: usize = 4;
const TABLE_LEAF_MAX_LOCAL_PAYLOAD_SUBTRACT: usize = 35;

pub(super) fn table_leaf_local_payload_size(
    payload_size: usize,
    usable_size: usize,
) -> Result<usize> {
    if usable_size < TABLE_LEAF_MAX_LOCAL_PAYLOAD_SUBTRACT {
        return Err(Error::InvalidBtreePage("usable size is too small"));
    }

    let max_leaf = usable_size - TABLE_LEAF_MAX_LOCAL_PAYLOAD_SUBTRACT;
    let min_leaf = ((usable_size - INDEX_INTERIOR_HEADER_SIZE) * MIN_EMBEDDED_PAYLOAD_FRACTION
        / PAYLOAD_FRACTION_DENOMINATOR)
        .checked_sub(LOCAL_PAYLOAD_ADJUSTMENT)
        .ok_or(Error::InvalidBtreePage("usable size is too small"))?;

    if payload_size <= max_leaf {
        return Ok(payload_size);
    }

    let overflow_payload_capacity = usable_size
        .checked_sub(OVERFLOW_POINTER_SIZE)
        .ok_or(Error::InvalidBtreePage("usable size is too small"))?;
    let surplus = min_leaf + ((payload_size - min_leaf) % overflow_payload_capacity);
    if surplus <= max_leaf {
        Ok(surplus)
    } else {
        Ok(min_leaf)
    }
}

pub(super) fn index_local_payload_size(payload_size: usize, usable_size: usize) -> Result<usize> {
    if usable_size < TABLE_LEAF_MAX_LOCAL_PAYLOAD_SUBTRACT {
        return Err(Error::InvalidBtreePage("usable size is too small"));
    }

    let max_local = ((usable_size - INDEX_INTERIOR_HEADER_SIZE) * MAX_EMBEDDED_PAYLOAD_FRACTION
        / PAYLOAD_FRACTION_DENOMINATOR)
        .checked_sub(LOCAL_PAYLOAD_ADJUSTMENT)
        .ok_or(Error::InvalidBtreePage("usable size is too small"))?;
    let min_local = ((usable_size - INDEX_INTERIOR_HEADER_SIZE) * MIN_EMBEDDED_PAYLOAD_FRACTION
        / PAYLOAD_FRACTION_DENOMINATOR)
        .checked_sub(LOCAL_PAYLOAD_ADJUSTMENT)
        .ok_or(Error::InvalidBtreePage("usable size is too small"))?;

    if payload_size <= max_local {
        return Ok(payload_size);
    }

    let overflow_payload_capacity = usable_size
        .checked_sub(OVERFLOW_POINTER_SIZE)
        .ok_or(Error::InvalidBtreePage("usable size is too small"))?;
    let surplus = min_local + ((payload_size - min_local) % overflow_payload_capacity);
    if surplus <= max_local {
        Ok(surplus)
    } else {
        Ok(min_local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_table_leaf_local_payload_size_like_sqlite() {
        assert_eq!(table_leaf_local_payload_size(477, 512).unwrap(), 477);
        assert_eq!(table_leaf_local_payload_size(478, 512).unwrap(), 39);
        assert_eq!(table_leaf_local_payload_size(545, 512).unwrap(), 39);
        assert_eq!(table_leaf_local_payload_size(985, 512).unwrap(), 477);
        assert_eq!(table_leaf_local_payload_size(986, 512).unwrap(), 39);
    }
}
