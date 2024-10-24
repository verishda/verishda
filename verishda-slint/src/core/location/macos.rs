use std::{sync::Arc, thread::{self}, time::Duration};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRunResult};
use objc2::{declare_class, msg_send_id, mutability, rc::Retained, runtime::ProtocolObject, ClassType, DeclaredClass, Message};
use objc2_core_location::{CLLocation, CLLocationManager, CLLocationManagerDelegate};
use tokio::sync::RwLock;


use super::Location;

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
                let loc = Location::from(loc);
                log::debug!("wrote location to rwlock: {loc:?}");
                *(self.ivars().current_location.blocking_write()) = loc;
            }
        }

    }
);

impl LocationDelegate {
    pub fn new(current_location: Arc<RwLock<Location>>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(LocationDelegateIvars{ current_location });
        unsafe { msg_send_id![super(this), init] }
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

#[derive(Debug)]
enum ServiceCommand {
    Start,
    Stop,
    Terminate,
}

#[derive(Debug)]
pub(crate) struct MacOsPollingLocator {
    current_location: Arc<RwLock<super::Location>>,
    cmd_tx: std::sync::mpsc::Sender<ServiceCommand>,
}

impl MacOsPollingLocator {
    fn send_cmd(&self, cmd: ServiceCommand) {
        self.cmd_tx.send(cmd).unwrap_or_else(|e|{
            log::error!("failed to send service command");
        })
    }
}

impl super::PollingLocator for MacOsPollingLocator {
    fn new() -> Self {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();

        let loc = Self {
            current_location: Arc::new(RwLock::new(Location::default())),
            cmd_tx
        };
        let target = loc.current_location.clone();
        thread::spawn(||run_location_manager_loop(target, cmd_rx));

        loc
    }

    fn start(&mut self) {
        self.send_cmd(ServiceCommand::Start);
    }

    fn stop(&mut self) {
        self.send_cmd(ServiceCommand::Stop);
    }

    async fn poll_location(&self) -> anyhow::Result<super::Location> {
        let loc = self.current_location.read().await.clone();
        log::debug!("read location {loc:?}");
        Ok(loc)
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
            manager.requestLocation();
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

fn run_location_manager_loop(current_location: Arc<RwLock<Location>>, cmd_rx: std::sync::mpsc::Receiver<ServiceCommand>) {
    let mut location_manager: Option<Retained<CLLocationManager>> = None;

    log::info!("STARTING run_location_manager_loop()");

    let delegate = LocationDelegate::new(current_location);
    unsafe {
        let mut suspended = true;

        loop {

            // handle reading commands and service suspension
            let cmd;
            if suspended {
                log::info!("MacOsPollingLocator suspended and waiting to be reactivated.");
                cmd = cmd_rx.recv().ok();
            } else {
                cmd = cmd_rx.try_recv().ok();
            }

            if let Some(cmd) = cmd {
                log::info!("MacOsPollingLocator received command {cmd:?}");
                match cmd {
                    ServiceCommand::Start => {
                        suspended = false;
                    }
                    ServiceCommand::Stop => {
                        suspended = true;
                        continue;
                    }
                    ServiceCommand::Terminate => {
                        break;
                    }
                }
            }
            
            // make sure we have a location manager, and if not, create it.
            // we do this on-demand here because setting the deletage
            // will trigger the user request if they allow to get the location.
            let lm = match location_manager.clone() {
                Some(lm) => lm.clone(),
                None => {
                    let lm = CLLocationManager::new();
                    lm.setDistanceFilter(100.);
                    lm.setDesiredAccuracy(50.);
                    lm.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                    location_manager = Some(lm.clone());
                    lm
                }
            };
                
            // run actual polling round
            const ONE_SECOND: Duration = Duration::from_secs(1);
            let res = CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, ONE_SECOND, false);
            log::info!("run loop finished with {res:?}");
            if res == CFRunLoopRunResult::Finished {
                std::thread::sleep(ONE_SECOND);
                lm.requestLocation();
            }
        }

        log::info!("location manager thread terminated regularly.")
    }
}

