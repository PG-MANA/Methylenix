# Methylenix
(Format:UTF-8)  
Rustで書かれたOS

## 概要
Methylenix とはアセンブリとRustで構成されたOSです。
ドキュメントは追々整備します。
なおこのコードはセキュリティキャンプ全国大会2018で書いたものです。

## Methylenixの由来
(頭弱い)自分が唐突に思いついたアイデアです。  
有機化合物みたいにいろいろな部品を組み合わせて作っていくようにモジュールを組み合わせて応用的に作っていきたいと考え、基の中で「nix」をくっつけてゴロが良さそうなメチレン基にしました。  
...なんか重大な間違いを起こしてそう...

## 更新
早速ですが、受験があり、一年はろくに更新できず、また現在のコードも不安定な部分が多いため、「ふーん、こんなんがあるんや。」程度に見てもらえると嬉しいです。  
いつか更新したい。

## ライセンス
Copyright 2018 PG_MANA  
This software is Licensed under the Apache License Version 2.0  
See LICENSE.md  

## ビルド
### 環境整備
必要なソフトウェア
* MAKE
* Ld
* Grub2-mkrescue
* NASM
* Rust(nightly)
* Cargo
* Xargo
詳しくは https://soft.taprix.org/wiki/oswiki/Rust:setup を参照してください。

### ビルド

```shell
make iso
#これでbin/img/boot.isoができる...はず
make clean
#objファイル削除
```

## リンク
### 開発者のTwitterアカウント
  [https://twitter.com/PG_MANA_](https://twitter.com/PG_MANA_)
### 開発者のWebページ
  https://pg-mana.net
