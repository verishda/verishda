

use std::{default, sync::Arc};

use tokio::sync::Mutex;
#[cfg(target_os = "windows")]
use windows::Devices::Geolocation::Geolocator;
use windows::{Devices::Geolocation::{BasicGeoposition}};
use anyhow::Result;

#[derive(Clone, Debug, Default)]
pub struct Location{
    latitude: f64,
    longitude: f64
}

impl Location {
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude
        }
    }
}

#[derive(Debug)]
struct GeoCircle {
    center: Location,
    radius: f64
}

impl GeoCircle {
    fn is_inside(&self, location: &Location) -> bool {
        // https://en.wikipedia.org/wiki/Geographical_distance#Spherical_Earth_projected_to_a_plane
        //
        // D = R * sqrt(Δφ^2 + (cos(φm)*Δλ)^2)
        //
        // φm = (φ1 + φ2) / 2
        // Δφ = φ2 - φ1
        // Δλ = λ2 - λ1
        //
        // where:
        // φ1, φ2 are the latitudes of the two points and
        // λ1, λ2 are the longitudes of the two points, all in radians.
        // D is the distance between the two points (along the surface of the sphere),
        // R is the radius of the earth,
        // r is the radius of the circle,
        // φm is the average latitude of the two points,
        //
        // Because we only want to know if r < D, we transform the formula to:
        //
        // r < R * sqrt(Δφ^2 + (cos(φm)*Δλ)^2)
        // 
        // squaring both sides leaves us with:
        //
        // r^2 < R^2 * (Δφ^2 + (cos(φm)*Δλ)^2)
        //

        let φ1 = location.latitude.to_radians();
        let λ1 = location.longitude.to_radians();
        let φ2 = self.center.latitude.to_radians();
        let λ2 = self.center.longitude.to_radians();

        let φm = (φ1 + φ2) / 2.;
        let Δφ = φ2 - φ1;
        let Δλ = λ2 - λ1;

        let R =  6378100.0f64; // radius of the earth in km
        let r = self.radius;

        r.powi(2) > R.powi(2) * (Δφ.powi(2) + (φm.cos()*Δλ).powi(2))
    }
}

#[derive(Debug,Default)]
pub(super) struct LocationHandler{
    shapes: std::collections::HashMap<String, GeoCircle>,
    poll_interval_seconds: u32,
    in_fences: std::collections::HashSet<String>,
}

#[cfg(target_os = "windows")]
impl From<&BasicGeoposition> for Location {
    fn from(pos: &BasicGeoposition) -> Self {
        Location {
            latitude: pos.Latitude,
            longitude: pos.Longitude
        }
    }
}

// https://learn.microsoft.com/en-us/previous-versions/windows/apps/dn263199(v=win.10)
// https://docs.microsoft.com/en-us/uwp/api/windows.devices.geolocation.geofencing.geofencemonitor
#[cfg(target_os = "windows")]
impl LocationHandler {
    pub fn new() -> Arc<Mutex<LocationHandler>> {
        let handler = Arc::new(Mutex::new(Self {
            poll_interval_seconds: 5,
            ..Self::default()
        }));

        let handler_clone = handler.clone();
        tokio::spawn(async move{
            loop {
                LocationHandler::poll(handler_clone.clone()).await;
            }
        });

        handler
    }

    pub async fn poll(handler: Arc<Mutex<Self>>) {

        log::debug!("get next location and update geofence presence");
        let loc = Geolocator::new().unwrap();
        let pos = loc.GetGeopositionAsync().unwrap().await.unwrap();
        let location = Location::from(&pos.Coordinate().unwrap().Point().unwrap().Position().unwrap());
        log::debug!("location: {location:?}");

        let mut handler = handler.lock().await;
        handler.check_geofences(&location);
        let sleep_duration = std::time::Duration::from_secs(handler.poll_interval_seconds as u64);
        drop(handler);  // dropping handler guard to release lock, avoiding deadlock

        // sleep until next iteration
        tokio::time::sleep(sleep_duration).await;
    }

    fn check_geofences(&mut self, location: &Location) {
        log::debug!("polling geofences");
        log::trace!("installed geofences: {:?}", self.shapes);
        for (id, shape) in &self.shapes {
            if shape.is_inside(&location) {
                if !self.in_fences.contains(id) {
                    log::info!("Entered geofence: {id}");
                    self.in_fences.insert(id.to_string());
                }
            } else {
                if self.in_fences.contains(id) {
                    log::info!("Exited geofence: {id}");
                    self.in_fences.remove(id);
                }
            }
        }
        log::debug!("in_fences: {:?}", self.in_fences);
    }

    pub fn add_geofence_circle(&mut self, id: &str, location: &Location, radius: f64) -> Result<()> {
        self.shapes.insert(id.to_string(), GeoCircle {
            center: location.clone(),
            radius
        });
        Ok(())
    }

    pub fn remove_geofence(&mut self, id: &str) -> Result<()> {
        self.shapes.remove(id);
        Ok(())
    }

    pub fn clear_geofences(&mut self) {
        self.shapes.clear();
    }

    pub fn get_occupied_geofences(&self) -> Vec<String> {
        self.in_fences.iter().cloned().collect()
    }
   
}


#[test]
fn test_geo_circle() {
    let circle = GeoCircle {
        center: Location::new(0.0, 0.0),
        radius: 100.0
    };

    let inside = Location::new(0.0, 0.0);
    let outside = Location::new(0.0, 100.0);

    assert!(circle.is_inside(&inside));
    assert!(!circle.is_inside(&outside));
}