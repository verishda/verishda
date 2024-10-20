use std::{collections::{HashMap, HashSet}, sync::Arc, time::Duration};

use anyhow::Result;
use tokio::sync::Mutex;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os="windows", target_os="macos")))]
mod dummy;

#[derive(Clone, Debug, Default)]
pub struct Location {
    latitude: f64,
    longitude: f64,
}

impl Location {
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
        }
    }

    #[allow(non_snake_case)]
    pub fn squared_distance(&self, location: &Location) -> f64 {
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
        // φm is the average latitude of the two points
        //

        let φ1 = location.latitude.to_radians();
        let λ1 = location.longitude.to_radians();
        let φ2 = self.latitude.to_radians();
        let λ2 = self.longitude.to_radians();

        let φm = (φ1 + φ2) / 2.;
        let Δφ = φ2 - φ1;
        let Δλ = λ2 - λ1;

        let R = 6378100.0f64; // radius of the earth in km

        // the squared distance is D2
        let D2 = R.powi(2) * (Δφ.powi(2) + (φm.cos() * Δλ).powi(2));
        D2
    }
}

#[derive(Debug)]
struct GeoCircle {
    center: Location,
    radius: f64,
}

impl GeoCircle {
    #[allow(non_snake_case)]
    fn is_inside(&self, location: &Location) -> bool {
        // To check if we are inside the circle
        // with radius r of the given location, we first
        // calculate the distance D to the other location.
        //
        // The check if we are inde the circle is
        // r < D
        //
        // Replacing D with it's actual formula yields:
        //
        // r < R * sqrt(Δφ^2 + (cos(φm)*Δλ)^2)
        //
        // Because we only want to know if r < D, and calculating
        // square roots is expensive, we can also compare the
        // squares:
        // r^2 < D^2
        //

        let D2 = self.center.squared_distance(location);
        let r = self.radius;

        r.powi(2) > D2
    }
}

pub(crate) trait PollingLocator {
    fn new() -> Self;

    async fn poll_location(&self) -> Location;
}

#[cfg(target_os="windows")]
type PollingLocatorImpl = windows::WindowsPollingLocator;
#[cfg(target_os="macos")]
type PollingLocatorImpl = macos::MacOsPollingLocator;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
type PollingLocatorImpl = dummy::DummyPollingLocator;

#[derive(Debug)]
pub(super) struct LocationHandler {
    polling_locator: PollingLocatorImpl,
    shapes: std::collections::HashMap<String, GeoCircle>,
    in_fences: std::collections::HashSet<String>,
}

impl LocationHandler {
    
    pub fn new() -> Arc<Mutex<LocationHandler>> {
        let handler = Arc::new(Mutex::new(Self {
            
            polling_locator: PollingLocatorImpl::new(),
            shapes: HashMap::new(),
            in_fences: HashSet::new(),
        }));

        let handler_clone = handler.clone();
        tokio::spawn(async move {
            let mut poll_interval = tokio::time::interval(Duration::from_secs(5));
            poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                // request locations from location handler
                LocationHandler::poll(handler_clone.clone()).await;

                // sleep until next iteration
                poll_interval.tick().await;
            }
        });

        handler
    }


    pub async fn poll(handler: Arc<Mutex<Self>>) {
        let mut handler = handler.lock().await;
        let location = handler.polling_locator.poll_location().await;

        handler.check_geofences(&location);
    }

    fn check_geofences(&mut self, location: &Location) {
        log::debug!("polling geofences against {location:?}");
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

    pub fn add_geofence_circle(
        &mut self,
        id: &str,
        location: &Location,
        radius: f64,
    ) -> Result<()> {
        self.shapes.insert(
            id.to_string(),
            GeoCircle {
                center: location.clone(),
                radius,
            },
        );
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
        radius: 100.0,
    };

    let inside = Location::new(0.0, 0.0);
    let outside = Location::new(0.0, 100.0);

    assert!(circle.is_inside(&inside));
    assert!(!circle.is_inside(&outside));
}

#[test]
fn test_distance() {
    let loc1 = Location {
        latitude: 48.48870120526846,
        longitude: 9.218084635543407,
    };
    let loc2 = Location {
        latitude: 48.4901237487793,
        longitude: 9.21942138671875,
    };
    let D2 = loc1.squared_distance(&loc2);
    let D = D2.sqrt();
    println!("distance betwen {loc1:?} and {loc2:?} is {D}");
    assert!(D < 200.);
}
