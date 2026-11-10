use std::cell::RefCell;
use std::rc::Rc;

use crate::stub::{self, Script};
use crate::{EventLoop, EventQueue, GuiError, Widgets};

/// 使うbackend。stubとネイティブのbackendを1つのバイナリに同居させるため、実行のときに選ぶ。
pub enum Backend {
    Native,
    Stub(Script),
}

/// 作ったbackend。`widgets`は`Toolkit`が、`event_loop`はメインのworkerの`Park`が持ち、両者は`queue`を共有する。
pub struct Native {
    pub widgets: Rc<RefCell<dyn Widgets>>,
    pub event_loop: Box<dyn EventLoop>,
    pub queue: EventQueue,
}

impl Backend {
    pub fn create(self) -> Result<Native, GuiError> {
        match self {
            Backend::Stub(script) => Ok(stub::create(script)),
            #[cfg(all(feature = "native", target_os = "macos"))]
            Backend::Native => crate::appkit::create(),
            #[cfg(all(feature = "native", target_os = "linux"))]
            Backend::Native => crate::qt::create(),
            #[cfg(all(feature = "native", windows))]
            Backend::Native => crate::winui::create(),
            #[cfg(not(all(
                feature = "native",
                any(target_os = "macos", target_os = "linux", windows)
            )))]
            Backend::Native => Err(GuiError::NotAvailable(
                "cannot open the GUI: built without the native toolkit".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(all(
        feature = "native",
        any(target_os = "macos", target_os = "linux", windows)
    )))]
    #[test]
    fn native_is_not_available_without_the_native_toolkit() {
        assert_eq!(
            Backend::Native.create().err(),
            Some(GuiError::NotAvailable(
                "cannot open the GUI: built without the native toolkit".to_string()
            ))
        );
    }

    #[test]
    fn stub_creates() {
        assert!(Backend::Stub(Script::default()).create().is_ok());
    }
}
