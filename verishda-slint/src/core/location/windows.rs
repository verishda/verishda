use windows::Devices::Geolocation::{BasicGeoposition, Geolocator};

use super::Location;

impl From<&BasicGeoposition> for Location {
    fn from(pos: &BasicGeoposition) -> Self {
        Location {
            latitude: pos.Latitude,
            longitude: pos.Longitude,
        }
    }
}


#[derive(Debug)]
pub(crate) struct WindowsPollingLocator {
    loc: Option<Geolocator>,
}

// https://learn.microsoft.com/en-us/previous-versions/windows/apps/dn263199(v=win.10)
// https://docs.microsoft.com/en-us/uwp/api/windows.devices.geolocation.geofencing.geofencemonitor
impl super::PollingLocator for WindowsPollingLocator {
    fn new() -> Self {
        Self {
            loc: None
        }
    }

    fn start(&mut self) {
        self.loc = Some(Geolocator::new().unwrap());
    }

    fn stop(&mut self) {
        self.loc = None;
    }

    // https://learn.microsoft.com/en-us/previous-versions/windows/apps/dn263199(v=win.10)
    // https://docs.microsoft.com/en-us/uwp/api/windows.devices.geolocation.geofencing.geofencemonitor
    async fn poll_location(&self) -> anyhow::Result<Location> {
        let loc = if let Some(loc) = &self.loc {
            loc
        } else {
            return Err(anyhow::anyhow!("cannot poll if not started"));
        };

        let pos = loc.GetGeopositionAsync()?.await?;
        let location = Location::from(
            &pos.Coordinate()?
                .Point()?
                .Position()?,
        );
        log::debug!("location: {location:?}");
        Ok(location)
    }
}
