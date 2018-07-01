# This software is Licensed under the Apache License Version 2.0 
# See LICENSE
#

#環境設定
##名前
NAME = methylenix

##ターゲット
TARGET_ARCH = $(2)
ifeq ($(strip $(TARGET_ARCH)),)
    TARGET_ARCH = x86_64
endif
RUST_TARGET = $(TARGET_ARCH)-unknown-none
#RUST_TARGET_FILE_FOLDER = target-json/ # https://github.com/japaric/xargo/issues/146

##ディレクトリ
SRC = src/
MAKE_BASEDIR ?= $(shell pwd)/
MAKE_BINDIR = $(MAKE_BASEDIR)bin/
MAKE_IMGDIR = $(MAKE_BINDIR)img/
MAKE_TMPDIR = $(MAKE_BASEDIR)tmp/
MAKE_OBJDIR = $(MAKE_TMPDIR)obj/
MAKE_CONGIGDIR =  $(MAKE_BASEDIR)config/$(TARGET_ARCH)/

##ソフトウェア
STRIP= strip
MKDIR = mkdir -p
CP = cp -r
RM = rm -rf
GRUBMKRES = grub2-mkrescue
AR = ar rcs
LD = ld -n --gc-sections -Map $(MAKE_TMPDIR)$(NAME).map -nostartfiles -nodefaultlibs -nostdlib -T $(MAKE_CONGIGDIR)linkerscript.ld
XARGO = xargo
include config/$(TARGET_ARCH)/assembler.mk
export AR
##ビルドファイル
KERNELFILES = kernel.sys
RUST_OBJ = target/$(RUST_TARGET)/release/lib$(NAME).a
BOOT_SYS_LIST = $(MAKE_OBJDIR)boot_asm.a $(RUST_OBJ)


#初期設定
export TARGET_ARCH
export MAKE_BINDIR
export MAKE_TMPDIR
export MAKE_OBJDIR


#各コマンド
##デフォルト動作
default:
	$(MAKE) kernel
	-$(STRIP) $(MAKE_BINDIR) *.sys #できなくてもいい

##初期化動作
init:
	-$(MKDIR) $(MAKE_BINDIR)
	-$(MKDIR) $(MAKE_TMPDIR)
	-$(MKDIR) $(MAKE_OBJDIR)

clean:
	$(RM) $(MAKE_TMPDIR)
	$(XARGO) clean

iso: 
	$(MAKE) kernel
	-$(MKDIR) $(MAKE_IMGDIR) $(MAKE_TMPDIR)grub-iso/boot/grub/ $(MAKE_TMPDIR)grub-iso/methylenix/
	$(CP) $(MAKE_BINDIR)kernel.sys $(MAKE_TMPDIR)grub-iso/methylenix/
	$(CP) $(MAKE_CONGIGDIR)/grub  $(MAKE_TMPDIR)grub-iso/boot/
	$(GRUBMKRES) -o $(MAKE_IMGDIR)boot.iso $(MAKE_TMPDIR)grub-iso/

kernel: 
	$(MAKE) init
	$(MAKE) $(KERNELFILES)


# ファイル生成規則
kernel.sys : $(BOOT_SYS_LIST)
	$(LD) -o $(MAKE_BINDIR)kernel.sys $(BOOT_SYS_LIST)

$(MAKE_OBJDIR)boot_asm.a : src/arch/$(TARGET_ARCH)/boot/Makefile .FORCE
	$(MAKE) -C src/arch/$(TARGET_ARCH)/boot/

$(RUST_OBJ) :  .FORCE
	$(XARGO) build --release --target $(RUST_TARGET_FILE_FOLDER) $(RUST_TARGET) 

.FORCE:
