# prepo

[English](./README.md) | [日本語](./README.ja.md)

> 開発リポジトリのためのプリフライトチェック。

`prepo` は、開発リポジトリが現在の環境で実行可能な状態かを確認する Rust 製 CLI ツールです。

依存関係のインストールや環境の変更は行わず、ファイル、環境変数、プロセス、ポート、HTTP エンドポイント、プロジェクト固有のコマンドを診断します。

## 使い方

```sh
prepo init
prepo validate
prepo check
```

特定のチェックやグループだけを実行できます。

```sh
prepo check development-server
prepo check --group network
```

能動的なチェックには明示的な許可が必要です。

```sh
prepo check --allow-command
prepo check --allow-network
prepo check --allow-env
```

`--allow-command` を指定すると、設定されたコマンドは現在の環境変数を引き継ぎ、ファイルシステムやネットワークへアクセスできます。信頼できる設定でのみ使用してください。

## 設定

リポジトリに `prepo.toml` を作成します。

```toml
version = 1

[project]
name = "example"

[[checks]]
id = "git-directory"
type = "path"
name = "Git repository"
path = ".git"
kind = "directory"

[[checks]]
id = "development-server"
type = "http"
name = "Development server"
url = "http://localhost:3000/health"
expected_status = 200
required = false
```

対応するチェック種別：

```text
env, path, command, process, port, http
```

## 出力

```sh
prepo check
prepo check --format json
prepo check --format markdown
```

## 開発

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

## ライセンス

[Apache License 2.0](./LICENSE) の下で公開します。
