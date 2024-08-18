use std::{sync::Arc, thread, time::Duration};
use objc2_foundation::{NSObject, NSArray, NSObjectProtocol, NSError};
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
use objc2::{declare_class, msg_send_id, mutability, rc::Retained, runtime::ProtocolObject, ClassType, DeclaredClass, Message};
use objc2_core_location::{CLLocation, CLLocationManager, CLLocationManagerDelegate};
use oslog::OsLogger;
use tokio::sync::RwLock;


use super::{Location, PollingLocator};

struct LocationDelegateInner {

}
declare_class!(
    struct LocationDelegate;

    unsafe impl ClassType for LocationDelegate {
        type Super = NSObject;
        type Mutability = mutability::Immutable; 
        const NAME: &'static str = "LocationDelegate";
    }

    impl DeclaredClass for LocationDelegate {
        type Ivars = LocationDelegateInner;
    }

    unsafe impl NSObjectProtocol for LocationDelegate {}
    
    unsafe impl CLLocationManagerDelegate for LocationDelegate {
        #[method(locationManager:didFailWithError:)]
        unsafe fn locationManager_didFailWithError(
            &self,
            _manager: &CLLocationManager,
            error: &NSError,
        ) {
            println!("received error from CLLocationManager {error}")
        }

        #[method(locationManagerDidChangeAuthorization:)]
        unsafe fn locationManagerDidChangeAuthorization(&self, manager: &CLLocationManager) {
            use objc2_core_location::*;

            let status = manager.authorizationStatus();
            log::debug!("locationManagerDidChangeAuthorization: current status is {status:?}");

            match status {
                CLAuthorizationStatus::kCLAuthorizationStatusNotDetermined => {
                    manager.requestAlwaysAuthorization();
                },
                CLAuthorizationStatus::kCLAuthorizationStatusAuthorized |
                CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedAlways |
                CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedWhenInUse => {
                    manager.startUpdatingLocation();
                },
                CLAuthorizationStatus::kCLAuthorizationStatusRestricted |
                CLAuthorizationStatus::kCLAuthorizationStatusDenied => {
                    manager.stopUpdatingLocation();
                }
                _ => {
                    log::error!("unknown status {status:?}");
                }
            }
        }   
             #[method(locationManager:didUpdateLocations:)]
        unsafe fn locationManager_didUpdateLocations(
            &self,
            _manager: &CLLocationManager,
            locations: &NSArray<CLLocation>,
        ) {
            println!("locations updated: {:?}", DebugNSArray{array:locations});     
        }

    }
);

impl LocationDelegate {
    pub fn new() -> Retained<Self> {
        unsafe { msg_send_id![Self::alloc(), init] }
    }
}


struct DebugNSArray<'a, T> 
{
    array: &'a NSArray<T>
}
impl <'a, T> std::fmt::Debug for DebugNSArray<'a, T> 
where T: Message + std::fmt::Debug
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut l = f.debug_list();
        for i in 0..self.array.len() {
            let obj = unsafe {
                self.array.objectAtIndex(i)
            };
            l.entry(&obj);
        }
        l.finish()
    }
}

impl From<Retained<CLLocation>> for Location {
    fn from(value: Retained<CLLocation>) -> Self {
        let coordinate;
        unsafe {
            coordinate = value.coordinate();
        }
        Location {
            latitude: coordinate.latitude,
            longitude: coordinate.longitude,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct MacOsPollingLocator {
    current_location: Arc<RwLock<super::Location>>,
}

impl MacOsPollingLocator {
    pub(crate) fn new() -> Self {
        let loc = Self::default();
        let target = loc.current_location.clone();
        thread::spawn(||run_location_manager_loop(target));

        loc
    }

    pub(crate) async fn poll_location(&self) -> super::Location {
        self.current_location.read().await.clone()
    }
}

fn run_location_manager_loop(current_location: Arc<RwLock<Location>>) {
    let location_manager;

    let delegate = LocationDelegate::new();
    unsafe {
        location_manager = CLLocationManager::new();
        log::info!("auth status: {:?}", location_manager.authorizationStatus());
        location_manager.setDistanceFilter(100.);
        location_manager.setDesiredAccuracy(50.);
        location_manager.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        location_manager.requestAlwaysAuthorization();
        
        loop {
            CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_secs(10), false);
            let loc = location_manager.location();
            log::info!("location: {loc:?}");
            if let Some(loc) = loc {
                *(current_location.blocking_write()) = Location::from(loc);
            }            
        }
    }
}

