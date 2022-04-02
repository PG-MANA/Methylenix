#環境設定
##名前
NAME = methylenix_loader

RUST_TARGET=aarch64-unknown-uefi

MAKE_BASEDIR ?= ../../../../
MAKE_EFIDIR = $(MAKE_BASEDIR)bin/

##ソフトウェア
CP = cp -r
CARGO = cargo

##ビルドファイル
KERNELFILES = kernel.elf
RUST_BIN = target/$(RUST_TARGET)/release/$(NAME).efi

#初期設定
export MAKE_BINDIR

#各コマンド
##デフォルト動作
.DEFAULT: all

all:
	$(CARGO) build --release
	$(CP) $(RUST_BIN) $(MAKE_EFIDIR)EFI/BOOT/BOOTAA64.EFI

clean:
	$(CARGO) clean