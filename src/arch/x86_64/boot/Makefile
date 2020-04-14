#環境設定(デフォルト)
MAKE_BINDIR ?= ./bin/
MAKE_TMPDIR ?= ./tmp/
MAKE_OBJDIR ?=  $(MAKE_TMPDIR)obj/
AR ?= ar rcs

#ビルドするもの
ASM_OBJS_LIST = multiboot_header.o common.o init_bios.o init_efi64.o init_long_mode.o init_x86_64.o
ASM_OBJS = $(addprefix $(MAKE_OBJDIR), $(ASM_OBJS_LIST))

default:
	$(MAKE) $(MAKE_OBJDIR)boot_asm.a

#ファイル生成規則
$(MAKE_OBJDIR)boot_asm.a: $(ASM_OBJS)
	$(AR) -o $@ $(ASM_OBJS)

$(MAKE_OBJDIR)%.o : %.asm
	$(ASSEMBLER) $< -o $@
