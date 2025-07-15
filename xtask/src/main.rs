//!
//! Build System
//!

use std::{env, fs, path::Path, process::Command};

const OS_PROJECT_NAME: &str = "Methylenix";

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
    let target_arch: &str;
    let loader: fn(
        cargo: &str,
        base_dir: &Path,
        target_dir: &Path,
        output_dir: &Path,
        build_type: &str,
    ) -> i32;
    /* The compiler emits compile errors when we wrote like `(target_arch, loader) = match ...` */
    match env::args().nth(2).as_deref() {
        Some("x86_64") => {
            target_arch = "x86_64-unknown-none";
            loader = build_loader_x86_64;
        }
        Some("aarch64") => {
            target_arch = "aarch64-unknown-none-softfloat";
            loader = build_loader_aarch64;
        }
        Some(a) => {
            eprintln!("Unknown architecture: {a}");
            return show_error(cargo, true);
        }
        None => {
            eprintln!("The target architecture is not specified.");
            return show_error(cargo, true);
        }
    };
    let output_dir = base_dir.join("bin");
    let build_type = "release";
    let target_dir = base_dir.join("target");
    let kernel_path = target_dir
        .join(target_arch)
        .join(build_type)
        .join(OS_PROJECT_NAME);

    /* Create the output dir */
    if let Err(err) = fs::create_dir_all(&output_dir) {
        eprintln!("Failed to create the output dir: {err:?}");
        return -1;
    }

    /* Build the kernel */
    let status = Command::new(cargo)
        .args([
            "build",
            format!("--{build_type}").as_str(),
            "--target",
            target_arch,
        ])
        .status();
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
        build_type,
    );
    if status != 0 {
        return status;
    }
    0
}

fn build_loader_x86_64(
    _cargo: &str,
    base_dir: &Path,
    _target_dir: &Path,
    output_dir: &Path,
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

fn build_loader_aarch64(
    cargo: &str,
    _base_dir: &Path,
    target_dir: &Path,
    output_dir: &Path,
    build_type: &str,
) -> i32 {
    let loader_path = "src/arch/aarch64/bootloader";
    let efi_path = output_dir.join("EFI/BOOT");
    let loader_arch = "aarch64-unknown-uefi";
    let loader_name = "methylenix_loader.efi";
    let deploy_name = "BOOTAA64.EFI";

    let status = Command::new(cargo)
        .current_dir(loader_path)
        .args(["build", format!("--{build_type}").as_str()])
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
        eprintln!("Failed to copy the kernel: {err:?}");
        return -1;
    }
    0
}

fn show_error(cargo: &str, is_error: bool) -> i32 {
    eprintln!(
        "
Usage: {cargo} xtask build TARGET_ARCH

Supported arch:
    x86_64
    aarch64
 "
    );
    if is_error { -1 } else { 0 }
}
