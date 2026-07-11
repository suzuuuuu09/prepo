# prepo

`prepo` は、開発リポジトリが現在の環境で実行可能な状態かを確認する Rust 製 CLI です。

[English](./README.md)

## できること

- 宣言的な `prepo.toml` の設定を検証する
- ファイル、環境変数、プロセス、ポート、HTTP エンドポイント、コマンドを確認する
- 依存関係を考慮しながら、独立したチェックを並列実行する
- 人間向け、JSON、Markdown 形式で結果を出力する
- 依存関係のインストールや環境の変更は行わない

## インストール

### ソースから

```bash
git clone https://github.com/suzuuuuu09/prepo.git
cd prepo
cargo install --path .
```

## ビルド

```bash
cargo build
cargo test
```

## 使い方

```bash
cargo run -- init
cargo run -- validate
cargo run -- check
cargo run -- check <id>...
cargo run -- check --group <group>
cargo run -- check --format json
```

インストール済みなら、`cargo run --` の代わりに `prepo` を使えます。

### コマンド

- `init`: 現在のディレクトリに `prepo.toml` のひな形を作成
- `validate`: チェックを実行せずに設定を検証
- `check`: すべてのチェック、指定した ID、またはグループを実行

## 設定ファイル

チェックは `prepo.toml` に定義します。

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

`--config` を指定しない場合は、現在のディレクトリから親ディレクトリへ向かって `prepo.toml` を探索します。

## チェック種別

- `env`: 環境変数の存在や条件を確認
- `path`: ファイルやディレクトリの存在を確認
- `command`: コマンドを実行し、終了コードや出力を確認
- `process`: プロセスが起動中または停止中か確認
- `port`: TCP ポートが開いている、閉じている、または利用可能か確認
- `http`: HTTP エンドポイントとレスポンスステータスを確認

各チェックでは、`required`、`group`、`depends_on`、`platforms`、`timeout_seconds`、`help` も指定できます。

## 許可

能動的なチェックには、明示的な許可フラグが必要です。

```bash
prepo check --allow-command
prepo check --allow-network
prepo check --allow-env
```

- `--allow-command`: 設定されたコマンドチェックを許可
- `--allow-network`: ポートチェックと HTTP チェックを許可
- `--allow-env`: 環境変数の値比較と HTTP ヘッダー内の環境変数展開を許可

`--allow-command` を指定すると、設定されたコマンドは現在の環境変数を引き継ぎ、ファイルシステムやネットワークへアクセスできます。信頼できる設定でのみ使用してください。

## 出力形式

```bash
prepo check --format human
prepo check --format json
prepo check --format markdown
```

`--sequential` で並列実行を無効化し、`--no-color` で人間向け出力の色を無効化できます。

## ライセンス

[Apache License 2.0](./LICENSE) の下で公開します。
