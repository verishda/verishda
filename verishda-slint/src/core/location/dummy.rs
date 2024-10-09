use super::Location;

#[derive(Debug)]
pub(crate) struct DummyPollingLocator;

impl super::PollingLocator for DummyPollingLocator {
    fn new() -> Self {
        Self
    }

    async fn poll_location(&self) -> anyhow::Result<super::Location> {
        Ok(Location::default())
    }

}