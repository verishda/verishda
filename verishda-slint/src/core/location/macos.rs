use std::{sync::Arc, thread, time::{Duration, Instant}};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSRunLoop};
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRun, CFRunLoopRunResult};
use objc2::{declare_class, msg_send_id, mutability, rc::Retained, runtime::ProtocolObject, ClassType, DeclaredClass, Message};
use objc2_core_location::{CLLocation, CLLocationManager, CLLocationManagerDelegate};
use oslog::OsLogger;
use tokio::sync::RwLock;


use super::{Location, PollingLocator};

struct LocationDelegateIvars {
    current_location: Arc<RwLock<super::Location>>, 
}
declare_class!(
    struct LocationDelegate;

    unsafe impl ClassType for LocationDelegate {
        type Super = NSObject;
        type Mutability = mutability::Immutable; 
        const NAME: &'static str = "LocationDelegate";
    }

    impl DeclaredClass for LocationDelegate {
        type Ivars = LocationDelegateIvars;
    }

    unsafe impl NSObjectProtocol for LocationDelegate {}
    
    unsafe impl CLLocationManagerDelegate for LocationDelegate {
        #[method(locationManager:didFailWithError:)]
        unsafe fn locationManager_didFailWithError(
            &self,
            _manager: &CLLocationManager,
            error: &NSError,
        ) {
            log::error!("received error from CLLocationManager {error}")
        }

        #[method(locationManagerDidChangeAuthorization:)]
        unsafe fn locationManagerDidChangeAuthorization(&self, manager: &CLLocationManager) {
            log::info!("locationManagerDidChangeAuthorization: ");
            handle_authorization_status(manager);
       }   

        #[method(locationManager:didUpdateLocations:)]
        unsafe fn locationManager_didUpdateLocations(
            &self,
            manager: &CLLocationManager,
            locations: &NSArray<CLLocation>,
        ) {
            log::info!("locations updated: {:?}", DebugNSArray{array:locations});     
            if let Some(loc) = locations.last() {
                *(self.ivars().current_location.blocking_write()) = Location::from(loc);
            }
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

impl From<&CLLocation> for Location {
    fn from(value: &CLLocation) -> Self {
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

unsafe fn handle_authorization_status(manager: &CLLocationManager) {
    use objc2_core_location::*;

    let status = manager.authorizationStatus();
    log::info!("current authorization status is {status:?}");

    match status {
        CLAuthorizationStatus::kCLAuthorizationStatusNotDetermined => {
            log::info!("requesting authorization");
            manager.requestAlwaysAuthorization();
        },
        CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedAlways |
        CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedWhenInUse => {
            log::info!("authorization for reading location data received");
            manager.startUpdatingLocation();
        },
        CLAuthorizationStatus::kCLAuthorizationStatusRestricted |
        CLAuthorizationStatus::kCLAuthorizationStatusDenied => {
            log::info!("authorization denied for reading location data");
            manager.stopUpdatingLocation();
        }
        _ => {
            log::error!("unknown status {status:?}");
        }
    }

}

fn run_location_manager_loop(current_location: Arc<RwLock<Location>>) {
    let location_manager;

    let delegate = LocationDelegate::new();
    unsafe {
        location_manager = CLLocationManager::new();
        location_manager.setDistanceFilter(100.);
        location_manager.setDesiredAccuracy(50.);
        location_manager.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        
        loop {
            const ONE_SECOND: Duration = Duration::from_secs(1);
            let res = CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, ONE_SECOND, false);
            log::info!("run loop finished with {res:?}");
            if res == CFRunLoopRunResult::Finished {
                std::thread::sleep(ONE_SECOND);
                location_manager.requestLocation();
            }
        }

        log::info!("location manager thread terminated regularly.")
    }
}

