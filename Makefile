## Build x86_64 iso with Grub2
TARGET_ARCH = x86_64

## Directory Settings
MAKE_BASEDIR ?= $(shell pwd)/
MAKE_BINDIR = $(MAKE_BASEDIR)bin/
MAKE_IMGDIR = $(MAKE_BINDIR)img/
MAKE_TMPDIR = $(MAKE_BASEDIR)tmp/
MAKE_CONGIGDIR =  $(MAKE_BASEDIR)config/$(TARGET_ARCH)/

## Software Paths
MKDIR = mkdir -p
CP = cp -r
GRUBMKRES = grub-mkrescue
GRUB2MKRES = grub2-mkrescue

iso:
	-$(MKDIR) $(MAKE_IMGDIR) $(MAKE_TMPDIR)grub-iso/boot/grub/ $(MAKE_TMPDIR)grub-iso/boot/methylenix/
	$(CP) target/x86_64-unknown-none/release/Methylenix $(MAKE_TMPDIR)grub-iso/boot/methylenix/kernel.elf
	$(CP) $(MAKE_CONGIGDIR)grub  $(MAKE_TMPDIR)grub-iso/boot/
	$(GRUBMKRES) -o $(MAKE_IMGDIR)boot.iso $(MAKE_TMPDIR)grub-iso || $(GRUB2MKRES) -o $(MAKE_IMGDIR)boot.iso $(MAKE_TMPDIR)grub-iso
