# Methylenix
(Format:UTF-8)  
Rustで書かれたOS

## 概要
Methylenix とはRustで構成されたOSです。
一番最初の初期化時のアセンブリを除き、すべてRustで記述されています。
ドキュメントは追々整備します。

## Methylenixとは
このプログラムの原点は、セキュリティキャンプ全国大会2018 集中コース「X　言語やOSを自作しよう」において、書きはじめたものです。  
セキュリティキャンプについては、[https://www.ipa.go.jp/jinzai/camp/index.html](セキュリティ・キャンプ：IPA 独立行政法人 情報処理推進機構)をご覧ください。
セキュリティキャンプでは割り込みまでを実装しました。(参考：[https://pg-mana.net/blog/seccamp_after/](セキュリティキャンプ2017参加記))

Methylenixという名前は(頭弱い)自分が唐突に思いついたアイデアです。  
有機化合物みたいにいろいろな部品を組み合わせて作っていくようにモジュールを組み合わせて応用的に作っていきたいと考え、基の中で「nix」をくっつけてゴロが良さそうなメチレン基にしました。
なんか重大な間違いを起こしてそう。

## 対応命令セット
* x86_64

## 更新
**早速ですが、受験があり、一年はろくに更新できず、また現在のコードも不安定な部分が多いため、「ふーん、こんなんがあるんや。」程度に見てもらえると嬉しいです。**
順調に行けば2019年4月に戻ってきます。

## ライセンス
Copyright 2018 PG_MANA  
This software is Licensed under the Apache License Version 2.0  
See LICENSE.md  

## ビルド
### 環境整備
必要なソフトウェア

* make
* ld
* grub2-mkrescue
* nasm
* rustc(nightly)
* cargo
* xargo

詳しくは https://soft.taprix.org/wiki/oswiki/Rust:setup を参照してください。

### ビルド

```shell
make iso
#これでbin/img/boot.isoができる...はず
make clean
#objファイル削除
```

## コーディング規約
基本は https://doc.rust-lang.org/1.1.0/style/README.html に従ってください。
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
