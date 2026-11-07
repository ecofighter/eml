//! Windows App SDKのruntimeのbootstrap。Windows 11のkernelbaseが持つ動的な依存の関数`TryCreatePackageDependency`と
//! `AddPackageDependency`で、runtimeのframeworkのパッケージをプロセスに加える。emlはbootstrapのDLLを配らず、runtimeの
//! DLLを名前で読み込むこともない (docs/implementation/distribution.mdの「Windows App SDKのruntime」)。

use std::sync::OnceLock;

use windows_core::{HRESULT, HSTRING, PCWSTR, PWSTR, s, w};

use super::bindings::{
    AddPackageDependencyOptions, AddPackageDependencyOptions_None, CreatePackageDependencyOptions,
    CreatePackageDependencyOptions_None, GetModuleHandleW, GetProcAddress, GetProcessHeap,
    HeapFree, PACKAGE_VERSION, PACKAGE_VERSION_0, PACKAGEDEPENDENCY_CONTEXT, PSID,
    PackageDependencyLifetimeKind, PackageDependencyLifetimeKind_Process,
    PackageDependencyProcessorArchitectures, PackageDependencyProcessorArchitectures_None,
};
use crate::GuiError;

const NO_RUNTIME: &str = "cannot open the GUI: the Windows App SDK 2 runtime is not installed (https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads)";

/// runtimeのframeworkのパッケージのfamily。2.xの版はどれも同じfamilyを使う (NuGetのパッケージ
/// Microsoft.WindowsAppSDK.Runtimeの`WindowsAppSDK-VersionInfo.h`の`WINDOWSAPPSDK_RUNTIME_PACKAGE_FRAMEWORK_PACKAGEFAMILYNAME`)。
const FAMILY: &str = "Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe";

/// 受け入れる一番古いruntime。bindingのメタデータの元の版なので、bindingのAPIはどれもruntimeにある
/// (nix/winui-bindings.sh)。同じヘッダは、これを`WINDOWSAPPSDK_RUNTIME_VERSION_UINT64`として示す。
const MIN_VERSION: [u16; 4] = [2, 6, 4, 0];

/// `PACKAGE_VERSION`は、major、minor、build、revisionをそれぞれ16ビットに詰め、majorを上位に置く。
fn pack_version([major, minor, build, revision]: [u16; 4]) -> u64 {
    (u64::from(major) << 48)
        | (u64::from(minor) << 32)
        | (u64::from(build) << 16)
        | u64::from(revision)
}

type TryCreatePackageDependency = unsafe extern "system" fn(
    user: PSID,
    family: PCWSTR,
    min_version: PACKAGE_VERSION,
    architectures: PackageDependencyProcessorArchitectures,
    lifetime: PackageDependencyLifetimeKind,
    lifetime_artifact: PCWSTR,
    options: CreatePackageDependencyOptions,
    id: *mut PWSTR,
) -> HRESULT;

type AddPackageDependency = unsafe extern "system" fn(
    id: PCWSTR,
    rank: i32,
    options: AddPackageDependencyOptions,
    context: *mut PACKAGEDEPENDENCY_CONTEXT,
    full_name: *mut PWSTR,
) -> HRESULT;

/// 依存はプロセスで1度だけ加え、プロセスの終わりまで外さない。外すと、読み込んだWinUIのDLLを使えなくなるからである。
/// 失敗はどれも、その版以降のruntimeがないものとして扱う。
pub(crate) fn bootstrap() -> Result<(), GuiError> {
    static DONE: OnceLock<Result<(), GuiError>> = OnceLock::new();
    DONE.get_or_init(|| {
        // SAFETY: `add_dependency`は文書にある関数を呼ぶだけで、引数はどれも呼び出しの間生きている
        unsafe { add_dependency() }.map_err(|_| GuiError::NotAvailable(NO_RUNTIME.to_string()))
    })
    .clone()
}

/// 2つの関数は、importせずに、どのプロセスも読み込んでいるkernelbaseから引く。Windows 10はこれらを持たず、importすると、
/// GUIを使わないときもemlがそこで起動できなくなるからである。
unsafe fn add_dependency() -> windows_core::Result<()> {
    // SAFETY: すでに読み込まれたmoduleのハンドルを取るだけで、何も読み込まない
    let kernelbase = unsafe { GetModuleHandleW(w!("kernelbase.dll")) };
    if kernelbase.is_null() {
        return Err(windows_core::Error::from_thread());
    }
    // SAFETY: `kernelbase`は生きているmoduleのハンドルで、名前はNULで終わる
    let found = unsafe {
        (
            GetProcAddress(kernelbase, s!("TryCreatePackageDependency")),
            GetProcAddress(kernelbase, s!("AddPackageDependency")),
        )
    };
    let (Some(try_create), Some(add)) = found else {
        return Err(windows_core::Error::from_thread());
    };
    // SAFETY: 2つの関数の型はこのとおりである (Windows SDKの`appmodel.h`)
    let (try_create, add) = unsafe {
        (
            std::mem::transmute::<unsafe extern "system" fn() -> isize, TryCreatePackageDependency>(
                try_create,
            ),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, AddPackageDependency>(add),
        )
    };
    let family = HSTRING::from(FAMILY);
    let mut id = PWSTR::null();
    // SAFETY: ポインタは、呼び出しの間生きている値を指す
    unsafe {
        try_create(
            PSID::default(),
            PCWSTR(family.as_ptr()),
            PACKAGE_VERSION {
                Anonymous: PACKAGE_VERSION_0 {
                    Version: pack_version(MIN_VERSION),
                },
            },
            PackageDependencyProcessorArchitectures_None,
            PackageDependencyLifetimeKind_Process,
            PCWSTR::null(),
            CreatePackageDependencyOptions_None,
            &mut id,
        )
    }
    .ok()?;
    let mut context = PACKAGEDEPENDENCY_CONTEXT::default();
    // SAFETY: ポインタは、呼び出しの間生きている値を指す
    let added = unsafe {
        add(
            PCWSTR(id.0),
            0,
            AddPackageDependencyOptions_None,
            &mut context,
            std::ptr::null_mut(),
        )
    };
    // SAFETY: IDは`TryCreatePackageDependency`がプロセスのヒープに取ったもので、この後は使わない
    unsafe {
        let _ = HeapFree(GetProcessHeap(), 0, id.0.cast());
    }
    added.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// familyと最小の版は、Microsoft.WindowsAppSDK.Runtime 2.6.4の`WindowsAppSDK-VersionInfo.h`の値である。
    #[test]
    fn the_runtime_pins_are_the_version_header_values() {
        assert_eq!(FAMILY, "Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe");
        assert_eq!(pack_version(MIN_VERSION), 0x0002_0006_0004_0000);
    }
}
