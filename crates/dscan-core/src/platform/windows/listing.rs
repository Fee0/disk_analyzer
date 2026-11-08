use std::io;

pub(super) const ERROR_NOT_SUPPORTED: i32 = 50;
pub(super) const ERROR_INVALID_PARAMETER: i32 = 87;
pub(super) const ERROR_INVALID_LEVEL: i32 = 124;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Listing {
    WithUnique128BitIds,
    ForFileSystemsWithoutUniqueIds,
}

pub(super) enum Batch {
    Filled,
    End,
}

fn file_system_lacks_the_listing(error: &io::Error) -> bool {
    [ERROR_INVALID_PARAMETER, ERROR_INVALID_LEVEL, ERROR_NOT_SUPPORTED]
        .iter()
        .any(|&code| error.raw_os_error() == Some(code))
}

pub(super) fn read_batches(
    buffer: &mut [u64],
    mut next_batch: impl FnMut(Listing, &mut [u64]) -> io::Result<Batch>,
    mut visit_batch: impl FnMut(Listing, &[u64]),
) -> io::Result<()> {
    let mut listing = Listing::WithUnique128BitIds;
    let mut may_fall_back = true;
    loop {
        match next_batch(listing, buffer) {
            Ok(Batch::Filled) => visit_batch(listing, buffer),
            Ok(Batch::End) => return Ok(()),
            Err(e) if may_fall_back && file_system_lacks_the_listing(&e) => {
                listing = Listing::ForFileSystemsWithoutUniqueIds;
            }
            Err(e) => return Err(e),
        }
        may_fall_back = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lacks_the_listing() -> io::Error {
        io::Error::from_raw_os_error(ERROR_NOT_SUPPORTED)
    }

    #[test]
    fn a_file_system_lacking_both_listings_fails_instead_of_retrying_forever() {
        let mut asked = Vec::new();
        let outcome = read_batches(
            &mut [0u64; 1],
            |listing, _| {
                asked.push(listing);
                assert!(asked.len() <= 2, "gave up after the fallback failed too");
                Err(lacks_the_listing())
            },
            |_, _| panic!("nothing was listed"),
        );
        assert!(outcome.is_err_and(|e| file_system_lacks_the_listing(&e)));
        assert_eq!(asked, [Listing::WithUnique128BitIds, Listing::ForFileSystemsWithoutUniqueIds]);
    }

    #[test]
    fn falls_back_once_and_lists_every_batch_with_the_fallback() {
        let mut answers = vec![Ok(Batch::End), Ok(Batch::Filled), Ok(Batch::Filled), Err(lacks_the_listing())];
        let mut visited = Vec::new();
        read_batches(&mut [0u64; 1], |_, _| answers.pop().unwrap(), |listing, _| visited.push(listing)).unwrap();
        assert_eq!(visited, [Listing::ForFileSystemsWithoutUniqueIds; 2]);
    }

    #[test]
    fn only_the_first_batch_may_fall_back() {
        let mut answers = vec![Err(lacks_the_listing()), Ok(Batch::Filled)];
        let outcome = read_batches(&mut [0u64; 1], |_, _| answers.pop().unwrap(), |_, _| {});
        assert!(outcome.is_err(), "a later failure is a real error, not a missing listing");
    }

    #[test]
    fn any_other_first_failure_is_reported_without_falling_back() {
        let mut asked = 0;
        let outcome = read_batches(
            &mut [0u64; 1],
            |_, _| {
                asked += 1;
                Err(io::ErrorKind::PermissionDenied.into())
            },
            |_, _| {},
        );
        assert!(outcome.is_err() && asked == 1, "only a missing listing is worth a second try");
    }
}
