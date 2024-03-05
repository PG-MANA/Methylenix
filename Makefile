# Methylenix Build Makefile
NAME = methylenix

TARGET_ARCH ?= x86_64
RUST_TARGET = $(TARGET_ARCH)-unknown-none

## Directory Settings
SRC = src/
MAKE_BASEDIR ?= $(shell pwd)/
MAKE_BINDIR = $(MAKE_BASEDIR)bin/
MAKE_EFIDIR = $(MAKE_BINDIR)EFI/BOOT/
MAKE_IMGDIR = $(MAKE_BINDIR)img/
MAKE_TMPDIR = $(MAKE_BASEDIR)tmp/
MAKE_CONGIGDIR =  $(MAKE_BASEDIR)config/$(TARGET_ARCH)/
BOOTLOADER = $(SRC)arch/$(TARGET_ARCH)/bootloader

## Software Paths
MKDIR = mkdir -p
CP = cp -r
RM = rm -rf
GRUBMKRES = grub-mkrescue
GRUB2MKRES = grub2-mkrescue
CARGO = cargo

KERNELFILES = kernel.elf
RUST_BIN = target/$(RUST_TARGET)/release/$(NAME)

export TARGET_ARCH
export MAKE_BINDIR
export MAKE_TMPDIR
export MAKE_OBJDIR

.DEFAULT: all

all: bootloader kernel

init:
	-$(MKDIR) $(MAKE_BINDIR)
	-$(MKDIR) $(MAKE_TMPDIR)
ifeq ($(strip $(TARGET_ARCH)), aarch64)
	-$(MKDIR) $(MAKE_EFIDIR)
endif

clean:
	$(RM) $(MAKE_TMPDIR)
	$(CARGO) clean
ifeq ($(strip $(TARGET_ARCH)), aarch64)
	cd $(BOOTLOADER) ; $(CARGO) clean
endif

iso: kernel
	-$(MKDIR) $(MAKE_IMGDIR) $(MAKE_TMPDIR)grub-iso/boot/grub/ $(MAKE_TMPDIR)grub-iso/boot/methylenix/
	$(CP) $(MAKE_BINDIR)kernel.elf $(MAKE_TMPDIR)grub-iso/boot/methylenix/
	$(CP) $(MAKE_CONGIGDIR)/grub  $(MAKE_TMPDIR)grub-iso/boot/
	$(GRUBMKRES) -o $(MAKE_IMGDIR)boot.iso $(MAKE_TMPDIR)grub-iso/ || $(GRUB2MKRES) -o $(MAKE_IMGDIR)boot.iso $(MAKE_TMPDIR)grub-iso/

kernel: init $(KERNELFILES)
ifeq ($(strip $(TARGET_ARCH)), aarch64)
	$(CP) $(MAKE_BINDIR)kernel.elf $(MAKE_EFIDIR)kernel.elf
endif

bootloader: init
ifeq ($(strip $(TARGET_ARCH)), aarch64)
	cd $(BOOTLOADER) ; $(CARGO) build --release
	$(CP) $(BOOTLOADER)/target/*/release/*.efi $(MAKE_EFIDIR)BOOTAA64.EFI
endif

kernel.elf : .FORCE
	$(CARGO) build --release --target $(RUST_TARGET)
	$(CP) $(RUST_BIN) $(MAKE_BINDIR)kernel.elf


.FORCE:
