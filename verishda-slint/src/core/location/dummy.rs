use super::Location;

#[derive(Debug)]
pub(crate) struct DummyPollingLocator;

impl super::PollingLocator for DummyPollingLocator {
    fn new() -> Self {
        Self
    }

    async fn poll_location(&self) -> super::Location {
        Location::default()
    }

}