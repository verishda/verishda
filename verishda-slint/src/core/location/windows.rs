use windows::Devices::Geolocation::{BasicGeoposition, Geolocator};


pub(crate) struct WindowsPollingLocator {
    loc: Geolocator,
}

// https://learn.microsoft.com/en-us/previous-versions/windows/apps/dn263199(v=win.10)
// https://docs.microsoft.com/en-us/uwp/api/windows.devices.geolocation.geofencing.geofencemonitor
impl WindowsPollingLocator {
    fn new() -> Self {
        Self {
            loc: Geolocator::new().unwrap()
        }
    }
}


impl From<&BasicGeoposition> for Location {
    fn from(pos: &BasicGeoposition) -> Self {
        Location {
            latitude: pos.Latitude,
            longitude: pos.Longitude,
        }
    }
}

impl PollingLocator for WindowsPollingLocator {
    async fn poll_location(&self) -> Location {
        let pos = self.loc.GetGeopositionAsync().unwrap().await.unwrap();
        let location = Location::from(
            &pos.Coordinate()
                .unwrap()
                .Point()
                .unwrap()
                .Position()
                .unwrap(),
        );
        log::debug!("location: {location:?}");
        location
    }
}
