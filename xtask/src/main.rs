//!
//! Build System
//!

use std::{env, fmt::Display, fs, path::Path, process::Command};

const OS_PROJECT_NAME: &str = "Methylenix";

#[derive(Copy, Clone, PartialEq, Eq)]
enum TargetArch {
    X86_64,
    AArch64,
    RiscV64,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum LoaderType {
    Uefi,
    Baremetal,
    Grub,
}

fn main() {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let base_dir = Path::new(manifest_dir.as_str()).parent().unwrap();
    let ret = match env::args().nth(1).as_deref() {
        Some("build") => build(cargo.as_str(), base_dir),
        Some("help") => show_error(cargo.as_str(), false),
        Some(c) => {
            eprintln!("Unknown command: {c}");
            show_error(&cargo, true)
        }
        None => show_error(&cargo, true),
    };
    std::process::exit(ret);
}

fn build(cargo: &str, base_dir: &Path) -> i32 {
    let loader: fn(
        cargo: &str,
        base_dir: &Path,
        target_dir: &Path,
        output_dir: &Path,
        target_arch: TargetArch,
        build_type: &str,
    ) -> i32;
    let additional_build_flags: &[&str];
    let mut build_type = "release";
    let mut target_arch: Option<TargetArch> = None;
    let mut loader_type: Option<LoaderType> = None;
    let mut feature_flags: Option<String> = None;

    let mut args = env::args().skip(2);
    while let Some(e) = args.next() {
        match e.as_str() {
            "--release" => build_type = "release",
            "--debug" => build_type = "debug",
            "--features" => {
                feature_flags = args.next();
                if feature_flags.is_none() {
                    eprintln!("Invalid feature flag option");
                    return show_error(cargo, true);
                }
            }
            "--loader" => {
                loader_type = args
                    .next()
                    .and_then(|l| LoaderType::try_from(l.as_str()).ok());
                if loader_type.is_none() {
                    eprintln!("Invalid loader type");
                    return show_error(cargo, true);
                }
            }
            a if let Ok(t) = TargetArch::try_from(a) => target_arch = Some(t),
            unknown => {
                eprintln!("Unknown argument: {unknown}");
                return show_error(cargo, true);
            }
        }
    }
    let Some(target_arch) = target_arch else {
        eprintln!("The target architecture is not specified.");
        return show_error(cargo, true);
    };
    match target_arch {
        TargetArch::X86_64 => {
            loader = match loader_type {
                Some(LoaderType::Uefi) => build_uefi_loader,
                Some(LoaderType::Baremetal) => {
                    eprintln!("Baremetal loader is not supported for {target_arch}");
                    return show_error(cargo, true);
                }
                Some(LoaderType::Grub) => build_grub_iso,
                None => build_uefi_loader,
            };
            additional_build_flags = &[];
        }
        TargetArch::AArch64 => {
            loader = match loader_type {
                Some(LoaderType::Uefi) => build_uefi_loader,
                Some(LoaderType::Baremetal) => {
                    eprintln!("Baremetal loader is not supported for {target_arch}");
                    return show_error(cargo, true);
                }
                Some(LoaderType::Grub) => {
                    eprintln!("GRUB is not supported for {target_arch}");
                    return show_error(cargo, true);
                }
                None => build_uefi_loader,
            };
            additional_build_flags = &[];
        }
        TargetArch::RiscV64 => {
            loader = match loader_type {
                Some(LoaderType::Uefi) => {
                    eprintln!("UEFI loader is not supported for {target_arch}");
                    return show_error(cargo, true);
                }
                Some(LoaderType::Baremetal) => build_baremetal_loader,
                Some(LoaderType::Grub) => {
                    eprintln!("GRUB is not supported for {target_arch}");
                    return show_error(cargo, true);
                }
                None => build_baremetal_loader,
            };
            additional_build_flags = &["-Z", "build-std=core,alloc"];
        }
    };
    let output_dir = base_dir.join("bin");
    let target_dir = base_dir.join("target");
    let target_triple = target_arch.to_target_triple();
    let kernel_path = target_dir
        .join(target_triple)
        .join(build_type)
        .join(OS_PROJECT_NAME);

    /* Create the output dir */
    if let Err(err) = fs::create_dir_all(&output_dir) {
        eprintln!("Failed to create the output dir: {err:?}");
        return -1;
    }

    /* Build the kernel */
    let mut cargo_build = Command::new(cargo);
    cargo_build.args([
        "build",
        format!("--{build_type}").as_str(),
        "--target",
        target_triple,
        "--config",
        base_dir
            .join(".cargo")
            .join("kernel.toml")
            .to_str()
            .unwrap(),
    ]);
    cargo_build.args(additional_build_flags);
    if let Some(f) = feature_flags {
        cargo_build.args(["--features", f.as_str()]);
    }
    let status = cargo_build.status();
    if !matches!(status.as_ref().map(|s| s.success()), Ok(true)) {
        eprintln!("Building the kernel is failed: {status:?}");
        return status.map_or(-1, |s| s.code().unwrap_or(-1));
    }

    /* Copy the kernel to the output dir */
    if let Err(err) = fs::copy(kernel_path, output_dir.join("kernel.elf")) {
        eprintln!("Failed to copy the kernel: {err:?}");
        return -1;
    }

    /* Build the loader */
    let status = loader(
        cargo,
        base_dir,
        target_dir.as_path(),
        output_dir.as_path(),
        target_arch,
        build_type,
    );
    if status != 0 {
        return status;
    }
    0
}

fn build_grub_iso(
    _cargo: &str,
    base_dir: &Path,
    _target_dir: &Path,
    output_dir: &Path,
    _target_arch: TargetArch,
    _build_type: &str,
) -> i32 {
    let iso_dir = output_dir.join("iso");
    let grub_dir = iso_dir.join("boot/grub");
    if let Err(err) = fs::create_dir_all(&grub_dir) {
        eprintln!("Failed to create the output dir: {err:?}");
        return -1;
    }

    /* Copy files */
    if let Err(err) = fs::copy(
        output_dir.join("kernel.elf"),
        iso_dir.join("boot/kernel.elf"),
    ) {
        eprintln!("Failed to copy the kernel: {err:?}");
        return -1;
    }
    if let Err(err) = fs::copy(
        base_dir.join("config/x86_64/grub.cfg"),
        grub_dir.join("grub.cfg"),
    ) {
        eprintln!("Failed to copy the kernel: {err:?}");
        return -1;
    }

    /* Run grub2-mkrescue */
    let mut status;
    for command_name in ["grub-mkrescue", "grub2-mkrescue"] {
        status = Command::new(command_name)
            .args([
                "-o",
                output_dir.join("boot.iso").to_str().unwrap(),
                iso_dir.to_str().unwrap(),
            ])
            .status();
        if matches!(status.as_ref().map(|s| s.success()), Ok(true)) {
            return 0;
        }
    }
    eprintln!("Building the grub iso is failed");
    -1
}

fn build_uefi_loader(
    cargo: &str,
    base_dir: &Path,
    target_dir: &Path,
    output_dir: &Path,
    target_arch: TargetArch,
    build_type: &str,
) -> i32 {
    let loader_path = "loader";
    let efi_path = output_dir.join("EFI/BOOT");
    let loader_name = "kernel_loader.efi";
    let loader_arch;
    let deploy_name;

    match target_arch {
        TargetArch::X86_64 => {
            loader_arch = "x86_64-unknown-uefi";
            deploy_name = "BOOTX64.EFI";
        }
        TargetArch::AArch64 => {
            loader_arch = "aarch64-unknown-uefi";
            deploy_name = "BOOTAA64.EFI";
        }
        TargetArch::RiscV64 => {
            loader_arch = "riscv64gc-unknown-uefi";
            deploy_name = "BOOTRV64.EFI";
        }
    }
    let status = Command::new(cargo)
        .current_dir(loader_path)
        .args([
            "build",
            format!("--{build_type}").as_str(),
            "--target",
            loader_arch,
            "--config",
            base_dir
                .join(".cargo")
                .join("loader.toml")
                .to_str()
                .unwrap(),
        ])
        .status();
    if !matches!(status.as_ref().map(|s| s.success()), Ok(true)) {
        eprintln!("Building the boot loader is failed: {status:?}");
        return status.map_or(-1, |s| s.code().unwrap_or(-1));
    }

    /* Copy the loader to the output dir */
    if let Err(err) = fs::create_dir_all(&efi_path) {
        eprintln!("Failed to create the output dir: {err:?}");
        return -1;
    }
    let binary_path = target_dir
        .join(loader_arch)
        .join(build_type)
        .join(loader_name);
    if let Err(err) = fs::copy(binary_path, efi_path.join(deploy_name)) {
        eprintln!("Failed to copy the boot loader: {err:?}");
        return -1;
    }
    0
}

fn build_baremetal_loader(
    cargo: &str,
    base_dir: &Path,
    target_dir: &Path,
    output_dir: &Path,
    target_arch: TargetArch,
    build_type: &str,
) -> i32 {
    let loader_path = "loader";
    let loader_name = "kernel_loader";
    let deploy_name = "Kernel";
    let loader_arch = target_arch.to_target_triple();
    let additional_build_flags: &[&str];

    if target_arch == TargetArch::RiscV64 {
        additional_build_flags = &["-Z", "build-std=core"];
    } else {
        additional_build_flags = &[];
    }

    let status = Command::new(cargo)
        .current_dir(loader_path)
        .args([
            "build",
            format!("--{build_type}").as_str(),
            "--target",
            loader_arch,
            "--config",
            base_dir
                .join(".cargo")
                .join("loader.toml")
                .to_str()
                .unwrap(),
        ])
        .args(additional_build_flags)
        .status();
    if !matches!(status.as_ref().map(|s| s.success()), Ok(true)) {
        eprintln!("Building the boot loader is failed: {status:?}");
        return status.map_or(-1, |s| s.code().unwrap_or(-1));
    }

    /* Copy the loader to the output dir */
    let binary_path = target_dir
        .join(loader_arch)
        .join(build_type)
        .join(loader_name);
    if let Err(err) = fs::copy(binary_path, output_dir.join(deploy_name)) {
        eprintln!("Failed to copy the kernel: {err:?}");
        return -1;
    }
    0
}

fn show_error(cargo: &str, is_error: bool) -> i32 {
    eprintln!("Usage: {cargo} xtask build TARGET_ARCH [Options]");
    eprintln!("\nSupported Architectures:");
    // make it better if Enum::iter was supported...
    [TargetArch::X86_64, TargetArch::AArch64, TargetArch::RiscV64]
        .iter()
        .for_each(|e| eprintln!("\t{e}"));
    eprintln!("\nOptions:");
    eprintln!("\t--release\t\t\tBuild in release mode");
    eprintln!("\t--debug\t\t\t\tBuild in debug mode");
    eprint!("\t--loader <Loader>\t\tSpecify the loader to build [possible values:");
    [LoaderType::Uefi, LoaderType::Baremetal, LoaderType::Grub]
        .iter()
        .for_each(|e| eprint!(" {e}"));
    eprintln!("]");
    eprintln!("\t--features <Features>\t\tBuild features to pass cargo");
    if is_error { -1 } else { 0 }
}

impl TryFrom<&str> for TargetArch {
    type Error = ();
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            s if s == <Self as Into<&str>>::into(Self::X86_64) => Ok(Self::X86_64),
            s if s == <Self as Into<&str>>::into(Self::AArch64) => Ok(Self::AArch64),
            s if s == <Self as Into<&str>>::into(Self::RiscV64) => Ok(Self::RiscV64),
            _ => Err(()),
        }
    }
}

impl Into<&str> for TargetArch {
    fn into(self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x86_64",
            TargetArch::AArch64 => "aarch64",
            TargetArch::RiscV64 => "riscv64",
        }
    }
}

impl Display for TargetArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.clone().into())
    }
}

impl TargetArch {
    const fn to_target_triple(self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x86_64-unknown-none",
            TargetArch::AArch64 => "aarch64-unknown-none-softfloat",
            TargetArch::RiscV64 => "riscv64imac-unknown-none-elf",
        }
    }
}

impl TryFrom<&str> for LoaderType {
    type Error = ();
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            s if s == <Self as Into<&str>>::into(Self::Uefi) => Ok(Self::Uefi),
            s if s == <Self as Into<&str>>::into(Self::Baremetal) => Ok(Self::Baremetal),
            s if s == <Self as Into<&str>>::into(Self::Grub) => Ok(Self::Grub),
            _ => Err(()),
        }
    }
}

impl Into<&str> for LoaderType {
    fn into(self) -> &'static str {
        match self {
            LoaderType::Uefi => "uefi",
            LoaderType::Baremetal => "baremetal",
            LoaderType::Grub => "grub",
        }
    }
}

impl Display for LoaderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.clone().into())
    }
}
