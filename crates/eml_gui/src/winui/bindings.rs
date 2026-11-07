// nix/winui-bindings.shが生成した。手では直さない。
// 生成: windows-bindgen 0.100.0
// メタデータ: Windows App SDK 2.6.4のMicrosoft.UI.winmdとMicrosoft.UI.Xaml.winmd (NuGetのパッケージ
// Microsoft.WindowsAppSDK.InteractiveExperiencesとMicrosoft.WindowsAppSDK.WinUIのもの)。microsoft/windows-rs
// (MIT OR Apache-2.0) がコミット2b5f63797a464bbe571b444b1f19dbd26d98c228のcrates/tools/reactor-metadata/winmdに置いたものを使う。
// windows-reactorも同じメタデータからbindingを生成する。
// 作り直し: nix develop -c nix/winui-bindings.sh (--checkは比べるだけ)
windows_core::link!("kernel32.dll" "system" fn GetModuleHandleW(lpmodulename : windows_core::PCWSTR) -> HMODULE);
windows_core::link!("kernel32.dll" "system" fn GetProcAddress(hmodule : HMODULE, lpprocname : windows_core::PCSTR) -> FARPROC);
windows_core::link!("kernel32.dll" "system" fn GetProcessHeap() -> HANDLE);
windows_core::link!("kernel32.dll" "system" fn HeapFree(hheap : HANDLE, dwflags : u32, lpmem : *mut core::ffi::c_void) -> windows_core::BOOL);
windows_core::link!("api-ms-win-core-winrt-l1-1-0.dll" "system" fn RoInitialize(inittype : RO_INIT_TYPE) -> windows_core::HRESULT);
pub type AddPackageDependencyOptions = u32;
pub const AddPackageDependencyOptions_None: AddPackageDependencyOptions = 0;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppWindow(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AppWindow,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for AppWindow {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAppWindow>();
}
unsafe impl windows_core::Interface for AppWindow {
    type Vtable = <IAppWindow as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAppWindow as windows_core::Interface>::IID;
}
impl core::ops::Deref for AppWindow {
    type Target = IAppWindow;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AppWindow {
    const NAME: &'static str = "Microsoft.UI.Windowing.AppWindow";
}
unsafe impl Send for AppWindow {}
unsafe impl Sync for AppWindow {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppWindowClosingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AppWindowClosingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for AppWindowClosingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAppWindowClosingEventArgs>();
}
unsafe impl windows_core::Interface for AppWindowClosingEventArgs {
    type Vtable = <IAppWindowClosingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAppWindowClosingEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for AppWindowClosingEventArgs {
    type Target = IAppWindowClosingEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AppWindowClosingEventArgs {
    const NAME: &'static str = "Microsoft.UI.Windowing.AppWindowClosingEventArgs";
}
unsafe impl Send for AppWindowClosingEventArgs {}
unsafe impl Sync for AppWindowClosingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppWindowPresenter(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AppWindowPresenter,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for AppWindowPresenter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAppWindowPresenter>();
}
unsafe impl windows_core::Interface for AppWindowPresenter {
    type Vtable = <IAppWindowPresenter as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAppWindowPresenter as windows_core::Interface>::IID;
}
impl core::ops::Deref for AppWindowPresenter {
    type Target = IAppWindowPresenter;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AppWindowPresenter {
    const NAME: &'static str = "Microsoft.UI.Windowing.AppWindowPresenter";
}
unsafe impl Send for AppWindowPresenter {}
unsafe impl Sync for AppWindowPresenter {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Application(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Application,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl Application {
    pub fn compose<T>(compose: T) -> windows_core::Result<Self>
    where
        T: windows_core::Compose,
    {
        Self::IApplicationFactory(|this| unsafe {
            let (derived__, base__) = windows_core::Compose::compose(compose);
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&derived__),
                base__ as *mut _ as _,
                &mut result__,
            )
            .ok()?;
            let _ = &derived__;
            windows_core::imp::Type::from_abi(result__)
        })
    }
    pub fn Current() -> windows_core::Result<Self> {
        Self::IApplicationStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Current)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn Start<P0>(callback: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ApplicationInitializationCallback>,
    {
        Self::IApplicationStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).Start)(
                windows_core::Interface::as_raw(this),
                callback.param().abi(),
            )
            .ok()
        })
    }
    fn IApplicationFactory<R, F: FnOnce(&IApplicationFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Application, IApplicationFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IApplicationStatics<R, F: FnOnce(&IApplicationStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Application, IApplicationStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Application {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IApplication>();
}
unsafe impl windows_core::Interface for Application {
    type Vtable = <IApplication as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IApplication as windows_core::Interface>::IID;
}
impl core::ops::Deref for Application {
    type Target = IApplication;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Application {
    const NAME: &'static str = "Microsoft.UI.Xaml.Application";
}
unsafe impl Send for Application {}
unsafe impl Sync for Application {}
windows_core::imp::define_interface!(
    ApplicationInitializationCallback,
    ApplicationInitializationCallback_Vtbl,
    0xd8eef1c9_1234_56f1_9963_45dd9c80a661
);
impl windows_core::RuntimeType for ApplicationInitializationCallback {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ApplicationInitializationCallback {
    pub fn new<F: Fn(windows_core::Ref<ApplicationInitializationCallbackParams>) + 'static>(
        invoke: F,
    ) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &ApplicationInitializationCallbackBox::<F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
}
#[repr(C)]
pub struct ApplicationInitializationCallback_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        p: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct ApplicationInitializationCallbackBox<
    F: Fn(windows_core::Ref<ApplicationInitializationCallbackParams>) + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<F: Fn(windows_core::Ref<ApplicationInitializationCallbackParams>) + 'static>
    ApplicationInitializationCallbackBox<F>
{
    const VTABLE: ApplicationInitializationCallback_Vtbl = ApplicationInitializationCallback_Vtbl {
        base__:
            windows_core::IUnknown_Vtbl {
                QueryInterface: windows_core::imp::DelegateBox::<
                    ApplicationInitializationCallback,
                    F,
                >::QueryInterface,
                AddRef:
                    windows_core::imp::DelegateBox::<ApplicationInitializationCallback, F>::AddRef,
                Release:
                    windows_core::imp::DelegateBox::<ApplicationInitializationCallback, F>::Release,
            },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        p: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<ApplicationInitializationCallback, F>);
            (this.invoke)(core::mem::transmute_copy(&p));
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationInitializationCallbackParams(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ApplicationInitializationCallbackParams,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for ApplicationInitializationCallbackParams {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        IApplicationInitializationCallbackParams,
    >();
}
unsafe impl windows_core::Interface for ApplicationInitializationCallbackParams {
    type Vtable = <IApplicationInitializationCallbackParams as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IApplicationInitializationCallbackParams as windows_core::Interface>::IID;
}
impl core::ops::Deref for ApplicationInitializationCallbackParams {
    type Target = IApplicationInitializationCallbackParams;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ApplicationInitializationCallbackParams {
    const NAME: &'static str = "Microsoft.UI.Xaml.ApplicationInitializationCallbackParams";
}
unsafe impl Send for ApplicationInitializationCallbackParams {}
unsafe impl Sync for ApplicationInitializationCallbackParams {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Button(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Button, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(
    Button,
    ButtonBase,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl Button {
    pub fn new() -> windows_core::Result<Self> {
        Self::IButtonFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IButtonFactory<R, F: FnOnce(&IButtonFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Button, IButtonFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Button {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IButton>();
}
unsafe impl windows_core::Interface for Button {
    type Vtable = <IButton as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IButton as windows_core::Interface>::IID;
}
impl core::ops::Deref for Button {
    type Target = IButton;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Button {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Button";
}
unsafe impl Send for Button {}
unsafe impl Sync for Button {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ButtonBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ButtonBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ButtonBase,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ButtonBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IButtonBase>();
}
unsafe impl windows_core::Interface for ButtonBase {
    type Vtable = <IButtonBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IButtonBase as windows_core::Interface>::IID;
}
impl core::ops::Deref for ButtonBase {
    type Target = IButtonBase;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ButtonBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.ButtonBase";
}
unsafe impl Send for ButtonBase {}
unsafe impl Sync for ButtonBase {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Canvas(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Canvas, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(
    Canvas,
    Panel,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl Canvas {
    pub fn new() -> windows_core::Result<Self> {
        Self::ICanvasFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn SetLeft<P0>(element: P0, length: f64) -> windows_core::Result<()>
    where
        P0: windows_core::Param<UIElement>,
    {
        Self::ICanvasStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).SetLeft)(
                windows_core::Interface::as_raw(this),
                element.param().abi(),
                length,
            )
            .ok()
        })
    }
    pub fn SetTop<P0>(element: P0, length: f64) -> windows_core::Result<()>
    where
        P0: windows_core::Param<UIElement>,
    {
        Self::ICanvasStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).SetTop)(
                windows_core::Interface::as_raw(this),
                element.param().abi(),
                length,
            )
            .ok()
        })
    }
    fn ICanvasFactory<R, F: FnOnce(&ICanvasFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Canvas, ICanvasFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ICanvasStatics<R, F: FnOnce(&ICanvasStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Canvas, ICanvasStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Canvas {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICanvas>();
}
unsafe impl windows_core::Interface for Canvas {
    type Vtable = <ICanvas as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICanvas as windows_core::Interface>::IID;
}
impl core::ops::Deref for Canvas {
    type Target = ICanvas;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Canvas {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Canvas";
}
unsafe impl Send for Canvas {}
unsafe impl Sync for Canvas {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckBox(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CheckBox,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    CheckBox,
    ToggleButton,
    ButtonBase,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl CheckBox {
    pub fn new() -> windows_core::Result<Self> {
        Self::ICheckBoxFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn ICheckBoxFactory<R, F: FnOnce(&ICheckBoxFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<CheckBox, ICheckBoxFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for CheckBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICheckBox>();
}
unsafe impl windows_core::Interface for CheckBox {
    type Vtable = <ICheckBox as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICheckBox as windows_core::Interface>::IID;
}
impl core::ops::Deref for CheckBox {
    type Target = ICheckBox;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CheckBox {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.CheckBox";
}
unsafe impl Send for CheckBox {}
unsafe impl Sync for CheckBox {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComboBox(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ComboBox,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ComboBox,
    Selector,
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ComboBox {
    pub fn new() -> windows_core::Result<Self> {
        Self::IComboBoxFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IComboBoxFactory<R, F: FnOnce(&IComboBoxFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ComboBox, IComboBoxFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ComboBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IComboBox>();
}
unsafe impl windows_core::Interface for ComboBox {
    type Vtable = <IComboBox as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IComboBox as windows_core::Interface>::IID;
}
impl core::ops::Deref for ComboBox {
    type Target = IComboBox;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ComboBox {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ComboBox";
}
unsafe impl Send for ComboBox {}
unsafe impl Sync for ComboBox {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComboBoxItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ComboBoxItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ComboBoxItem,
    SelectorItem,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ComboBoxItem {
    pub fn new() -> windows_core::Result<Self> {
        Self::IComboBoxItemFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IComboBoxItemFactory<R, F: FnOnce(&IComboBoxItemFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ComboBoxItem, IComboBoxItemFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ComboBoxItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IComboBoxItem>();
}
unsafe impl windows_core::Interface for ComboBoxItem {
    type Vtable = <IComboBoxItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IComboBoxItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for ComboBoxItem {
    type Target = IComboBoxItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ComboBoxItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ComboBoxItem";
}
unsafe impl Send for ComboBoxItem {}
unsafe impl Sync for ComboBoxItem {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentControl(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ContentControl,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ContentControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IContentControl>();
}
unsafe impl windows_core::Interface for ContentControl {
    type Vtable = <IContentControl as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IContentControl as windows_core::Interface>::IID;
}
impl core::ops::Deref for ContentControl {
    type Target = IContentControl;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ContentControl {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ContentControl";
}
unsafe impl Send for ContentControl {}
unsafe impl Sync for ContentControl {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Control(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Control,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(Control, FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for Control {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IControl>();
}
unsafe impl windows_core::Interface for Control {
    type Vtable = <IControl as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IControl as windows_core::Interface>::IID;
}
impl core::ops::Deref for Control {
    type Target = IControl;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Control {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Control";
}
unsafe impl Send for Control {}
unsafe impl Sync for Control {}
pub type CreatePackageDependencyOptions = u32;
pub const CreatePackageDependencyOptions_None: CreatePackageDependencyOptions = 0;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyObject(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DependencyObject,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for DependencyObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDependencyObject>();
}
unsafe impl windows_core::Interface for DependencyObject {
    type Vtable = <IDependencyObject as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDependencyObject as windows_core::Interface>::IID;
}
impl core::ops::Deref for DependencyObject {
    type Target = IDependencyObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DependencyObject {
    const NAME: &'static str = "Microsoft.UI.Xaml.DependencyObject";
}
unsafe impl Send for DependencyObject {}
unsafe impl Sync for DependencyObject {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatcherQueue(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DispatcherQueue,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl DispatcherQueue {
    pub fn GetForCurrentThread() -> windows_core::Result<Self> {
        Self::IDispatcherQueueStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetForCurrentThread)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IDispatcherQueueStatics<
        R,
        F: FnOnce(&IDispatcherQueueStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<DispatcherQueue, IDispatcherQueueStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DispatcherQueue {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDispatcherQueue>();
}
unsafe impl windows_core::Interface for DispatcherQueue {
    type Vtable = <IDispatcherQueue as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDispatcherQueue as windows_core::Interface>::IID;
}
impl core::ops::Deref for DispatcherQueue {
    type Target = IDispatcherQueue;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DispatcherQueue {
    const NAME: &'static str = "Microsoft.UI.Dispatching.DispatcherQueue";
}
unsafe impl Send for DispatcherQueue {}
unsafe impl Sync for DispatcherQueue {}
windows_core::imp::define_interface!(
    DispatcherQueueHandler,
    DispatcherQueueHandler_Vtbl,
    0x2e0872a9_4e29_5f14_b688_fb96d5f9d5f8
);
impl windows_core::RuntimeType for DispatcherQueueHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl DispatcherQueueHandler {
    pub fn new<F: Fn() + 'static>(invoke: F) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &DispatcherQueueHandlerBox::<F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
}
#[repr(C)]
pub struct DispatcherQueueHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(this: *mut core::ffi::c_void) -> windows_core::HRESULT,
}
struct DispatcherQueueHandlerBox<F: Fn() + 'static>(core::marker::PhantomData<(fn() -> F,)>);
impl<F: Fn() + 'static> DispatcherQueueHandlerBox<F> {
    const VTABLE: DispatcherQueueHandler_Vtbl = DispatcherQueueHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<DispatcherQueueHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<DispatcherQueueHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<DispatcherQueueHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(this: *mut core::ffi::c_void) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<DispatcherQueueHandler, F>);
            (this.invoke)();
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatcherQueueTimer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DispatcherQueueTimer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for DispatcherQueueTimer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDispatcherQueueTimer>();
}
unsafe impl windows_core::Interface for DispatcherQueueTimer {
    type Vtable = <IDispatcherQueueTimer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDispatcherQueueTimer as windows_core::Interface>::IID;
}
impl core::ops::Deref for DispatcherQueueTimer {
    type Target = IDispatcherQueueTimer;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DispatcherQueueTimer {
    const NAME: &'static str = "Microsoft.UI.Dispatching.DispatcherQueueTimer";
}
unsafe impl Send for DispatcherQueueTimer {}
unsafe impl Sync for DispatcherQueueTimer {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DispatcherShutdownMode(pub i32);
impl DispatcherShutdownMode {
    pub const OnLastWindowClose: Self = Self(0);
    pub const OnExplicitShutdown: Self = Self(1);
}
impl windows_core::imp::TypeKind for DispatcherShutdownMode {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for DispatcherShutdownMode {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.DispatcherShutdownMode;i4)",
    );
}
pub type FARPROC = Option<unsafe extern "system" fn() -> isize>;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameworkElement(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FrameworkElement,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for FrameworkElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFrameworkElement>();
}
unsafe impl windows_core::Interface for FrameworkElement {
    type Vtable = <IFrameworkElement as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFrameworkElement as windows_core::Interface>::IID;
}
impl core::ops::Deref for FrameworkElement {
    type Target = IFrameworkElement;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FrameworkElement {
    const NAME: &'static str = "Microsoft.UI.Xaml.FrameworkElement";
}
unsafe impl Send for FrameworkElement {}
unsafe impl Sync for FrameworkElement {}
pub type HANDLE = *mut core::ffi::c_void;
pub type HINSTANCE = *mut core::ffi::c_void;
pub type HMODULE = HINSTANCE;
windows_core::imp::define_interface!(
    IAppWindow,
    IAppWindow_Vtbl,
    0xcfa788b3_643b_5c5e_ad4e_321d48a82acd
);
impl windows_core::RuntimeType for IAppWindow {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAppWindow {
    pub fn Presenter(&self) -> windows_core::Result<AppWindowPresenter> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Presenter)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Size(&self) -> windows_core::Result<SizeInt32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Size)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Closing<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<AppWindow>, windows_core::Ref<AppWindowClosingEventArgs>) + 'static,
    {
        let handler: TypedEventHandler<AppWindow, AppWindowClosingEventArgs> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<AppWindow, AppWindowClosingEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<AppWindow, AppWindowClosingEventArgs, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Closing)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveClosing,
            ))
        }
    }
}
#[repr(C)]
pub struct IAppWindow_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Id: usize,
    IsShownInSwitchers: usize,
    SetIsShownInSwitchers: usize,
    IsVisible: usize,
    OwnerWindowId: usize,
    Position: usize,
    pub Presenter: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Size:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut SizeInt32) -> windows_core::HRESULT,
    Title: usize,
    SetTitle: usize,
    TitleBar: usize,
    Destroy: usize,
    Hide: usize,
    Move: usize,
    MoveAndResize: usize,
    MoveAndResizeRelativeToDisplayArea: usize,
    Resize: usize,
    SetIcon: usize,
    SetIconWithIconId: usize,
    SetPresenter: usize,
    SetPresenterByKind: usize,
    Show: usize,
    ShowWithActivation: usize,
    Changed: usize,
    RemoveChanged: usize,
    pub Closing: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveClosing:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAppWindow2,
    IAppWindow2_Vtbl,
    0x6cd41292_794c_5cac_8961_210d012c6ebc
);
impl windows_core::RuntimeType for IAppWindow2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAppWindow2 {
    pub fn ClientSize(&self) -> windows_core::Result<SizeInt32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ClientSize)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ResizeClient(&self, size: SizeInt32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).ResizeClient)(
                windows_core::Interface::as_raw(self),
                size,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IAppWindow2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ClientSize:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut SizeInt32) -> windows_core::HRESULT,
    MoveInZOrderAtBottom: usize,
    MoveInZOrderAtTop: usize,
    MoveInZOrderBelow: usize,
    pub ResizeClient:
        unsafe extern "system" fn(*mut core::ffi::c_void, SizeInt32) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAppWindowClosingEventArgs,
    IAppWindowClosingEventArgs_Vtbl,
    0x0e09d90b_2261_590b_9ad1_8504991d8754
);
impl windows_core::RuntimeType for IAppWindowClosingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAppWindowClosingEventArgs {
    pub fn Cancel(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Cancel)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetCancel(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCancel)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IAppWindowClosingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Cancel:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetCancel: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAppWindowPresenter,
    IAppWindowPresenter_Vtbl,
    0xbc3042c2_c6c6_5632_8989_ff0ec6d3b40d
);
impl windows_core::RuntimeType for IAppWindowPresenter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IAppWindowPresenter_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IApplication,
    IApplication_Vtbl,
    0x06a8f4e7_1146_55af_820d_ebd55643b021
);
impl windows_core::RuntimeType for IApplication {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IApplication {
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Resources)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetResources)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Exit(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Exit)(windows_core::Interface::as_raw(self)).ok()
        }
    }
}
#[repr(C)]
pub struct IApplication_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Resources: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetResources: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    DebugSettings: usize,
    RequestedTheme: usize,
    SetRequestedTheme: usize,
    FocusVisualKind: usize,
    SetFocusVisualKind: usize,
    HighContrastAdjustment: usize,
    SetHighContrastAdjustment: usize,
    UnhandledException: usize,
    RemoveUnhandledException: usize,
    pub Exit: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IApplication3,
    IApplication3_Vtbl,
    0xbe941595_61fe_5b36_a3d3_962a647d7c6f
);
impl windows_core::RuntimeType for IApplication3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IApplication3 {
    pub fn DispatcherShutdownMode(&self) -> windows_core::Result<DispatcherShutdownMode> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).DispatcherShutdownMode)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetDispatcherShutdownMode(
        &self,
        value: DispatcherShutdownMode,
    ) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetDispatcherShutdownMode)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IApplication3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub DispatcherShutdownMode: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut DispatcherShutdownMode,
    ) -> windows_core::HRESULT,
    pub SetDispatcherShutdownMode: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        DispatcherShutdownMode,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IApplicationFactory,
    IApplicationFactory_Vtbl,
    0x9fd96657_5294_5a65_a1db_4fea143597da
);
impl windows_core::RuntimeType for IApplicationFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IApplicationFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IApplicationInitializationCallbackParams,
    IApplicationInitializationCallbackParams_Vtbl,
    0x1b1906ea_5b7b_5876_81ab_7c2281ac3d20
);
impl windows_core::RuntimeType for IApplicationInitializationCallbackParams {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IApplicationInitializationCallbackParams_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IApplicationOverrides,
    IApplicationOverrides_Vtbl,
    0xa33e81ef_c665_503b_8827_d27ef1720a06
);
impl windows_core::RuntimeType for IApplicationOverrides {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IApplicationOverrides");
}
impl IApplicationOverrides {
    pub fn OnLaunched<P0>(&self, args: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<LaunchActivatedEventArgs>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).OnLaunched)(
                windows_core::Interface::as_raw(self),
                args.param().abi(),
            )
            .ok()
        }
    }
}
impl windows_core::RuntimeName for IApplicationOverrides {
    const NAME: &'static str = "Microsoft.UI.Xaml.IApplicationOverrides";
}
pub trait IApplicationOverrides_Impl: windows_core::IUnknownImpl {
    fn OnLaunched(
        &self,
        args: windows_core::Ref<LaunchActivatedEventArgs>,
    ) -> windows_core::Result<()>;
}
impl IApplicationOverrides_Vtbl {
    pub const fn new<Identity: IApplicationOverrides_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn OnLaunched<
            Identity: IApplicationOverrides_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            args: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                IApplicationOverrides_Impl::OnLaunched(this, core::mem::transmute_copy(&args))
                    .into()
            }
        }
        Self {
            base__: windows_core::IInspectable_Vtbl::new::<Identity, IApplicationOverrides, OFFSET>(
            ),
            OnLaunched: OnLaunched::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<IApplicationOverrides as windows_core::Interface>::IID
    }
}
#[repr(C)]
pub struct IApplicationOverrides_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub OnLaunched: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IApplicationStatics,
    IApplicationStatics_Vtbl,
    0x4e0d09f5_4358_512c_a987_503b52848e95
);
impl windows_core::RuntimeType for IApplicationStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IApplicationStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Current: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Start: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IButton,
    IButton_Vtbl,
    0x216c183d_d07a_5aa5_b8a4_0300a2683e87
);
impl windows_core::RuntimeType for IButton {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IButton_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IButtonBase,
    IButtonBase_Vtbl,
    0x65714269_2473_5327_a652_0ea6bce7f403
);
impl windows_core::RuntimeType for IButtonBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IButtonBase {
    pub fn Click<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>)
            + 'static,
    {
        let handler: RoutedEventHandler = {
            let com = windows_core::imp::DelegateBox::<RoutedEventHandler, F>::new(
                &RoutedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Click)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveClick,
            ))
        }
    }
}
#[repr(C)]
pub struct IButtonBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ClickMode: usize,
    SetClickMode: usize,
    IsPointerOver: usize,
    IsPressed: usize,
    Command: usize,
    SetCommand: usize,
    CommandParameter: usize,
    SetCommandParameter: usize,
    pub Click: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveClick:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IButtonFactory,
    IButtonFactory_Vtbl,
    0xfe393422_d91c_57b1_9a9c_2c7e3f41f77c
);
impl windows_core::RuntimeType for IButtonFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IButtonFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICanvas,
    ICanvas_Vtbl,
    0x457ba139_1146_51d2_807e_d9d65c927060
);
impl windows_core::RuntimeType for ICanvas {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICanvas_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ICanvasFactory,
    ICanvasFactory_Vtbl,
    0x374c5050_3481_5557_9948_804c0b8eea89
);
impl windows_core::RuntimeType for ICanvasFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICanvasFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICanvasStatics,
    ICanvasStatics_Vtbl,
    0xc00d5e0f_77e3_5c59_8fcd_86761f0c6607
);
impl windows_core::RuntimeType for ICanvasStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICanvasStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    LeftProperty: usize,
    GetLeft: usize,
    pub SetLeft: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        f64,
    ) -> windows_core::HRESULT,
    TopProperty: usize,
    GetTop: usize,
    pub SetTop: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        f64,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICheckBox,
    ICheckBox_Vtbl,
    0xc5830000_4c9d_5fdd_9346_674c71cd80c5
);
impl windows_core::RuntimeType for ICheckBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICheckBox_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ICheckBoxFactory,
    ICheckBoxFactory_Vtbl,
    0xf43ff58d_31d5_5835_af7b_375bc6a9bcf3
);
impl windows_core::RuntimeType for ICheckBoxFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICheckBoxFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IComboBox,
    IComboBox_Vtbl,
    0xc77da58b_4fd7_51e0_a431_f84658a83e9e
);
impl windows_core::RuntimeType for IComboBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IComboBox_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IComboBoxFactory,
    IComboBoxFactory_Vtbl,
    0x71c1014b_acdf_5c03_b5ed_02871caaeb6b
);
impl windows_core::RuntimeType for IComboBoxFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IComboBoxFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IComboBoxItem,
    IComboBoxItem_Vtbl,
    0xdf65f6f4_863f_558d_bc4e_82c12f28062f
);
impl windows_core::RuntimeType for IComboBoxItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IComboBoxItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IComboBoxItemFactory,
    IComboBoxItemFactory_Vtbl,
    0x8540b906_c96b_592d_88d7_0c23a3386d93
);
impl windows_core::RuntimeType for IComboBoxItemFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IComboBoxItemFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IContentControl,
    IContentControl_Vtbl,
    0x07e81761_11b2_52ae_8f8b_4d53d2b5900a
);
impl windows_core::RuntimeType for IContentControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IContentControl {
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Content)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContent)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IContentControl_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Content: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IControl,
    IControl_Vtbl,
    0x857d6e8a_d45a_5c69_a99c_bf6a5c54fb38
);
impl windows_core::RuntimeType for IControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IControl {
    pub fn IsEnabled(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsEnabled)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsEnabled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsEnabled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IControl_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    IsFocusEngagementEnabled: usize,
    SetIsFocusEngagementEnabled: usize,
    IsFocusEngaged: usize,
    SetIsFocusEngaged: usize,
    RequiresPointer: usize,
    SetRequiresPointer: usize,
    FontSize: usize,
    SetFontSize: usize,
    FontFamily: usize,
    SetFontFamily: usize,
    FontWeight: usize,
    SetFontWeight: usize,
    FontStyle: usize,
    SetFontStyle: usize,
    FontStretch: usize,
    SetFontStretch: usize,
    CharacterSpacing: usize,
    SetCharacterSpacing: usize,
    Foreground: usize,
    SetForeground: usize,
    IsTextScaleFactorEnabled: usize,
    SetIsTextScaleFactorEnabled: usize,
    pub IsEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDependencyObject,
    IDependencyObject_Vtbl,
    0xe7beaee7_160e_50f7_8789_d63463f979fa
);
impl windows_core::RuntimeType for IDependencyObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDependencyObject_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDispatcherQueue,
    IDispatcherQueue_Vtbl,
    0xf6ebf8fa_be1c_5bf6_a467_73da28738ae8
);
impl windows_core::RuntimeType for IDispatcherQueue {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDispatcherQueue {
    pub fn CreateTimer(&self) -> windows_core::Result<DispatcherQueueTimer> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CreateTimer)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn TryEnqueue<P0>(&self, callback: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<DispatcherQueueHandler>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TryEnqueue)(
                windows_core::Interface::as_raw(self),
                callback.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IDispatcherQueue_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateTimer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub TryEnqueue: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut bool,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDispatcherQueue3,
    IDispatcherQueue3_Vtbl,
    0x14a7a175_5c27_5a35_b079_21960cf764a8
);
impl windows_core::RuntimeType for IDispatcherQueue3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDispatcherQueue3 {
    pub fn EnqueueEventLoopExit(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).EnqueueEventLoopExit)(
                windows_core::Interface::as_raw(self),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IDispatcherQueue3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub EnqueueEventLoopExit:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDispatcherQueueStatics,
    IDispatcherQueueStatics_Vtbl,
    0xcd3382ea_a455_5124_b63a_ca40d34ca23c
);
impl windows_core::RuntimeType for IDispatcherQueueStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDispatcherQueueStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetForCurrentThread: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDispatcherQueueTimer,
    IDispatcherQueueTimer_Vtbl,
    0xad4d63fd_88fe_541f_ac11_bf2dc1ed2ce5
);
impl windows_core::RuntimeType for IDispatcherQueueTimer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDispatcherQueueTimer {
    pub fn Interval(&self) -> windows_core::Result<windows_time::TimeSpan> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Interval)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetInterval(&self, value: windows_time::TimeSpan) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetInterval)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn IsRepeating(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsRepeating)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsRepeating(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsRepeating)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Start(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Start)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
    pub fn Stop(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Stop)(windows_core::Interface::as_raw(self)).ok()
        }
    }
    pub fn Tick<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<DispatcherQueueTimer>,
                windows_core::Ref<windows_core::IInspectable>,
            ) + 'static,
    {
        let handler: TypedEventHandler<DispatcherQueueTimer, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < DispatcherQueueTimer , windows_core::IInspectable > , F >::new (& TypedEventHandlerBox::< DispatcherQueueTimer , windows_core::IInspectable , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Tick)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTick,
            ))
        }
    }
}
#[repr(C)]
pub struct IDispatcherQueueTimer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Interval: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut windows_time::TimeSpan,
    ) -> windows_core::HRESULT,
    pub SetInterval: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        windows_time::TimeSpan,
    ) -> windows_core::HRESULT,
    IsRunning: usize,
    pub IsRepeating:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsRepeating:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub Start: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub Stop: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub Tick: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTick: unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFrameworkElement,
    IFrameworkElement_Vtbl,
    0xfe08f13d_dc6a_5495_ad44_c2d8d21863b0
);
impl windows_core::RuntimeType for IFrameworkElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IFrameworkElement {
    pub fn Width(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Width)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetWidth)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Height(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Height)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHeight)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn ActualThemeChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<FrameworkElement>, windows_core::Ref<windows_core::IInspectable>)
            + 'static,
    {
        let handler: TypedEventHandler<FrameworkElement, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<FrameworkElement, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<FrameworkElement, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).ActualThemeChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveActualThemeChanged,
            ))
        }
    }
}
#[repr(C)]
pub struct IFrameworkElement_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Triggers: usize,
    Resources: usize,
    SetResources: usize,
    Tag: usize,
    SetTag: usize,
    Language: usize,
    SetLanguage: usize,
    ActualWidth: usize,
    ActualHeight: usize,
    pub Width: unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub SetWidth: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    pub Height:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub SetHeight: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    MinWidth: usize,
    SetMinWidth: usize,
    MaxWidth: usize,
    SetMaxWidth: usize,
    MinHeight: usize,
    SetMinHeight: usize,
    MaxHeight: usize,
    SetMaxHeight: usize,
    HorizontalAlignment: usize,
    SetHorizontalAlignment: usize,
    VerticalAlignment: usize,
    SetVerticalAlignment: usize,
    Margin: usize,
    SetMargin: usize,
    Name: usize,
    SetName: usize,
    BaseUri: usize,
    DataContext: usize,
    SetDataContext: usize,
    AllowFocusOnInteraction: usize,
    SetAllowFocusOnInteraction: usize,
    FocusVisualMargin: usize,
    SetFocusVisualMargin: usize,
    FocusVisualSecondaryThickness: usize,
    SetFocusVisualSecondaryThickness: usize,
    FocusVisualPrimaryThickness: usize,
    SetFocusVisualPrimaryThickness: usize,
    FocusVisualSecondaryBrush: usize,
    SetFocusVisualSecondaryBrush: usize,
    FocusVisualPrimaryBrush: usize,
    SetFocusVisualPrimaryBrush: usize,
    AllowFocusWhenDisabled: usize,
    SetAllowFocusWhenDisabled: usize,
    Style: usize,
    SetStyle: usize,
    Parent: usize,
    FlowDirection: usize,
    SetFlowDirection: usize,
    RequestedTheme: usize,
    SetRequestedTheme: usize,
    IsLoaded: usize,
    ActualTheme: usize,
    Loaded: usize,
    RemoveLoaded: usize,
    Unloaded: usize,
    RemoveUnloaded: usize,
    DataContextChanged: usize,
    RemoveDataContextChanged: usize,
    SizeChanged: usize,
    RemoveSizeChanged: usize,
    LayoutUpdated: usize,
    RemoveLayoutUpdated: usize,
    Loading: usize,
    RemoveLoading: usize,
    pub ActualThemeChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveActualThemeChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IItemsControl,
    IItemsControl_Vtbl,
    0xbf1ccb54_83e2_5b98_acbc_736f876c3d35
);
impl windows_core::RuntimeType for IItemsControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IItemsControl {
    pub fn Items(&self) -> windows_core::Result<ItemCollection> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Items)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IItemsControl_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ItemsSource: usize,
    SetItemsSource: usize,
    pub Items: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ILaunchActivatedEventArgs,
    ILaunchActivatedEventArgs_Vtbl,
    0xd505cea9_1bcb_5b29_a8be_944e00f06f78
);
impl windows_core::RuntimeType for ILaunchActivatedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ILaunchActivatedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IOverlappedPresenter,
    IOverlappedPresenter_Vtbl,
    0x21693970_4f4c_5172_9e9d_682a2d174884
);
impl windows_core::RuntimeType for IOverlappedPresenter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IOverlappedPresenter_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IOverlappedPresenter3,
    IOverlappedPresenter3_Vtbl,
    0x55d26138_4c38_57e7_a0c1_d467b774db8c
);
impl windows_core::RuntimeType for IOverlappedPresenter3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IOverlappedPresenter3 {
    pub fn PreferredMinimumHeight(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PreferredMinimumHeight)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
            .and_then(|r__: windows_reference::IReference<i32>| r__.Value())
        }
    }
    pub fn SetPreferredMinimumHeight(&self, value: Option<i32>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<i32> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetPreferredMinimumHeight)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
    pub fn PreferredMinimumWidth(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PreferredMinimumWidth)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
            .and_then(|r__: windows_reference::IReference<i32>| r__.Value())
        }
    }
    pub fn SetPreferredMinimumWidth(&self, value: Option<i32>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<i32> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetPreferredMinimumWidth)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IOverlappedPresenter3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub PreferredMinimumHeight: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetPreferredMinimumHeight: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub PreferredMinimumWidth: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetPreferredMinimumWidth: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(IPanel, IPanel_Vtbl, 0x27a1b418_56f3_525e_b883_cefed905eed3);
impl windows_core::RuntimeType for IPanel {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IPanel {
    pub fn Children(&self) -> windows_core::Result<UIElementCollection> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Children)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IPanel_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Children: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IProgressBar,
    IProgressBar_Vtbl,
    0x87555c8c_0aaf_52c1_8390_0db17f40438e
);
impl windows_core::RuntimeType for IProgressBar {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IProgressBar_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IProgressBarFactory,
    IProgressBarFactory_Vtbl,
    0x189826ad_f6f2_533e_9ddb_b6600e88675b
);
impl windows_core::RuntimeType for IProgressBarFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IProgressBarFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IRangeBase,
    IRangeBase_Vtbl,
    0x540d6d61_8fac_5d5c_b5b0_e172a7dde103
);
impl windows_core::RuntimeType for IRangeBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IRangeBase {
    pub fn Minimum(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Minimum)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetMinimum(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMinimum)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Maximum(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Maximum)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetMaximum(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMaximum)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Value(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Value)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetValue(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetValue)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IRangeBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Minimum:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub SetMinimum: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    pub Maximum:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub SetMaximum: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    SmallChange: usize,
    SetSmallChange: usize,
    LargeChange: usize,
    SetLargeChange: usize,
    pub Value: unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub SetValue: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IResourceDictionary,
    IResourceDictionary_Vtbl,
    0x1b690975_a710_5783_a6e1_15836f6186c2
);
impl windows_core::RuntimeType for IResourceDictionary {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IResourceDictionary {
    pub fn MergedDictionaries(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<ResourceDictionary>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).MergedDictionaries)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IResourceDictionary_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Source: usize,
    SetSource: usize,
    pub MergedDictionaries: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IRoutedEventArgs,
    IRoutedEventArgs_Vtbl,
    0x0908c407_1c7d_5de3_9c50_d971c62ec8ec
);
impl windows_core::RuntimeType for IRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IRoutedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISelectionChangedEventArgs,
    ISelectionChangedEventArgs_Vtbl,
    0xb6c18076_4b76_5416_ad29_e2dc20c46246
);
impl windows_core::RuntimeType for ISelectionChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ISelectionChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISelector,
    ISelector_Vtbl,
    0x8f7e2159_e61d_576f_8476_f83fde3d689e
);
impl windows_core::RuntimeType for ISelector {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ISelector {
    pub fn SelectedIndex(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectedIndex)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetSelectedIndex(&self, value: i32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSelectedIndex)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SelectionChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<SelectionChangedEventArgs>,
            ) + 'static,
    {
        let handler: SelectionChangedEventHandler = {
            let com = windows_core::imp::DelegateBox::<SelectionChangedEventHandler, F>::new(
                &SelectionChangedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).SelectionChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveSelectionChanged,
            ))
        }
    }
}
#[repr(C)]
pub struct ISelector_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub SelectedIndex:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    pub SetSelectedIndex:
        unsafe extern "system" fn(*mut core::ffi::c_void, i32) -> windows_core::HRESULT,
    SelectedItem: usize,
    SetSelectedItem: usize,
    SelectedValue: usize,
    SetSelectedValue: usize,
    SelectedValuePath: usize,
    SetSelectedValuePath: usize,
    IsSynchronizedWithCurrentItem: usize,
    SetIsSynchronizedWithCurrentItem: usize,
    pub SelectionChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveSelectionChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ISelectorItem,
    ISelectorItem_Vtbl,
    0x5772c4de_60ea_5492_8c5e_b3323d5a3ca6
);
impl windows_core::RuntimeType for ISelectorItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ISelectorItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITextBlock,
    ITextBlock_Vtbl,
    0x1ac8d84f_392c_5c7e_83f5_a53e3bf0abb0
);
impl windows_core::RuntimeType for ITextBlock {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITextBlock {
    pub fn TextWrapping(&self) -> windows_core::Result<TextWrapping> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TextWrapping)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetTextWrapping(&self, value: TextWrapping) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetTextWrapping)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Text(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Text)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetText(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ITextBlock_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FontSize: usize,
    SetFontSize: usize,
    FontFamily: usize,
    SetFontFamily: usize,
    FontWeight: usize,
    SetFontWeight: usize,
    FontStyle: usize,
    SetFontStyle: usize,
    FontStretch: usize,
    SetFontStretch: usize,
    CharacterSpacing: usize,
    SetCharacterSpacing: usize,
    Foreground: usize,
    SetForeground: usize,
    pub TextWrapping: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut TextWrapping,
    ) -> windows_core::HRESULT,
    pub SetTextWrapping:
        unsafe extern "system" fn(*mut core::ffi::c_void, TextWrapping) -> windows_core::HRESULT,
    TextTrimming: usize,
    SetTextTrimming: usize,
    TextAlignment: usize,
    SetTextAlignment: usize,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextBox,
    ITextBox_Vtbl,
    0x873af7c2_ab89_5d76_8dbe_3d6325669df5
);
impl windows_core::RuntimeType for ITextBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITextBox {
    pub fn Text(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Text)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetText(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn TextChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<TextChangedEventArgs>,
            ) + 'static,
    {
        let handler: TextChangedEventHandler = {
            let com = windows_core::imp::DelegateBox::<TextChangedEventHandler, F>::new(
                &TextChangedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).TextChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTextChanged,
            ))
        }
    }
}
#[repr(C)]
pub struct ITextBox_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    SelectedText: usize,
    SetSelectedText: usize,
    SelectionLength: usize,
    SetSelectionLength: usize,
    SelectionStart: usize,
    SetSelectionStart: usize,
    MaxLength: usize,
    SetMaxLength: usize,
    IsReadOnly: usize,
    SetIsReadOnly: usize,
    AcceptsReturn: usize,
    SetAcceptsReturn: usize,
    TextAlignment: usize,
    SetTextAlignment: usize,
    TextWrapping: usize,
    SetTextWrapping: usize,
    IsSpellCheckEnabled: usize,
    SetIsSpellCheckEnabled: usize,
    IsTextPredictionEnabled: usize,
    SetIsTextPredictionEnabled: usize,
    InputScope: usize,
    SetInputScope: usize,
    Header: usize,
    SetHeader: usize,
    HeaderTemplate: usize,
    SetHeaderTemplate: usize,
    PlaceholderText: usize,
    SetPlaceholderText: usize,
    SelectionHighlightColor: usize,
    SetSelectionHighlightColor: usize,
    PreventKeyboardDisplayOnProgrammaticFocus: usize,
    SetPreventKeyboardDisplayOnProgrammaticFocus: usize,
    IsColorFontEnabled: usize,
    SetIsColorFontEnabled: usize,
    SelectionHighlightColorWhenNotFocused: usize,
    SetSelectionHighlightColorWhenNotFocused: usize,
    HorizontalTextAlignment: usize,
    SetHorizontalTextAlignment: usize,
    CharacterCasing: usize,
    SetCharacterCasing: usize,
    PlaceholderForeground: usize,
    SetPlaceholderForeground: usize,
    CanPasteClipboardContent: usize,
    CanUndo: usize,
    CanRedo: usize,
    SelectionFlyout: usize,
    SetSelectionFlyout: usize,
    ProofingMenuFlyout: usize,
    Description: usize,
    SetDescription: usize,
    pub TextChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTextChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextBoxFactory,
    ITextBoxFactory_Vtbl,
    0xe1d8b82e_bc60_5d27_b646_5ca4c4a69432
);
impl windows_core::RuntimeType for ITextBoxFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ITextBoxFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextChangedEventArgs,
    ITextChangedEventArgs_Vtbl,
    0x71c37e43_7be7_52fc_bf8c_9867f44be5f4
);
impl windows_core::RuntimeType for ITextChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ITextChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IToggleButton,
    IToggleButton_Vtbl,
    0x686fbaa4_c866_568b_8f75_481d8d545291
);
impl windows_core::RuntimeType for IToggleButton {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IToggleButton {
    pub fn IsChecked(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsChecked)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
            .and_then(|r__: windows_reference::IReference<bool>| r__.Value())
        }
    }
    pub fn SetIsChecked(&self, value: Option<bool>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<bool> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetIsChecked)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IToggleButton_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsChecked: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetIsChecked: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IUIElement,
    IUIElement_Vtbl,
    0xc3c01020_320c_5cf6_9d24_d396bbfa4d8b
);
impl windows_core::RuntimeType for IUIElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IUIElement {
    pub fn DesiredSize(&self) -> windows_core::Result<Size> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).DesiredSize)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Measure(&self, availablesize: Size) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Measure)(
                windows_core::Interface::as_raw(self),
                availablesize,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IUIElement_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub DesiredSize:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut Size) -> windows_core::HRESULT,
    AllowDrop: usize,
    SetAllowDrop: usize,
    Opacity: usize,
    SetOpacity: usize,
    Clip: usize,
    SetClip: usize,
    RenderTransform: usize,
    SetRenderTransform: usize,
    Projection: usize,
    SetProjection: usize,
    Transform3D: usize,
    SetTransform3D: usize,
    RenderTransformOrigin: usize,
    SetRenderTransformOrigin: usize,
    IsHitTestVisible: usize,
    SetIsHitTestVisible: usize,
    Visibility: usize,
    SetVisibility: usize,
    RenderSize: usize,
    UseLayoutRounding: usize,
    SetUseLayoutRounding: usize,
    Transitions: usize,
    SetTransitions: usize,
    CacheMode: usize,
    SetCacheMode: usize,
    IsTapEnabled: usize,
    SetIsTapEnabled: usize,
    IsDoubleTapEnabled: usize,
    SetIsDoubleTapEnabled: usize,
    CanDrag: usize,
    SetCanDrag: usize,
    IsRightTapEnabled: usize,
    SetIsRightTapEnabled: usize,
    IsHoldingEnabled: usize,
    SetIsHoldingEnabled: usize,
    ManipulationMode: usize,
    SetManipulationMode: usize,
    PointerCaptures: usize,
    ContextFlyout: usize,
    SetContextFlyout: usize,
    CompositeMode: usize,
    SetCompositeMode: usize,
    Lights: usize,
    CanBeScrollAnchor: usize,
    SetCanBeScrollAnchor: usize,
    ExitDisplayModeOnAccessKeyInvoked: usize,
    SetExitDisplayModeOnAccessKeyInvoked: usize,
    IsAccessKeyScope: usize,
    SetIsAccessKeyScope: usize,
    AccessKeyScopeOwner: usize,
    SetAccessKeyScopeOwner: usize,
    AccessKey: usize,
    SetAccessKey: usize,
    KeyTipPlacementMode: usize,
    SetKeyTipPlacementMode: usize,
    KeyTipHorizontalOffset: usize,
    SetKeyTipHorizontalOffset: usize,
    KeyTipVerticalOffset: usize,
    SetKeyTipVerticalOffset: usize,
    KeyTipTarget: usize,
    SetKeyTipTarget: usize,
    XYFocusKeyboardNavigation: usize,
    SetXYFocusKeyboardNavigation: usize,
    XYFocusUpNavigationStrategy: usize,
    SetXYFocusUpNavigationStrategy: usize,
    XYFocusDownNavigationStrategy: usize,
    SetXYFocusDownNavigationStrategy: usize,
    XYFocusLeftNavigationStrategy: usize,
    SetXYFocusLeftNavigationStrategy: usize,
    XYFocusRightNavigationStrategy: usize,
    SetXYFocusRightNavigationStrategy: usize,
    KeyboardAccelerators: usize,
    KeyboardAcceleratorPlacementTarget: usize,
    SetKeyboardAcceleratorPlacementTarget: usize,
    KeyboardAcceleratorPlacementMode: usize,
    SetKeyboardAcceleratorPlacementMode: usize,
    HighContrastAdjustment: usize,
    SetHighContrastAdjustment: usize,
    TabFocusNavigation: usize,
    SetTabFocusNavigation: usize,
    OpacityTransition: usize,
    SetOpacityTransition: usize,
    Translation: usize,
    SetTranslation: usize,
    TranslationTransition: usize,
    SetTranslationTransition: usize,
    Rotation: usize,
    SetRotation: usize,
    RotationTransition: usize,
    SetRotationTransition: usize,
    Scale: usize,
    SetScale: usize,
    ScaleTransition: usize,
    SetScaleTransition: usize,
    TransformMatrix: usize,
    SetTransformMatrix: usize,
    CenterPoint: usize,
    SetCenterPoint: usize,
    RotationAxis: usize,
    SetRotationAxis: usize,
    ActualOffset: usize,
    ActualSize: usize,
    XamlRoot: usize,
    SetXamlRoot: usize,
    Shadow: usize,
    SetShadow: usize,
    RasterizationScale: usize,
    SetRasterizationScale: usize,
    FocusState: usize,
    UseSystemFocusVisuals: usize,
    SetUseSystemFocusVisuals: usize,
    XYFocusLeft: usize,
    SetXYFocusLeft: usize,
    XYFocusRight: usize,
    SetXYFocusRight: usize,
    XYFocusUp: usize,
    SetXYFocusUp: usize,
    XYFocusDown: usize,
    SetXYFocusDown: usize,
    IsTabStop: usize,
    SetIsTabStop: usize,
    TabIndex: usize,
    SetTabIndex: usize,
    KeyUp: usize,
    RemoveKeyUp: usize,
    KeyDown: usize,
    RemoveKeyDown: usize,
    GotFocus: usize,
    RemoveGotFocus: usize,
    LostFocus: usize,
    RemoveLostFocus: usize,
    DragStarting: usize,
    RemoveDragStarting: usize,
    DropCompleted: usize,
    RemoveDropCompleted: usize,
    CharacterReceived: usize,
    RemoveCharacterReceived: usize,
    DragEnter: usize,
    RemoveDragEnter: usize,
    DragLeave: usize,
    RemoveDragLeave: usize,
    DragOver: usize,
    RemoveDragOver: usize,
    Drop: usize,
    RemoveDrop: usize,
    PointerPressed: usize,
    RemovePointerPressed: usize,
    PointerMoved: usize,
    RemovePointerMoved: usize,
    PointerReleased: usize,
    RemovePointerReleased: usize,
    PointerEntered: usize,
    RemovePointerEntered: usize,
    PointerExited: usize,
    RemovePointerExited: usize,
    PointerCaptureLost: usize,
    RemovePointerCaptureLost: usize,
    PointerCanceled: usize,
    RemovePointerCanceled: usize,
    PointerWheelChanged: usize,
    RemovePointerWheelChanged: usize,
    Tapped: usize,
    RemoveTapped: usize,
    DoubleTapped: usize,
    RemoveDoubleTapped: usize,
    Holding: usize,
    RemoveHolding: usize,
    ContextRequested: usize,
    RemoveContextRequested: usize,
    ContextCanceled: usize,
    RemoveContextCanceled: usize,
    RightTapped: usize,
    RemoveRightTapped: usize,
    ManipulationStarting: usize,
    RemoveManipulationStarting: usize,
    ManipulationInertiaStarting: usize,
    RemoveManipulationInertiaStarting: usize,
    ManipulationStarted: usize,
    RemoveManipulationStarted: usize,
    ManipulationDelta: usize,
    RemoveManipulationDelta: usize,
    ManipulationCompleted: usize,
    RemoveManipulationCompleted: usize,
    AccessKeyDisplayRequested: usize,
    RemoveAccessKeyDisplayRequested: usize,
    AccessKeyDisplayDismissed: usize,
    RemoveAccessKeyDisplayDismissed: usize,
    AccessKeyInvoked: usize,
    RemoveAccessKeyInvoked: usize,
    ProcessKeyboardAccelerators: usize,
    RemoveProcessKeyboardAccelerators: usize,
    GettingFocus: usize,
    RemoveGettingFocus: usize,
    LosingFocus: usize,
    RemoveLosingFocus: usize,
    NoFocusCandidateFound: usize,
    RemoveNoFocusCandidateFound: usize,
    PreviewKeyDown: usize,
    RemovePreviewKeyDown: usize,
    PreviewKeyUp: usize,
    RemovePreviewKeyUp: usize,
    BringIntoViewRequested: usize,
    RemoveBringIntoViewRequested: usize,
    pub Measure: unsafe extern "system" fn(*mut core::ffi::c_void, Size) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindow,
    IWindow_Vtbl,
    0x61f0ec79_5d52_56b5_86fb_40fa4af288b0
);
impl windows_core::RuntimeType for IWindow {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IWindow {
    pub fn Content(&self) -> windows_core::Result<UIElement> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Content)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContent)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Title(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Title)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetTitle(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetTitle)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn SizeChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<WindowSizeChangedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<windows_core::IInspectable, WindowSizeChangedEventArgs> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<windows_core::IInspectable, WindowSizeChangedEventArgs>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        windows_core::IInspectable,
                        WindowSizeChangedEventArgs,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).SizeChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveSizeChanged,
            ))
        }
    }
    pub fn Activate(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Activate)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
    pub fn Close(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Close)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}
#[repr(C)]
pub struct IWindow_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Bounds: usize,
    Visible: usize,
    pub Content: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    CoreWindow: usize,
    Compositor: usize,
    Dispatcher: usize,
    DispatcherQueue: usize,
    pub Title: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetTitle: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    ExtendsContentIntoTitleBar: usize,
    SetExtendsContentIntoTitleBar: usize,
    Activated: usize,
    RemoveActivated: usize,
    Closed: usize,
    RemoveClosed: usize,
    pub SizeChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveSizeChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    VisibilityChanged: usize,
    RemoveVisibilityChanged: usize,
    pub Activate: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub Close: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindow2,
    IWindow2_Vtbl,
    0x42febaa5_1c32_522a_a591_57618c6f665d
);
impl windows_core::RuntimeType for IWindow2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IWindow2 {
    pub fn AppWindow(&self) -> windows_core::Result<AppWindow> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).AppWindow)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IWindow2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    SystemBackdrop: usize,
    SetSystemBackdrop: usize,
    pub AppWindow: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowFactory,
    IWindowFactory_Vtbl,
    0xf0441536_afef_5222_918f_324a9b2dec75
);
impl windows_core::RuntimeType for IWindowFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IWindowFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowSizeChangedEventArgs,
    IWindowSizeChangedEventArgs_Vtbl,
    0x542f6f2c_4b64_5c72_a7a5_3a7e0664b8ff
);
impl windows_core::RuntimeType for IWindowSizeChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IWindowSizeChangedEventArgs {
    pub fn Size(&self) -> windows_core::Result<Size> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Size)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IWindowSizeChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Handled: usize,
    SetHandled: usize,
    pub Size: unsafe extern "system" fn(*mut core::ffi::c_void, *mut Size) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IXamlControlsResources,
    IXamlControlsResources_Vtbl,
    0x918ca043_f42c_5805_861b_62d6d1d0c162
);
impl windows_core::RuntimeType for IXamlControlsResources {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IXamlControlsResources_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IXamlMetadataProvider,
    IXamlMetadataProvider_Vtbl,
    0xa96251f0_2214_5d53_8746_ce99a2593cd7
);
impl windows_core::RuntimeType for IXamlMetadataProvider {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Markup.IXamlMetadataProvider",
    );
}
windows_core::imp::interface_hierarchy!(
    IXamlMetadataProvider,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl IXamlMetadataProvider {
    pub fn GetXamlType(&self, r#type: &TypeName) -> windows_core::Result<IXamlType> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetXamlType)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(r#type),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn GetXamlTypeByFullName(&self, fullname: &str) -> windows_core::Result<IXamlType> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetXamlTypeByFullName)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(fullname)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn GetXmlnsDefinitions(
        &self,
    ) -> windows_core::Result<windows_core::Array<XmlnsDefinition>> {
        unsafe {
            let mut result__ = core::mem::MaybeUninit::zeroed();
            (windows_core::Interface::vtable(self).GetXmlnsDefinitions)(
                windows_core::Interface::as_raw(self),
                windows_core::Array::<XmlnsDefinition>::set_abi_len(core::mem::transmute(
                    &mut result__,
                )),
                result__.as_mut_ptr() as *mut _ as _,
            )
            .map(|| result__.assume_init())
        }
    }
}
impl windows_core::RuntimeName for IXamlMetadataProvider {
    const NAME: &'static str = "Microsoft.UI.Xaml.Markup.IXamlMetadataProvider";
}
pub trait IXamlMetadataProvider_Impl: windows_core::IUnknownImpl {
    fn GetXamlType(&self, r#type: &TypeName) -> windows_core::Result<IXamlType>;
    fn GetXamlTypeByFullName(
        &self,
        fullName: &windows_core::HSTRING,
    ) -> windows_core::Result<IXamlType>;
    fn GetXmlnsDefinitions(&self) -> windows_core::Result<windows_core::Array<XmlnsDefinition>>;
}
impl IXamlMetadataProvider_Vtbl {
    pub const fn new<Identity: IXamlMetadataProvider_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn GetXamlType<
            Identity: IXamlMetadataProvider_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            r#type: core::mem::MaybeUninit<TypeName>,
            result__: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match IXamlMetadataProvider_Impl::GetXamlType(this, core::mem::transmute(&r#type)) {
                    Ok(ok__) => {
                        result__.write(core::mem::transmute_copy(&ok__));
                        core::mem::forget(ok__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        unsafe extern "system" fn GetXamlTypeByFullName<
            Identity: IXamlMetadataProvider_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            fullname: *mut core::ffi::c_void,
            result__: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match IXamlMetadataProvider_Impl::GetXamlTypeByFullName(
                    this,
                    core::mem::transmute(&fullname),
                ) {
                    Ok(ok__) => {
                        result__.write(core::mem::transmute_copy(&ok__));
                        core::mem::forget(ok__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        unsafe extern "system" fn GetXmlnsDefinitions<
            Identity: IXamlMetadataProvider_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            result_size__: *mut u32,
            result__: *mut *mut core::mem::MaybeUninit<XmlnsDefinition>,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match IXamlMetadataProvider_Impl::GetXmlnsDefinitions(this) {
                    Ok(ok__) => {
                        let (ok_data__, ok_data_len__) = ok__.into_abi();
                        result__.write(ok_data__);
                        result_size__.write(ok_data_len__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        Self {
            base__: windows_core::IInspectable_Vtbl::new::<Identity, IXamlMetadataProvider, OFFSET>(
            ),
            GetXamlType: GetXamlType::<Identity, OFFSET>,
            GetXamlTypeByFullName: GetXamlTypeByFullName::<Identity, OFFSET>,
            GetXmlnsDefinitions: GetXmlnsDefinitions::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<IXamlMetadataProvider as windows_core::Interface>::IID
    }
}
#[repr(C)]
pub struct IXamlMetadataProvider_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetXamlType: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        core::mem::MaybeUninit<TypeName>,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetXamlTypeByFullName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetXmlnsDefinitions: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut u32,
        *mut *mut core::mem::MaybeUninit<XmlnsDefinition>,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IXamlType,
    IXamlType_Vtbl,
    0xd24219df_7ec9_57f1_a27b_6af251d9c5bc
);
impl windows_core::RuntimeType for IXamlType {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IXamlType,
    windows_core::IUnknown,
    windows_core::IInspectable
);
#[repr(C)]
pub struct IXamlType_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemCollection(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ItemCollection,
    windows_core::IUnknown,
    windows_core::IInspectable,
    windows_collections::IObservableVector<windows_core::IInspectable>
);
impl windows_core::RuntimeType for ItemCollection {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        windows_collections::IObservableVector<windows_core::IInspectable>,
    >();
}
unsafe impl windows_core::Interface for ItemCollection {
    type Vtable = < windows_collections::IObservableVector < windows_core::IInspectable > as windows_core::Interface >::Vtable ;
    const IID: windows_core::GUID = <windows_collections::IObservableVector<
        windows_core::IInspectable,
    > as windows_core::Interface>::IID;
}
impl core::ops::Deref for ItemCollection {
    type Target = windows_collections::IObservableVector<windows_core::IInspectable>;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ItemCollection {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ItemCollection";
}
unsafe impl Send for ItemCollection {}
unsafe impl Sync for ItemCollection {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemsControl(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ItemsControl,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ItemsControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IItemsControl>();
}
unsafe impl windows_core::Interface for ItemsControl {
    type Vtable = <IItemsControl as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IItemsControl as windows_core::Interface>::IID;
}
impl core::ops::Deref for ItemsControl {
    type Target = IItemsControl;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ItemsControl {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ItemsControl";
}
unsafe impl Send for ItemsControl {}
unsafe impl Sync for ItemsControl {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchActivatedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    LaunchActivatedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for LaunchActivatedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ILaunchActivatedEventArgs>();
}
unsafe impl windows_core::Interface for LaunchActivatedEventArgs {
    type Vtable = <ILaunchActivatedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ILaunchActivatedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for LaunchActivatedEventArgs {
    type Target = ILaunchActivatedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for LaunchActivatedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.LaunchActivatedEventArgs";
}
unsafe impl Send for LaunchActivatedEventArgs {}
unsafe impl Sync for LaunchActivatedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlappedPresenter(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    OverlappedPresenter,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(OverlappedPresenter, AppWindowPresenter);
impl windows_core::RuntimeType for OverlappedPresenter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IOverlappedPresenter>();
}
unsafe impl windows_core::Interface for OverlappedPresenter {
    type Vtable = <IOverlappedPresenter as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IOverlappedPresenter as windows_core::Interface>::IID;
}
impl core::ops::Deref for OverlappedPresenter {
    type Target = IOverlappedPresenter;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for OverlappedPresenter {
    const NAME: &'static str = "Microsoft.UI.Windowing.OverlappedPresenter";
}
unsafe impl Send for OverlappedPresenter {}
unsafe impl Sync for OverlappedPresenter {}
pub type PACKAGEDEPENDENCY_CONTEXT = *mut core::ffi::c_void;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PACKAGE_VERSION {
    pub Anonymous: PACKAGE_VERSION_0,
}
impl Default for PACKAGE_VERSION {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub union PACKAGE_VERSION_0 {
    pub Version: u64,
    pub Anonymous: PACKAGE_VERSION_0_0,
}
impl Default for PACKAGE_VERSION_0 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PACKAGE_VERSION_0_0 {
    pub Revision: u16,
    pub Build: u16,
    pub Minor: u16,
    pub Major: u16,
}
pub type PSID = *mut core::ffi::c_void;
pub type PackageDependencyLifetimeKind = i32;
pub const PackageDependencyLifetimeKind_Process: PackageDependencyLifetimeKind = 0;
pub type PackageDependencyProcessorArchitectures = u32;
pub const PackageDependencyProcessorArchitectures_Arm: PackageDependencyProcessorArchitectures = 8;
pub const PackageDependencyProcessorArchitectures_Arm64: PackageDependencyProcessorArchitectures =
    16;
pub const PackageDependencyProcessorArchitectures_Neutral: PackageDependencyProcessorArchitectures =
    1;
pub const PackageDependencyProcessorArchitectures_X64: PackageDependencyProcessorArchitectures = 4;
pub const PackageDependencyProcessorArchitectures_X86: PackageDependencyProcessorArchitectures = 2;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Panel(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Panel, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Panel, FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for Panel {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPanel>();
}
unsafe impl windows_core::Interface for Panel {
    type Vtable = <IPanel as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPanel as windows_core::Interface>::IID;
}
impl core::ops::Deref for Panel {
    type Target = IPanel;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Panel {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Panel";
}
unsafe impl Send for Panel {}
unsafe impl Sync for Panel {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressBar(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ProgressBar,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ProgressBar,
    RangeBase,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ProgressBar {
    pub fn new() -> windows_core::Result<Self> {
        Self::IProgressBarFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IProgressBarFactory<R, F: FnOnce(&IProgressBarFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ProgressBar, IProgressBarFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ProgressBar {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IProgressBar>();
}
unsafe impl windows_core::Interface for ProgressBar {
    type Vtable = <IProgressBar as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IProgressBar as windows_core::Interface>::IID;
}
impl core::ops::Deref for ProgressBar {
    type Target = IProgressBar;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ProgressBar {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ProgressBar";
}
unsafe impl Send for ProgressBar {}
unsafe impl Sync for ProgressBar {}
pub const RO_INIT_SINGLETHREADED: RO_INIT_TYPE = 0;
pub type RO_INIT_TYPE = i32;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RangeBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    RangeBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    RangeBase,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for RangeBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IRangeBase>();
}
unsafe impl windows_core::Interface for RangeBase {
    type Vtable = <IRangeBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IRangeBase as windows_core::Interface>::IID;
}
impl core::ops::Deref for RangeBase {
    type Target = IRangeBase;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for RangeBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.RangeBase";
}
unsafe impl Send for RangeBase {}
unsafe impl Sync for RangeBase {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDictionary(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ResourceDictionary,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(ResourceDictionary, DependencyObject);
impl windows_core::RuntimeType for ResourceDictionary {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IResourceDictionary>();
}
unsafe impl windows_core::Interface for ResourceDictionary {
    type Vtable = <IResourceDictionary as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IResourceDictionary as windows_core::Interface>::IID;
}
impl core::ops::Deref for ResourceDictionary {
    type Target = IResourceDictionary;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ResourceDictionary {
    const NAME: &'static str = "Microsoft.UI.Xaml.ResourceDictionary";
}
unsafe impl Send for ResourceDictionary {}
unsafe impl Sync for ResourceDictionary {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    RoutedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for RoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IRoutedEventArgs>();
}
unsafe impl windows_core::Interface for RoutedEventArgs {
    type Vtable = <IRoutedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IRoutedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for RoutedEventArgs {
    type Target = IRoutedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for RoutedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.RoutedEventArgs";
}
unsafe impl Send for RoutedEventArgs {}
unsafe impl Sync for RoutedEventArgs {}
windows_core::imp::define_interface!(
    RoutedEventHandler,
    RoutedEventHandler_Vtbl,
    0xdae23d85_69ca_5bdf_805b_6161a3a215cc
);
impl windows_core::RuntimeType for RoutedEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct RoutedEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct RoutedEventHandlerBox<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>) + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>) + 'static,
> RoutedEventHandlerBox<F>
{
    const VTABLE: RoutedEventHandler_Vtbl = RoutedEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface: windows_core::imp::DelegateBox::<RoutedEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<RoutedEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<RoutedEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<RoutedEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SelectionChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(SelectionChangedEventArgs, RoutedEventArgs);
impl windows_core::RuntimeType for SelectionChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISelectionChangedEventArgs>();
}
unsafe impl windows_core::Interface for SelectionChangedEventArgs {
    type Vtable = <ISelectionChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISelectionChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for SelectionChangedEventArgs {
    type Target = ISelectionChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for SelectionChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.SelectionChangedEventArgs";
}
unsafe impl Send for SelectionChangedEventArgs {}
unsafe impl Sync for SelectionChangedEventArgs {}
windows_core::imp::define_interface!(
    SelectionChangedEventHandler,
    SelectionChangedEventHandler_Vtbl,
    0xa232390d_0e34_595e_8931_fa928a9909f4
);
impl windows_core::RuntimeType for SelectionChangedEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct SelectionChangedEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct SelectionChangedEventHandlerBox<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<SelectionChangedEventArgs>,
        ) + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<SelectionChangedEventArgs>,
        ) + 'static,
> SelectionChangedEventHandlerBox<F>
{
    const VTABLE: SelectionChangedEventHandler_Vtbl = SelectionChangedEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<SelectionChangedEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<SelectionChangedEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<SelectionChangedEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<SelectionChangedEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selector(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Selector,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    Selector,
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for Selector {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISelector>();
}
unsafe impl windows_core::Interface for Selector {
    type Vtable = <ISelector as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISelector as windows_core::Interface>::IID;
}
impl core::ops::Deref for Selector {
    type Target = ISelector;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Selector {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.Selector";
}
unsafe impl Send for Selector {}
unsafe impl Sync for Selector {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectorItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SelectorItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    SelectorItem,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for SelectorItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISelectorItem>();
}
unsafe impl windows_core::Interface for SelectorItem {
    type Vtable = <ISelectorItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISelectorItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for SelectorItem {
    type Target = ISelectorItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for SelectorItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.SelectorItem";
}
unsafe impl Send for SelectorItem {}
unsafe impl Sync for SelectorItem {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
impl windows_core::imp::TypeKind for Size {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Size {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.Foundation.Size;f4;f4)");
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SizeInt32 {
    pub width: i32,
    pub height: i32,
}
impl windows_core::imp::TypeKind for SizeInt32 {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for SizeInt32 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.Graphics.SizeInt32;i4;i4)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextBlock(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextBlock,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(TextBlock, FrameworkElement, UIElement, DependencyObject);
impl TextBlock {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            TextBlock,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TextBlock {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextBlock>();
}
unsafe impl windows_core::Interface for TextBlock {
    type Vtable = <ITextBlock as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextBlock as windows_core::Interface>::IID;
}
impl core::ops::Deref for TextBlock {
    type Target = ITextBlock;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TextBlock {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TextBlock";
}
unsafe impl Send for TextBlock {}
unsafe impl Sync for TextBlock {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextBox(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextBox,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    TextBox,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl TextBox {
    pub fn new() -> windows_core::Result<Self> {
        Self::ITextBoxFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn ITextBoxFactory<R, F: FnOnce(&ITextBoxFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TextBox, ITextBoxFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TextBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextBox>();
}
unsafe impl windows_core::Interface for TextBox {
    type Vtable = <ITextBox as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextBox as windows_core::Interface>::IID;
}
impl core::ops::Deref for TextBox {
    type Target = ITextBox;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TextBox {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TextBox";
}
unsafe impl Send for TextBox {}
unsafe impl Sync for TextBox {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(TextChangedEventArgs, RoutedEventArgs);
impl windows_core::RuntimeType for TextChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextChangedEventArgs>();
}
unsafe impl windows_core::Interface for TextChangedEventArgs {
    type Vtable = <ITextChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for TextChangedEventArgs {
    type Target = ITextChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TextChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TextChangedEventArgs";
}
unsafe impl Send for TextChangedEventArgs {}
unsafe impl Sync for TextChangedEventArgs {}
windows_core::imp::define_interface!(
    TextChangedEventHandler,
    TextChangedEventHandler_Vtbl,
    0x5d8ddcff_45d8_5e7c_9b8b_c41d2893c6a1
);
impl windows_core::RuntimeType for TextChangedEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct TextChangedEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct TextChangedEventHandlerBox<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<TextChangedEventArgs>)
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<TextChangedEventArgs>)
        + 'static,
> TextChangedEventHandlerBox<F>
{
    const VTABLE: TextChangedEventHandler_Vtbl = TextChangedEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<TextChangedEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<TextChangedEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<TextChangedEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<TextChangedEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextWrapping(pub i32);
impl TextWrapping {
    pub const NoWrap: Self = Self(1);
    pub const Wrap: Self = Self(2);
    pub const WrapWholeWords: Self = Self(3);
}
impl windows_core::imp::TypeKind for TextWrapping {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for TextWrapping {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.TextWrapping;i4)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToggleButton(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ToggleButton,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ToggleButton,
    ButtonBase,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ToggleButton {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IToggleButton>();
}
unsafe impl windows_core::Interface for ToggleButton {
    type Vtable = <IToggleButton as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IToggleButton as windows_core::Interface>::IID;
}
impl core::ops::Deref for ToggleButton {
    type Target = IToggleButton;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ToggleButton {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.ToggleButton";
}
unsafe impl Send for ToggleButton {}
unsafe impl Sync for ToggleButton {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TypeKind(pub i32);
impl TypeKind {
    pub const Primitive: Self = Self(0);
    pub const Metadata: Self = Self(1);
    pub const Custom: Self = Self(2);
}
impl windows_core::imp::TypeKind for TypeKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for TypeKind {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Windows.UI.Xaml.Interop.TypeKind;i4)");
}
#[repr(C)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypeName {
    pub name: windows_core::HSTRING,
    pub kind: TypeKind,
}
impl windows_core::imp::TypeKind for TypeName {
    type TypeKind = windows_core::imp::CloneType;
}
impl windows_core::RuntimeType for TypeName {
    const SIGNATURE : windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice (b"struct(Windows.UI.Xaml.Interop.TypeName;string;enum(Windows.UI.Xaml.Interop.TypeKind;i4))") ;
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedEventHandler<TSender, TResult>(
    windows_core::IUnknown,
    core::marker::PhantomData<TSender>,
    core::marker::PhantomData<TResult>,
)
where
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static;
unsafe impl<
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static,
> windows_core::Interface for TypedEventHandler<TSender, TResult>
{
    type Vtable = TypedEventHandler_Vtbl<TSender, TResult>;
    const IID: windows_core::GUID =
        windows_core::GUID::from_signature(<Self as windows_core::RuntimeType>::SIGNATURE);
}
impl<TSender: windows_core::RuntimeType + 'static, TResult: windows_core::RuntimeType + 'static>
    windows_core::RuntimeType for TypedEventHandler<TSender, TResult>
{
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::new()
        .push_slice(b"pinterface({9de1c534-6ae1-11e0-84e1-18a905bcc53f}")
        .push_slice(b";")
        .push_other(TSender::SIGNATURE)
        .push_slice(b";")
        .push_other(TResult::SIGNATURE)
        .push_slice(b")");
}
#[repr(C)]
pub struct TypedEventHandler_Vtbl<TSender, TResult>
where
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static,
{
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: windows_core::imp::AbiType<TSender>,
        args: windows_core::imp::AbiType<TResult>,
    ) -> windows_core::HRESULT,
    TSender: core::marker::PhantomData<TSender>,
    TResult: core::marker::PhantomData<TResult>,
}
struct TypedEventHandlerBox<
    TSender,
    TResult,
    F: Fn(windows_core::Ref<TSender>, windows_core::Ref<TResult>) + 'static,
>(core::marker::PhantomData<(TSender, TResult, fn() -> F)>)
where
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static;
impl<
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static,
    F: Fn(windows_core::Ref<TSender>, windows_core::Ref<TResult>) + 'static,
> TypedEventHandlerBox<TSender, TResult, F>
{
    const VTABLE : TypedEventHandler_Vtbl < TSender , TResult , > = TypedEventHandler_Vtbl::< TSender , TResult , > { base__ : windows_core::IUnknown_Vtbl { QueryInterface : windows_core::imp::DelegateBox::< TypedEventHandler < TSender , TResult > , F >::QueryInterface , AddRef : windows_core::imp::DelegateBox::< TypedEventHandler < TSender , TResult > , F >::AddRef , Release : windows_core::imp::DelegateBox::< TypedEventHandler < TSender , TResult > , F >::Release , } , Invoke : Self::Invoke , TSender : core::marker::PhantomData::< TSender > , TResult : core::marker::PhantomData::< TResult > } ;
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: windows_core::imp::AbiType<TSender>,
        args: windows_core::imp::AbiType<TResult>,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<TypedEventHandler<TSender, TResult>, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&args),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UIElement(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    UIElement,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(UIElement, DependencyObject);
impl windows_core::RuntimeType for UIElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IUIElement>();
}
unsafe impl windows_core::Interface for UIElement {
    type Vtable = <IUIElement as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IUIElement as windows_core::Interface>::IID;
}
impl core::ops::Deref for UIElement {
    type Target = IUIElement;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for UIElement {
    const NAME: &'static str = "Microsoft.UI.Xaml.UIElement";
}
unsafe impl Send for UIElement {}
unsafe impl Sync for UIElement {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UIElementCollection(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    UIElementCollection,
    windows_core::IUnknown,
    windows_core::IInspectable,
    windows_collections::IVector<UIElement>
);
impl windows_core::RuntimeType for UIElementCollection {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, windows_collections::IVector<UIElement>>(
        );
}
unsafe impl windows_core::Interface for UIElementCollection {
    type Vtable = <windows_collections::IVector<UIElement> as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <windows_collections::IVector<UIElement> as windows_core::Interface>::IID;
}
impl core::ops::Deref for UIElementCollection {
    type Target = windows_collections::IVector<UIElement>;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for UIElementCollection {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.UIElementCollection";
}
unsafe impl Send for UIElementCollection {}
unsafe impl Sync for UIElementCollection {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Window(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Window, windows_core::IUnknown, windows_core::IInspectable);
impl Window {
    pub fn new() -> windows_core::Result<Self> {
        Self::IWindowFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IWindowFactory<R, F: FnOnce(&IWindowFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Window, IWindowFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Window {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindow>();
}
unsafe impl windows_core::Interface for Window {
    type Vtable = <IWindow as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindow as windows_core::Interface>::IID;
}
impl core::ops::Deref for Window {
    type Target = IWindow;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Window {
    const NAME: &'static str = "Microsoft.UI.Xaml.Window";
}
unsafe impl Send for Window {}
unsafe impl Sync for Window {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowSizeChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowSizeChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for WindowSizeChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowSizeChangedEventArgs>();
}
unsafe impl windows_core::Interface for WindowSizeChangedEventArgs {
    type Vtable = <IWindowSizeChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindowSizeChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for WindowSizeChangedEventArgs {
    type Target = IWindowSizeChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for WindowSizeChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.WindowSizeChangedEventArgs";
}
unsafe impl Send for WindowSizeChangedEventArgs {}
unsafe impl Sync for WindowSizeChangedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XamlControlsResources(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    XamlControlsResources,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(XamlControlsResources, ResourceDictionary, DependencyObject);
impl XamlControlsResources {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            XamlControlsResources,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for XamlControlsResources {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IXamlControlsResources>();
}
unsafe impl windows_core::Interface for XamlControlsResources {
    type Vtable = <IXamlControlsResources as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IXamlControlsResources as windows_core::Interface>::IID;
}
impl core::ops::Deref for XamlControlsResources {
    type Target = IXamlControlsResources;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for XamlControlsResources {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.XamlControlsResources";
}
unsafe impl Send for XamlControlsResources {}
unsafe impl Sync for XamlControlsResources {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XamlControlsXamlMetaDataProvider(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    XamlControlsXamlMetaDataProvider,
    windows_core::IUnknown,
    windows_core::IInspectable,
    IXamlMetadataProvider
);
impl XamlControlsXamlMetaDataProvider {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            XamlControlsXamlMetaDataProvider,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for XamlControlsXamlMetaDataProvider {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IXamlMetadataProvider>();
}
unsafe impl windows_core::Interface for XamlControlsXamlMetaDataProvider {
    type Vtable = <IXamlMetadataProvider as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IXamlMetadataProvider as windows_core::Interface>::IID;
}
impl core::ops::Deref for XamlControlsXamlMetaDataProvider {
    type Target = IXamlMetadataProvider;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for XamlControlsXamlMetaDataProvider {
    const NAME: &'static str = "Microsoft.UI.Xaml.XamlTypeInfo.XamlControlsXamlMetaDataProvider";
}
unsafe impl Send for XamlControlsXamlMetaDataProvider {}
unsafe impl Sync for XamlControlsXamlMetaDataProvider {}
#[repr(C)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XmlnsDefinition {
    pub xml_namespace: windows_core::HSTRING,
    pub namespace: windows_core::HSTRING,
}
impl windows_core::imp::TypeKind for XmlnsDefinition {
    type TypeKind = windows_core::imp::CloneType;
}
impl windows_core::RuntimeType for XmlnsDefinition {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"struct(Microsoft.UI.Xaml.Markup.XmlnsDefinition;string;string)",
    );
}
