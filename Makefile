#環境設定
##名前
NAME = methylenix

##ターゲット
TARGET_ARCH ?= x86_64

RUST_TARGET = $(TARGET_ARCH)-unknown-none
RUST_TARGET_JSON = config/$(TARGET_ARCH)/$(RUST_TARGET).json

##ディレクトリ
SRC = src/
MAKE_BASEDIR ?= $(shell pwd)/
MAKE_BINDIR = $(MAKE_BASEDIR)bin/
MAKE_EFIDIR = $(MAKE_BINDIR)EFI/BOOT/
MAKE_IMGDIR = $(MAKE_BINDIR)img/
MAKE_TMPDIR = $(MAKE_BASEDIR)tmp/
MAKE_CONGIGDIR =  $(MAKE_BASEDIR)config/$(TARGET_ARCH)/
BOOTLOADER = $(SRC)arch/$(TARGET_ARCH)/bootloader

##ソフトウェア
MAKE_SUB = $(MAKE) -C
MKDIR = mkdir -p
CP = cp -r
RM = rm -rf
GRUBMKRES = grub-mkrescue
GRUB2MKRES = grub2-mkrescue #Temporary
#LD = ld -n --gc-sections -Map $(MAKE_TMPDIR)$(NAME).map -nostartfiles -nodefaultlibs -nostdlib -T $(MAKE_CONGIGDIR)linkerscript.ld
#LD = ld.lld --no-nmagic --gc-sections --Map=$(MAKE_TMPDIR)$(NAME).map  -nostdlib --script=$(MAKE_CONGIGDIR)linkerscript.ld
CARGO = cargo

##ビルドファイル
KERNELFILES = kernel.elf
RUST_BIN = target/$(RUST_TARGET)/release/$(NAME)

#初期設定
export TARGET_ARCH
export MAKE_BINDIR
export MAKE_TMPDIR
export MAKE_OBJDIR

#各コマンド
##デフォルト動作
.DEFAULT: all

all: bootloader kernel

##初期化動作
init:
	-$(MKDIR) $(MAKE_BINDIR)
	-$(MKDIR) $(MAKE_TMPDIR)

clean:
	$(RM) $(MAKE_TMPDIR)
	$(CARGO) clean
ifeq ($(strip $(TARGET_ARCH)), aarch64)
	$(MAKE_SUB) $(BOOTLOADER) clean
endif

iso: kernel
	-$(MKDIR) $(MAKE_IMGDIR) $(MAKE_TMPDIR)grub-iso/boot/grub/ $(MAKE_TMPDIR)grub-iso/boot/methylenix/
	$(CP) $(MAKE_BINDIR)kernel.elf $(MAKE_TMPDIR)grub-iso/boot/methylenix/
	$(CP) $(MAKE_CONGIGDIR)/grub  $(MAKE_TMPDIR)grub-iso/boot/
	$(GRUBMKRES) -o $(MAKE_IMGDIR)boot.iso $(MAKE_TMPDIR)grub-iso/ || $(GRUB2MKRES) -o $(MAKE_IMGDIR)boot.iso $(MAKE_TMPDIR)grub-iso/

kernel: init $(KERNELFILES)
ifeq ($(strip $(TARGET_ARCH)), aarch64)
	$(CP) $(MAKE_BINDIR)kernel.elf $(MAKE_EFIDIR)kernel.elf
	$(MAKE_SUB) $(BOOTLOADER)
endif

bootloader: init
ifeq ($(strip $(TARGET_ARCH)), aarch64)
	-$(MKDIR) $(MAKE_EFIDIR)
	$(MAKE_SUB) $(BOOTLOADER)
endif

# ファイル生成規則
kernel.elf : .FORCE
	$(CARGO) build --release --target $(RUST_TARGET_JSON)
	$(CP) $(RUST_BIN) $(MAKE_BINDIR)kernel.elf


.FORCE:
