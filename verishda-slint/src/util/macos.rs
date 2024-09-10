use objc2::{declare_class, msg_send, msg_send_id, mutability::MainThreadOnly, rc::Retained, runtime::{NSObject, NSObjectProtocol, ProtocolObject}, ClassType, DeclaredClass};
use objc2_app_kit::{NSApplicationDelegate, NSApplication};
use objc2_foundation::{MainThreadMarker, NSArray, NSURL};


declare_class!(
    struct ApplicationDelegate;

    unsafe impl ClassType for ApplicationDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "ApplicationDelegate";
    }

    impl DeclaredClass for ApplicationDelegate {
    }

    unsafe impl NSObjectProtocol for ApplicationDelegate {}

    unsafe impl NSApplicationDelegate for ApplicationDelegate {
        #[method(application:openURLs:)]
        unsafe fn application_openURLs(&self, application: &NSApplication, urls: &NSArray<NSURL>) {
            handle_open_urls(urls)
        }
    }
);

impl ApplicationDelegate {
    pub fn new() -> Retained<Self> {
        let mtm = MainThreadMarker::new().expect("not on main thread");
        unsafe { msg_send_id![mtm.alloc(), init] }
    }
}

unsafe fn handle_open_urls(urls: &NSArray<NSURL>) {
    let urls = urls.iter()
    .filter_map(|url|url.absoluteString())
    .map(|url|url.to_string())
    .collect::<Vec<String>>();
    log::info!("handle_open_urls: {urls:?}")
}

pub(super) fn init() {
    let delegate = ApplicationDelegate::new();
    
    let mtm = MainThreadMarker::new().unwrap();
    let nsapp = NSApplication::sharedApplication(mtm);
    nsapp.setDelegate(Some(&ProtocolObject::from_retained(delegate)));

}