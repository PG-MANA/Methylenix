# Methylenix

An operating system written in Rust

## About

Methylenix is an operating system written in Rust.
Except for the boot sequence and some special instructions, All processes are written in Rust.

This is my hobby project.
I'm not aiming for practical use.

The origin of the name Methylenix comes from the methylene group.
I aimed to develop a UNIX-like OS by combining modules like organic compounds.

Therefore, I named this OS Methylene-nix, Methylenix.

## The origin (Japanese)

このプログラムの原点は、セキュリティ・キャンプ全国大会2017 集中コース
「X 言語やOSを自作しよう」に受講生として参加した際に開発を行ったことです。
セキュリティ・キャンプについては、[セキュリティ・キャンプ：IPA 独立行政法人 情報処理推進機構](https://www.ipa.go.jp/jinzai/camp/index.html)
をご覧ください。

セキュリティキャンプでは割り込みまでを実装しました。([セキュリティキャンプ2017参加記 | PG_MANAの雑記](https://pg-mana.net/blog/seccamp_after/))

## Current Status

- Manage memory resources
- Manage the process and thread
- Support multi-processors
- Support ACPI Machine Language
    - Developed the interpreter
- Support a simple GUI
    - Show the debug messages
- Read data from NVMe
- Support some file systems
    - FAT32(Read Only)
    - XFS(Read Only)
- Support Socket API
- Run applications
    - Compatible with Linux/FreeBSD

## Supported Architectures

* x86_64
* AArch64

## License

Copyright 2018 PG_MANA

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

## Build

### Dependencies

- rustc (nightly)
- cargo
- grub2-mkrescue (x86_64 only)

### Steps

#### x86_64

```shell
git clone https://github.com/PG-MANA/Methylenix.git
cd Methylenix
rustup target add x86_64-unknown-none
cargo xtask build x86_64 --release
```

The kernel will be placed in `bin/`.

You can download built images from https://repo.taprix.org/pg_mana/methylenix/images/x86_64 .

### AArch64

```shell
git clone https://github.com/PG-MANA/Methylenix.git
cd Methylenix
rustup target add aarch64-unknown-uefi
rustup target add aarch64-unknown-none-softfloat
cargo xtask build aarch64 --release
```

The kernel will be placed in `bin/`.

You can download built images from https://repo.taprix.org/pg_mana/methylenix/images/aarch64 .

## Run on the QEMU

### x86_64

```shell
qemu-system-x86_64 -cpu qemu64,+fsgsbase --cdrom bin/img/boot.iso

# or (OVMF)
qemu-system-x86_64 --cdrom bin/img/boot.iso -cpu qemu64,+fsgsbase -smp 2 -m 512M -bios /usr/bin/OVMF/OVMF.fd

# or (to emulate host cpu)
qemu-system-x86_64 --cdrom bin/img/boot.iso  -cpu host -smp 2 -m 512M -bios /usr/bin/OVMF/OVMF.fd --enable-kvm

# NIC and NVMe Emulation
qemu-system-x86_64 -drive if=pflash,format=raw,readonly=on,file=/path/to/OVMF_CODE.fd -drive if=pflash,format=raw,file=/path/to/QEMU_VARS.fd -m 1G -cdrom bin/img/boot.iso -smp 4 --enable-kvm -cpu host -netdev user,id=net0,hostfwd=tcp::7777-:8080 -device e1000e,netdev=net0,mac=52:54:00:12:34:56 -drive file=/path/to/img.qcow2,if=none,id=nvm1 -device nvme,serial=12345678,drive=nvm -device nvme,id=nvm,serial=deadbeef -device nvme-ns,drive=nvm1
```

### AArch64

```shell
qemu-system-aarch64 -m 1G -cpu a64fx -machine virt,gic-version=3 -smp 2 -nographic -bios /usr/bin/OVMF/OVMF_AARCH64.fd  -drive file=fat:rw:bin/,format=raw,media=disk
```

## Documents

```shell
cargo doc --open 
```

## Links

### Author's Twitter account

[https://twitter.com/PG_MANA_](https://twitter.com/PG_MANA_)

### Author's website

https://pg-mana.net

### Project website

https://methylenix.org (Currently, only redirects to GitHub)

