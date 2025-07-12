use std::{env, fs, path::Path, process::Command};

const OS_PROJECT_NAME: &str = "methylenix";

fn main() {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let ret = match env::args().nth(1).as_deref() {
        Some("build") => build(&cargo),
        Some("help") => show_error(&cargo, false),
        Some(c) => {
            eprintln!("Unknown command: {c}");
            show_error(&cargo, true)
        }
        None => show_error(&cargo, true),
    };
    std::process::exit(ret);
}

fn build(cargo: &String) -> i32 {
    let (target_arch, loader, loader_arch, original_loader_name, loader_name) =
        match env::args().nth(2).as_deref() {
            Some("x86_64") => ("x86_64-unknown-none", None, "", "", ""),
            Some("aarch64") => (
                "aarch64-unknown-none-softfloat",
                Some("src/arch/aarch64/bootloader"),
                "aarch64-unknown-uefi",
                "methylenix_loader.efi",
                "BOOTAA64.EFI",
            ),
            Some(a) => {
                eprintln!("Unknown architecture: {a}");
                return show_error(cargo, true);
            }
            None => {
                eprintln!("The target architecture is not specified.");
                return show_error(cargo, true);
            }
        };
    let output_dir = "bin/EFI/BOOT";
    let build_type = "release";
    let Ok(current_dir) = env::current_dir() else {
        eprintln!("Failed to get the current directory.");
        return -1;
    };
    let kernel_path = current_dir
        .clone()
        .join("target")
        .join(target_arch)
        .join(build_type)
        .join(OS_PROJECT_NAME);

    /* Create the output dir */
    if let Err(err) = fs::create_dir_all(output_dir) {
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
    if let Err(err) = fs::copy(kernel_path, Path::new(output_dir).join("kernel.elf")) {
        eprintln!("Failed to copy the kernel: {err:?}");
        return -1;
    }

    /* Build the loader */
    if let Some(loader) = loader {
        let loader_path = current_dir
            .clone()
            .join("target")
            .join(loader_arch)
            .join(build_type)
            .join(original_loader_name);
        let status = Command::new(cargo)
            .current_dir(loader)
            .args(["build", format!("--{build_type}").as_str()])
            .status();
        if !matches!(status.as_ref().map(|s| s.success()), Ok(true)) {
            eprintln!("Building the boot loader is failed: {status:?}");
            return status.map_or(-1, |s| s.code().unwrap_or(-1));
        }

        /* Copy the loader to the output dir */
        if let Err(err) = fs::copy(loader_path, Path::new(output_dir).join(loader_name)) {
            eprintln!("Failed to copy the kernel: {err:?}");
            return -1;
        }
    }

    0
}

fn show_error(cargo: &String, is_error: bool) -> i32 {
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
