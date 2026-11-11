#[cfg(all(
    not(miri),
    any(
        all(target_arch = "aarch64", not(windows)),
        all(target_arch = "x86_64", not(windows))
    )
))]
mod regs;
#[cfg(all(
    not(miri),
    any(
        all(target_arch = "aarch64", not(windows)),
        all(target_arch = "x86_64", not(windows))
    )
))]
mod stub;
