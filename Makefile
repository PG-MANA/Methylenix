#環境設定
##名前
NAME = methylenix

##ターゲット
TARGET_ARCH = $(2)
ifeq ($(strip $(TARGET_ARCH)),)
    TARGET_ARCH = x86_64
endif
RUST_TARGET = $(TARGET_ARCH)-unknown-none
RUST_TARGET_JSON = config/$(TARGET_ARCH)/$(RUST_TARGET).json

##ディレクトリ
SRC = src/
MAKE_BASEDIR ?= $(shell pwd)/
MAKE_BINDIR = $(MAKE_BASEDIR)bin/
MAKE_IMGDIR = $(MAKE_BINDIR)img/
MAKE_TMPDIR = $(MAKE_BASEDIR)tmp/
MAKE_CONGIGDIR =  $(MAKE_BASEDIR)config/$(TARGET_ARCH)/

##ソフトウェア
STRIP = strip
OBJCOPY = objcopy
MKDIR = mkdir -p
CP = cp -r
RM = rm -rf
GRUBMKRES = grub-mkrescue
GRUB2MKRES = grub2-mkrescue #Temporary
#LD = ld -n --gc-sections -Map $(MAKE_TMPDIR)$(NAME).map -nostartfiles -nodefaultlibs -nostdlib -T $(MAKE_CONGIGDIR)linkerscript.ld
LD = ld.lld --no-nmagic --gc-sections --Map=$(MAKE_TMPDIR)$(NAME).map  -nostdlib --script=$(MAKE_CONGIGDIR)linkerscript.ld
CARGO = cargo

##ビルドファイル
KERNELFILES = kernel.elf
RUST_OBJ = target/$(RUST_TARGET)/release/lib$(NAME).a
BOOT_SYS_LIST = $(RUST_OBJ)

#初期設定
export TARGET_ARCH
export MAKE_BINDIR
export MAKE_TMPDIR
export MAKE_OBJDIR

#各コマンド
##デフォルト動作
default:
	$(MAKE) kernel

##初期化動作
init:
	-$(MKDIR) $(MAKE_BINDIR)
	-$(MKDIR) $(MAKE_TMPDIR)

clean:
	$(RM) $(MAKE_TMPDIR)
	$(CARGO) clean

iso:
	$(MAKE) kernel
	-$(MKDIR) $(MAKE_IMGDIR) $(MAKE_TMPDIR)grub-iso/boot/grub/ $(MAKE_TMPDIR)grub-iso/boot/methylenix/
	$(CP) $(MAKE_BINDIR)kernel.elf $(MAKE_TMPDIR)grub-iso/boot/methylenix/
	$(CP) $(MAKE_CONGIGDIR)/grub  $(MAKE_TMPDIR)grub-iso/boot/
	$(GRUBMKRES) -o $(MAKE_IMGDIR)boot.iso $(MAKE_TMPDIR)grub-iso/ || $(GRUB2MKRES) -o $(MAKE_IMGDIR)boot.iso $(MAKE_TMPDIR)grub-iso/

kernel:
	$(MAKE) init
	$(MAKE) $(KERNELFILES)


# ファイル生成規則
kernel.elf : $(BOOT_SYS_LIST)
	$(LD) -o $(MAKE_BINDIR)kernel.elf $(BOOT_SYS_LIST)
	-$(OBJCOPY) --only-keep-debug $(MAKE_BINDIR)kernel.elf $(MAKE_BINDIR)kernel.elf.debug
	$(CP) $(MAKE_BINDIR)kernel.elf $(MAKE_BINDIR)kernel_original.elf
	-$(STRIP) $(MAKE_BINDIR)kernel.elf

$(RUST_OBJ) :  .FORCE
	$(CARGO) xbuild --release --target $(RUST_TARGET_JSON)

.FORCE:
