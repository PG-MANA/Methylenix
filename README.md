# Methylenix
(Format:UTF-8)  
The operating system written in Rust

## 概要
Methylenix とはRustで構成されたOSです。  
起動時初期化とI/O命令などアセンブリでしかかけない箇所を除き、すべてRustで記述されています。  

## Methylenixとは
このプログラムの原点は、セキュリティ・キャンプ全国大会2017 集中コース「X　言語やOSを自作しよう」に受講生として参加した際に開発を行ったことです。  
セキュリティ・キャンプについては、[セキュリティ・キャンプ：IPA 独立行政法人 情報処理推進機構](https://www.ipa.go.jp/jinzai/camp/index.html)をご覧ください。
セキュリティキャンプでは割り込みまでを実装しました。(参考：[セキュリティキャンプ2017参加記 | PG_MANAの雑記](https://pg-mana.net/blog/seccamp_after/))

Methylenixという名前はメチレン基(Methylene)より採っています。 
有機化合物みたいにいろいろな部品を組み合わせて作っていくようにモジュールを組み合わせて応用的に作っていきたいと考え、
基の中で「nix」をくっつけて語呂が良さそうなメチレン基にしました。

## 現状

* APIC・GICv3によるデバイス割り込み
* メモリ・ページング動的管理
* タスク管理
* マルチコア対応
* ACPI Tableの部分的な解析とAML Interpreterによるシャットダウンボタンによるシャットダウン実装(一部未対応)
* フォント解析による簡易GUI
* NVM Expressからのデータ読み出し
* GPT及びFAT32/XFSからのディレクトリとファイル取得
* 簡易アプリケーション実行
* NIC対応及びTCP/IP通信及びSocket API提供(部分的)

## 方針
* GUIについては基本対応しない(デバイスの認識などはしておく、デバッグテキストを表示する程度)
* 複数のアーキテクチャに対応できるような作りにしたい
* OS自作入門レベルのこと(割り込み・音・マルチタスクなど)を当分の目標とする
* 動的に柔軟に対応できるようにする(UEFIなどから渡される情報をしっかり活用する)

## 対応命令セット
* x86_64
* AArch64

## ライセンス
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

## ビルド
### 環境整備
必要なソフトウェア

* make
* grub2-mkrescue
* rustc(nightly)
* cargo

### ビルド
### x86_64
```shell
git clone https://github.com/PG-MANA/Methylenix.git
cd Methylenix
make iso
# created bin/img/boot.iso
```

なおビルド済みのisoイメージは https://repo.taprix.org/pg_mana/methylenix/iso/ にあります。

### AArch64
```shell
git clone https://github.com/PG-MANA/Methylenix.git
cd Methylenix
make TARGET_ARCH=aarch64
# created bin/EFI/BOOT/
```

## 実行
### x86_64
qemu-system-x86_64が必要です。

```shell
qemu-system-x86_64 -cpu qemu64,+fsgsbase --cdrom bin/img/boot.iso

# or (OVMF)
qemu-system-x86_64 --cdrom bin/img/boot.iso -cpu qemu64,+fsgsbase -smp 2 -m 512M -bios /usr/bin/OVMF/OVMF.fd

# or (to emulate host cpu)
qemu-system-x86_64 --cdrom bin/img/boot.iso  -cpu host -smp 2 -m 512M -bios /usr/bin/OVMF/OVMF.fd --enable-kvm

# NIC and NVMe Emulation
qemu-system-x86_64 -drive if=pflash,format=raw,readonly=on,file=/path/to/OVMF_CODE.fd -drive if=pflash,format=raw,file=/path/to/QEMU_VARS.fd -m 1G -cdrom bin/img/boot.iso -smp 4 --enable-kvm -cpu host -netdev user,id=net0,hostfwd=tcp::7777-:8080 -device e1000e,netdev=net0,mac=52:54:00:12:34:56 -drive file=/path/to/img.qcow2,if=none,id=nvm -device nvme,serial=12345678,drive=nvm --boot order=d
```

### AArch64
qemu-system-aarch64とAArch64向けのOVMFが必要です。

```shell
# Modify "/usr/bin/OVMF/OVMF_AARCH64.fd" to your suitable path
qemu-system-aarch64 -m 1G -cpu a64fx -machine virt,gic-version=3 -smp 2 -nographic -bios /usr/bin/OVMF/OVMF_AARCH64.fd  -drive file=fat:rw:bin/,format=raw,media=disk
```

## ドキュメント

```shell
cargo doc --open 
```

## コーディング規約
基本は https://doc.rust-lang.org/1.1.0/style/style/naming/README.html に従ってください。
コード整形はrustfmtを使用します。  
(本人が守れてないかも)

## リンク
### 開発者のTwitterアカウント
  [https://twitter.com/PG_MANA_](https://twitter.com/PG_MANA_)
### 開発者のWebページ
  https://pg-mana.net
### 公式ページ
  https://methylenix.org (現在はGitHubへリダイレクトするだけ。いつできるかな。)

