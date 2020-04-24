# Methylenix
(Format:UTF-8)  
The operating system written in Rust

## 概要
Methylenix とはRustで構成されたOSです。
一番最初の初期化時のアセンブリを除き、すべてRustで記述されています。
ドキュメントは追々整備します。

## Methylenixとは
このプログラムの原点は、セキュリティキャンプ全国大会2017 集中コース「X　言語やOSを自作しよう」において、書きはじめたものです。  
セキュリティキャンプについては、[セキュリティ・キャンプ：IPA 独立行政法人 情報処理推進機構](https://www.ipa.go.jp/jinzai/camp/index.html)をご覧ください。
セキュリティキャンプでは割り込みまでを実装しました。(参考：[セキュリティキャンプ2017参加記](https://pg-mana.net/blog/seccamp_after/))

Methylenixという名前は(頭弱い)自分が唐突に思いついたアイデアです。  
有機化合物みたいにいろいろな部品を組み合わせて作っていくようにモジュールを組み合わせて応用的に作っていきたいと考え、基の中で「nix」をくっつけてゴロが良さそうなメチレン基にしました。
なんか重大な間違いを起こしてそう。

## 現状

* APICによるデバイス割り込み
* メモリ・ページング動的管理

## 方針
* GUIについては基本対応しない(デバイスの認識などはしておく)
* UEFIをなるべく利用したい...?(GRUBを駆使して)
* OS自作入門レベルのこと(割り込み・音・マルチタスクなど)を当分の目標とする
* 動的に柔軟に対応できるようにする(UEFIなどから渡される情報をしっかり活用する)

## 対応命令セット
* x86_64

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
* ld
* grub2-mkrescue
* rustc(nightly)
* cargo
* cargo-xbuild

### ビルド

```shell
git clone https://github.com/PG-MANA/Methylenix.git
cd Methylenix
make iso
#これでbin/img/boot.isoができる...はず
make clean
#objファイル削除
```

なおビルド済みのisoイメージは https://repo.taprix.org/pg_mana/methylenix/iso/ にあります。

## 実行

qemu-system-x86_64が必要です。
```
qemu-system-x86_64 --cdrom bin/img/boot.iso
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
### OS Wiki
  https://soft.taprix.org/wiki/oswiki/
