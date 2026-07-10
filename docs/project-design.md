# prepo 設計書

## 1. 概要

prepo は、開発リポジトリが現在の環境で Ready かを診断する Rust 製 CLI ツールである。

~~~
preflight + repository = prepo
~~~

Nix、Devbox、mise などは必要なソフトウェア環境を用意する。一方で、起動済みプロセス、TCP ポート、外部サービス、デバイス、設定ファイル、プロジェクト固有コマンドの成功までは保証しない。prepo は環境を構築・修正せず、宣言済みの前提条件への適合を診断する。

Ready は「現在の環境で適用対象となる必須チェックがすべて Passed である」ことを意味する。プロジェクトが絶対的に実行可能である保証ではなく、未宣言の前提条件は判定対象外である。用語の正本は CONTEXT.md とする。

## 2. 基本情報

| 項目 | 値 |
| --- | --- |
| プロジェクト名 | prepo |
| crate 名 / CLI 名 | prepo |
| 設定ファイル | prepo.toml |
| 実装言語 | Rust 2024 Edition |
| MSRV | Rust 1.85 |
| ライセンス | MIT OR Apache-2.0 |

英語の短い説明文は “Preflight checks for development repositories.” とする。より詳しい説明文は “A declarative CLI for checking whether a development repository is ready to run in the current environment.” とする。

Cargo manifest のライセンス表記は MIT OR Apache-2.0 とし、LICENSE-MIT、LICENSE-APACHE、README、コントリビューション方針で同じデュアルライセンスを示す。

## 3. 目的と非目的

prepo は、開発環境で起きる「なぜ動かないのか分からない」を減らす。

- 新しい開発者の環境構築後に、宣言済みの前提を確認する。
- OS、プロセス、TCP ポート、HTTP、設定ファイル、環境変数を横断して診断する。
- 問題の安全な理由と修正案を表示し、CI、Issue、AI へ結果を共有できるようにする。
- Nix、Devbox、mise、Docker Compose、ビルドシステム、プロセス監視の代替にはならない。
- 利用者の環境を自動修正しない。

MVP の対象外は、パッケージ・ドライバーのインストールや更新、GUI、常駐監視、watch、リモート監視、設定継承、UDP・デバイス・Git・JSON・バージョンの各チェック、HTTP POST、リダイレクト、プロキシ、企業内 CA、外部プラグイン、公開ライブラリ API、自動更新、コード署名である。

report コマンド、出力ファイルへの直接保存、進捗、verbose、RUST_LOG、fail-fast、allow-all、force、設定自動移行も MVP に含めない。

## 4. CLI

MVP のコマンドは三つだけである。

~~~
prepo init

prepo validate [--config <PATH>]

prepo check
  [--config <PATH>]
  [--format <human|json|markdown>]
  [--sequential]
  [--no-color]
  [--allow-command]
  [--allow-network]
  [--allow-env]
  [--group <GROUP> | <ID>...]
~~~

- check の既定出力は human。
- config の相対パスは現在ディレクトリ基準。
- init は常に現在ディレクトリを対象にし、config を受け付けない。
- 実行制御、選択、許可フラグは check 専用。
- no-color は human 専用であり、JSON/Markdown との併用は CLI エラー。
- group と ID 列挙は相互排他。短縮オプションは設けない。

## 5. init と設定探索

init は現在ディレクトリへ prepo.toml を原子的に新規作成する。Git ルートへ移動しない。同名のファイル、ディレクトリ、シンボリックリンクがあれば作成せず、終了コード 2 を返す。上書き、マージ、自動推測、force/minimal/full オプションは提供しない。

生成物は project name と replace-me という必須 path チェック一件を持つ。利用者が実際のプロジェクトマーカーへ置き換えるまで、validate は成功し、check は意図的に Not Ready となる。Active Check は生成しない。

設定探索は次の通りである。

- config 指定時はそのファイルだけを使い、上方向探索をしない。
- 未指定時は現在ディレクトリから親へ進み、最初の prepo.toml を使う。
- Git リポジトリ内ではリポジトリルートを含め、そこを越えない。Git 外ではファイルシステムルートまで探索する。
- 複数設定があれば最も近いものが勝つ。
- 設定ファイルがシンボリックリンクでも読み込む。ただし Project Root と相対パスの基準は、リンク先ではなくリンク配置先の論理上の親ディレクトリとする。
- 設定が見つからない、読めない、UTF-8 TOML として解釈できない場合は終了コード 2。
- 通常出力、JSON、Markdown に探索経路、現在ディレクトリ、絶対パスを含めない。

設定全体は 1 MiB 以下とする。全量読込前にサイズを確認し、サイズ不明なら 1 MiB + 1 バイトで読込を打ち切る。

## 6. 設定スキーマ

version は必須の正整数であり、MVP は 1 だけを受け付ける。未指定、0、負数、未知の値は設定エラーとする。設定、出力、本体のバージョンは独立して管理し、同じ設定スキーマ番号の意味を変更しない。

~~~
version = 1

[project]
name = "arm-strike-game"

[runner]
parallel = true
max_concurrency = 4
default_timeout_seconds = 10

[[checks]]
id = "nix-shell"
type = "env"
name = "Nix development shell"
group = "environment"
variable = "IN_NIX_SHELL"
condition = "exists"
required = false
help = "Run nix develop before starting development."

[[checks]]
id = "unity-project"
type = "path"
name = "Unity project"
group = "unity"
path = "ProjectSettings/ProjectVersion.txt"
expected = "exists"
kind = "file"

[[checks]]
id = "bridge-process"
type = "process"
name = "Kinect Bridge process"
group = "bridge"
process = "KinectBridge.exe"
expected = "running"
platforms = ["windows"]

[[checks]]
id = "bridge-port"
type = "port"
name = "Kinect Bridge port"
group = "bridge"
host = "127.0.0.1"
port = 5005
expected = "open"
platforms = ["windows"]
depends_on = ["bridge-process"]
timeout_seconds = 3

[[checks]]
id = "development-server"
type = "http"
name = "Development server"
group = "network"
url = "http://localhost:3000/health"
method = "GET"
expected_status = 200
timeout_seconds = 3
required = false
~~~

### 6.1 共通フィールド

| フィールド | 必須 | 既定値 | 内容 |
| --- | ---: | --- | --- |
| id | はい | - | 一意な ASCII kebab-case ID |
| type | はい | - | env、path、command、process、port、http |
| name | はい | - | 安全な単一行の表示名 |
| group | いいえ | なし | ASCII kebab-case の分類 |
| required | いいえ | true | Ready への影響を決めるポリシー |
| depends_on | いいえ | 空配列 | 成功が必要な依存先 ID |
| platforms | いいえ | 全対応 OS | 適用条件 |
| help | いいえ | なし | 安全な単一行の修正案 |

id と group は ASCII kebab-case とする。name と help は Unicode を許可するが、空白のみ、改行、ASCII/C1 制御文字、ANSI ESC、Unicode 双方向上書き制御文字、前後の空白を拒否する。

RawConfig は入力表現、ValidatedConfig は既定値、正規化済みプラットフォーム、コンパイル済み正規表現、検証済み依存関係を持つ内部表現である。実行器は ValidatedConfig だけを受け取る。

### 6.2 種別固有フィールド

| 種別 | 固有フィールド |
| --- | --- |
| env | variable、condition、値条件時の expected |
| path | path、expected、expected が exists 時の kind |
| command | command、expected_exit_code、出力条件、working_directory、checks.env、timeout_seconds |
| process | process、expected |
| port | host、port、expected、timeout_seconds |
| http | url、method、expected_status、本文条件、checks.headers、timeout_seconds |

process.match と port.protocol は受け付けない。前者は常に完全一致、後者は常に TCP である。project.description は MVP に含めない。

### 6.3 validate

validate は副作用なしの静的検証である。ファイル存在、環境変数、DNS、プロセス、ポート、HTTP、コマンドの存在や許可フラグを確認しない。同じ設定内容ならどのマシンでも同じ結果になる。

検証対象は TOML 構文、型、未知フィールド、不適切なフィールド、必須値、ID 重複、自己・重複依存、存在しない依存先、循環、必須・任意の整合性、Applicability の含意、正規表現、URL、ヘッダー、ポート、パス構文、サイズ上限である。

- チェックは少なくとも一件、必須チェックは少なくとも一件必要。
- 必須チェックは直接・間接を問わず任意チェックへ依存してはならない。問題時は依存経路を表示する。
- 依存元が適用対象となる全環境で依存先も適用対象でなければならない。Linux、macOS、Windows の有限集合で多段依存まで検証する。
- 未知フィールド、不正な組合せ、空・重複・未知の platforms はエラー。
- 構文または型エラーでは即終了する。意味検証は独立したエラーを最大 50 件、チェック宣言順とフィールド順で収集する。
- 成功時は stdout に Configuration is valid. を一行出す。失敗時は stderr に安全な設定位置とエラーを出し、終了コード 2。

上限は、チェック 1,024 件、直接依存 128 件、ID/group 64 バイト、name 256 バイト、help 2,048 バイト、正規表現・部分一致パターン 4,096 バイト、パス/URL 8,192 バイト、コマンド引数 128 個かつ各 8,192 バイト、ヘッダー 64 個かつ各値 8,192 バイト、checks.env 128 項目かつ各値 8,192 バイトとする。

### 6.4 パス構文

検証はホスト OS に依存しない字句規則を使う。

- / から始まるものは POSIX 絶対パスであり、platforms は Linux/macOS の部分集合。
- ドライブ絶対パスと UNC パスは Windows 絶対パスであり、platforms は windows だけ。
- C:foo のドライブ相対パスと、単一バックスラッシュから始まるルート相対パスは曖昧なため拒否する。
- 相対パスは / を共通区切りにする。相対パス中のバックスラッシュは Windows 限定チェックだけで許可する。
- working_directory は全ての絶対形式を禁止する。
- .. は許可するが、validate はファイルシステムを参照しない。

## 7. チェック種別

### 7.1 env

condition は exists、not_exists、equals、not_equals、contains、matches である。

- exists は空値・非 Unicode 値を含め、変数が存在すれば Passed。not_exists は不在時だけ Passed。
- 値条件は変数が存在し Unicode として読める場合だけ評価する。未設定は Failed であり、not_equals でも成功にしない。
- 非 Unicode 値は Error / non_unicode_environment_value。
- 比較と部分一致は大文字・小文字を区別する。matches は Rust 正規表現の部分一致であり、完全一致には ^...$ を使う。
- 値条件は allow-env を必要とし、値は表示・保存しない。

### 7.2 path

expected は exists または missing。exists 時の kind は file、directory、any のいずれかで必須、missing 時は指定禁止である。

- 相対パスは Project Root 基準。
- exists はシンボリックリンクをたどり、リンク先の種別を比較する。
- 壊れたリンクは Failed / broken_symlink。
- missing はリンクを含むディレクトリエントリ自体がないときだけ Passed。壊れたリンクを missing 成功にしない。
- 権限不足、リンクループ、メタデータ取得不能は Error。
- リンクそのものを種別として検査する機能は MVP 外。

### 7.3 command

command は空でない引数配列であり、シェルを介さず先頭要素を実行する。利用者が明示的にシェル実行ファイルを指定することはできるが、prepo が暗黙にシェルを起動しない。

- 標準入力は閉じる。
- 作業ディレクトリ未指定時は Project Root、指定時は Project Root 基準の相対パス。
- 親環境は OS の子プロセス継承に任せ、checks.env のリテラル値で追加・上書きする。checks.env に環境変数展開はない。
- allow-command は環境継承、ファイル、ネットワークを含む包括的な信頼許可。
- 既定の期待終了コードは 0。終了コードと stdout_contains、stderr_not_contains を全て満たしたときだけ Passed。
- 出力条件は非 Unicode 出力にも対応するストリーミング部分一致で評価し、チャンク境界をまたぐ一致を検出する。空パターンは禁止。
- 実行ファイル不在・実行不能、作業ディレクトリ不正、タイムアウト、異常終了、終了コード不一致、出力条件不一致は Failed。
- 隔離失敗、観測上限超過、停止確認失敗、実行基盤の局所的異常は Error。

通常の子孫プロセスは Unix の専用プロセスグループ、Windows の Job Object に入れ、タイムアウト・観測上限・中断時に終了する。隔離を確立できなければ起動しない。意図的に隔離から脱出する敵対的コマンドは MVP の脅威モデル外であり、allow-command はサンドボックスではない。

### 7.4 process

process は OS が報告するプロセス名を大文字・小文字を区別して完全一致で確認する。

- 一件以上一致すれば running、一件もなければ stopped。
- expected は running または stopped。
- PID、実行パス、コマンドライン、ユーザー名は収集・出力しない。
- 一覧取得不能は Error。
- OS 差は platforms で分離する。部分一致は MVP 外。

### 7.5 port

TCP のみを扱う。

- open は TCP 接続確立で Passed。接続拒否・タイムアウトは Failed。DNS、経路、OS API の異常は Error。
- closed は明示的な接続拒否だけを Passed とする。接続成功は Failed、タイムアウト・到達不能は Error。
- available はローカル bind 成功後ただちに解放できたとき Passed、使用中なら Failed、権限不足等は Error。
- available の host は 127.0.0.1、::1、0.0.0.0、:: だけを許可する。
- available はポートを予約せず、検査時点のスナップショットである。

### 7.6 http

HTTP は Active Check であり allow-network を必要とする。

- URL は host を持つ絶対 http/https URL だけを許可する。userinfo、fragment、制御文字、URL 内の環境変数展開、自動 HTTP→HTTPS 昇格を禁止する。
- メソッドは GET と HEAD のみ。既定は GET。HEAD に本文条件を指定した場合は validate エラー。
- expected_status の既定値は 200、比較は完全一致。
- システム・環境変数プロキシを使わず、URL のホストを DNS 解決して直接接続する。リダイレクトは追跡しない。
- TLS は WebPKI の組み込みルートだけを使い、OS ルート、無効証明書、自己署名証明書、企業内 CA を使わない。
- タイムアウトは DNS、接続、TLS、ヘッダー、本文評価を含むリクエスト全体。
- ステータス不一致または本文条件なしでは本文を読まない。
- body_contains はストリーミング部分一致で評価する。identity と単一 gzip を許可し、複数 coding または gzip 以外は Error / unsupported_content_encoding。gzip 展開失敗は Error / body_decode_failed。
- DNS、接続拒否、TLS 検証、リクエスト全体のタイムアウト、ステータス不一致、本文不一致は Failed。本文読込・デコード・クライアント構築の局所的失敗と観測上限超過は Error。

ヘッダーの環境変数展開は値中の `${NAME}` だけであり、NAME は ASCII の環境変数名形式を満たす。URL 展開は行わない。展開には allow-env と allow-network の両方が必要である。変数未設定は Failed / missing_environment_variable、非 Unicode は Error、展開後に不正な値なら Error。

ヘッダー名は大文字・小文字を無視して重複検査する。Host、Content-Length、Transfer-Encoding、Connection、Proxy-Authorization、Accept-Encoding は設定禁止である。展開後の各値は 8,192 バイト、合計は 64 KiB を上限とし、超過時は送信せず Error / expanded_header_too_large とする。

## 8. 実行モデル

### 8.1 Full Run と Partial Run

引数なしの check は Full Run であり、現在の環境で適用対象となる全チェックを対象にする。

- 適用対象の必須チェックが全て Passed なら Ready。
- 適用対象の必須チェックに Failed、Error、PermissionDenied、Blocked が一件でもあれば Not Ready。
- 適用対象の必須チェックが一件もなければ Not Evaluated。終了コードは 0 だが Ready と表示しない。

ID 列挙または group は Partial Run である。全体の readiness を判定・出力しない。

- 選択した ID の和集合、または一つの group が Explicit Check。
- 依存先は再帰的に自動追加し、Dependency Check とする。明示選択も依存先である場合は explicit を優先する。
- 不明 ID、空 group、重複 ID、ID と group の併用は終了コード 4 であり、何も実行しない。
- 全ての Explicit Check が Not Applicable なら Selected run: Not Evaluated として終了コード 0。
- それ以外は実行範囲の必須チェックから Selected run: Passed | Failed を導出する。

### 8.2 依存関係、並列実行、タイムアウト

依存先が全て Passed のときだけ依存元を実行する。Failed、Error、PermissionDenied、Blocked の依存先が一つでもあれば依存元は Blocked となる。

- blocked_by は直接の阻害依存先だけを宣言順で保持する。
- Blocked は実行していないため duration_ms は 0。
- Optional Check も依存先が成功しなければ Blocked。
- 有効な設定で、適用対象の依存元だけが Not Applicable にはならない。

既定は並列であり、max_concurrency の既定値は 4、許容範囲は 1..=64。runner.parallel = false または sequential で逐次実行し、CLI 指定が優先する。依存関係が解決したチェックは設定の宣言順で起動し、結果、group、JSON、Markdown は完了順ではなく宣言順で出す。

個別 timeout は command、port、http だけが指定できる。既定は runner.default_timeout_seconds = 10、範囲は 1..=3600 秒。timeout は実行開始から測り、timeout 自体は Failed / timed_out、後処理失敗は Error。env、path、process の timeout 指定は validate エラー。fail-fast は実装しない。

### 8.3 中断

check は Ctrl-C を、Unix では SIGTERM も処理する。最初のシグナルでは新規起動を止め、HTTP/TCP を中断し、管理中の command ツリーを終了して最大 5 秒待つ。不完全な結果は stdout に出さず、stderr に安全な Interrupted. を出す。Ctrl-C は 130、SIGTERM は 143。二回目のシグナルは同期的な best-effort 終了後ただちに終了する。

## 9. Active Check と秘密情報

リポジトリ設定は既定で未信頼とする。validate は常に副作用なしであり、Active Check は実行ごとの明示許可が必要である。

| 操作 | 必要な許可 |
| --- | --- |
| path、process、環境変数の存在確認 | なし |
| 環境変数の値比較 | allow-env |
| port、環境変数を使わない http | allow-network |
| 環境変数を展開する HTTP ヘッダー | allow-network と allow-env |
| command | allow-command |

不足許可は PermissionDenied / permission_missing であり、missing_permissions に全ての不足フラグを入れ、human と Markdown の問題詳細にも表示する。必須なら Not Ready と終了コード 1、任意なら Warning と終了コード 0。allow-all は提供しない。

環境変数は全量を保持しない。env と HTTP 展開だけが必要な変数を必要な時点で読む。command の環境継承は OS へ任せる。値、HTTP ヘッダー・本文・Cookie、command stdout/stderr、アクセストークン、パスワード、秘密鍵、不要な絶対パスは CheckResult、human、JSON、Markdown、エラーのどれにも保持・出力しない。

観測データは判定中だけ保持し直後に破棄する。command の stdout/stderr は各 8 MiB、HTTP 本文は展開後 1 MiB を固定上限とする。上限超過は中断し、通常の CheckResult として Error / observation_limit_exceeded を返す。

## 10. Outcome、表示、終了コード

Check Outcome は客観的な結果であり、required や表示状態を含まない。

~~~
Passed
Failed
Error
PermissionDenied
NotApplicable
Blocked
~~~

Error はチェック単位の判定不能であり、他のチェックは継続する。Prepo Error は診断全体を信頼して継続できない内部障害であり、Check Outcome ではない。必須の Failed/Error/PermissionDenied/Blocked は表示上 Failed、任意の同結果は Warning となる。

reason は閉じた機械可読 enum である。Rust では PascalCase、JSON では snake_case を使う。Passed の reason は null、他は非 null。例は permission_missing、platform_not_applicable、dependency_not_passed、non_unicode_environment_value、broken_symlink、timed_out、observation_limit_exceeded、unsupported_content_encoding、expanded_header_too_large である。OS やライブラリの生エラー文を reason に入れない。同じ output_schema_version 内で既存 reason の意味を変更・削除しない。

| 終了コード | 意味 |
| ---: | --- |
| 0 | Ready、Not Evaluated、または任意チェックの Warning のみ |
| 1 | 実行範囲の適用対象必須チェックが Passed 以外 |
| 2 | CLI 構文、設定探索・読込・validate、init 作成失敗 |
| 3 | Prepo Error、対応外 OS、出力のシリアライズ・書込失敗 |
| 4 | 指定 ID または group が存在しない |
| 130 | Ctrl-C |
| 143 | Unix SIGTERM |

help/version は 0。stdout の Broken Pipe だけは静かな 0 とし、その他の出力失敗は 3 とする。

## 11. 出力

### 11.1 Human

MVP は英語表示のみ。診断結果は終了コード 1 の場合も stdout へ出す。設定エラー、Prepo Error、中断通知は stderr へ出す。

ライブ進捗は出さず、完了後に宣言順で出す。TTY でのみ色を自動有効化し、非 TTY、NO_COLOR、no-color では無効にする。色だけへ依存せず、記号、表示状態、Outcome、reason を併記する。Passed は ✓、必須の問題は ✗、任意の問題は △、Not Applicable は - とする。Full Run は Readiness: Ready | Not Ready | Not Evaluated、Partial Run は Selected run: Passed | Failed | Not Evaluated を最後に表示する。

### 11.2 JSON

JSON は stdout に JSON だけを出す。表示上の Warning/Failed を保存せず、outcome と required から導出する。

トップレベルは output_schema_version、config_schema_version、prepo_version、run_kind、Full Run 時だけの readiness、project.name、environment.os、environment.arch、total_duration_ms、summary、results を持つ。

summary は total_count、applicable_count、evaluated_count、evaluated_required_count と各 Outcome の件数を持つ。evaluated_count は Passed、Failed、Error だけを数える。各 Outcome 件数の合計は total_count である。

各結果は id、name、group、required、outcome、reason、安全な summary、help、duration_ms、blocked_by、missing_permissions を持つ。Partial Run のみ selection: explicit | dependency を持ち、Full Run では selection を省略する。Passed の reason は null、blocked_by と missing_permissions は該当しない場合も空配列である。日時、タイムゾーン、ホスト名、ユーザー名、現在ディレクトリ、絶対パスを含めない。

### 11.3 Markdown

check の Markdown 出力は stdout に Markdown だけを出す。見出しは prepo check とし、Full Run は Readiness、Partial Run は Selected run を示す。Project、OS、Architecture、prepo version、総実行時間を表示する。

結果表は宣言順で Display | Outcome | Check | Reason | Duration を持ち、Partial Run だけ Selection 列を追加する。問題のあるチェックの help、blocked_by、missing_permissions は表の後に表示する。設定由来の文字列は表、見出し、詳細の全てで Markdown エスケープする。ANSI、生データ、絶対パス、日時を含めない。

## 12. プラットフォーム

MVP の対応 OS は linux、macos、windows だけである。platforms 未指定は三つ全てであり、内部と JSON では linux、macos、windows の順へ正規化する。対応外 OS では診断を開始せず、stderr へ安全な説明を出して終了コード 3。

OS は実行バイナリから得て、環境変数で上書きしない。将来アーキテクチャ等を追加しても、依存元の適用条件が成立する全環境で依存先も成立するという含意規則を維持する。

## 13. 実装構造と依存関係

MVP はバイナリ専用 crate とし、autolib = false を設定する。公開ライブラリ API は作らない。

- CLI、アプリケーション制御、設定読込、RawConfig/ValidatedConfig、結果モデル、実行計画・scheduler・readiness、各チェック、出力 renderer/sanitizer、OS 固有プロセス隔離を分離する。
- 各チェック種別はタグ付き enum で分岐する。trait object、async-trait、外部プラグイン API は使わない。
- checks は客観的 Outcome だけを返す。required による表示と終了コードは engine が導出し、output は状態を変更しない。
- EnvironmentReader のような小さな境界で必要な環境変数読込を差し替え可能にする。

通常依存は clap、serde、toml、serde_json、tokio、reqwest、sysinfo、thiserror、regex、console とする。reqwest は既定機能を切り、Rustls、WebPKI roots、gzip だけを有効にする。Unix の安全なプロセスグループ操作と Windows Job Object 用の依存は target-specific にする。開発依存は tempfile と assert_cmd。petgraph、predicates、futures-util、async-trait は使わない。

依存バージョンは実装開始時に最新資料と MSRV 1.85 を確認して固定する。Cargo.lock はコミットし、CI は Rust 1.85 と最新 stable の両方を使う。

## 14. テスト方針

最上位のテスト境界は CLI である。テストは実装内部ではなく、設定入力、stdout/stderr、終了コード、JSON/Markdown 契約、外部に見える副作用を確認する。このリポジトリには既存実装のテスト慣行がないため、以下を基準にする。

- 単体テストで RawConfig→ValidatedConfig、全 validation 規則、依存グラフ、Ready 三状態、終了コード導出、ストリーミング照合、出力 sanitization を確認する。
- CLI 統合テストで三コマンド、全終了コード、選択、stdout/stderr 分離、JSON 混入なしを確認する。
- HTTP/TCP はテスト内ローカルサーバーだけを使い、外部サービスへ依存しない。
- Active Check の許可行列、環境値非保持、gzip/identity、観測上限、timeout、中断、Broken Pipe を確認する。
- command は stdin 閉鎖、環境継承、出力上限、timeout、通常の子孫プロセス終了を各 OS で確認する。
- path はファイル、ディレクトリ、リンク、壊れたリンク、権限エラーを確認する。
- Golden JSON/Markdown は duration を正規化し、順序、フィールド省略、エスケープを確認する。
- CI は Ubuntu、macOS、Windows の各 OS で MSRV と最新 stable をテストし、stable Ubuntu では format と警告をエラー化した Clippy を実行する。

## 15. 配布

crates.io では cargo install prepo を提供する。GitHub Releases は Git タグだけを起点に作り、通常 CI は公開しない。

MVP の配布対象は x86_64/aarch64 Linux musl、x86_64/aarch64 macOS、x86_64 Windows MSVC である。Linux は musl と Rustls を使い、OpenSSL 等の動的依存を避ける。各アーカイブにはバイナリ、README、両ライセンス、SHA-256 チェックサムを含める。Windows ARM64、Homebrew/Scoop/winget、インストールスクリプト、コード署名は将来検討する。

## 16. MVP の完成条件と実装順序

MVP は六つのチェック種別、三コマンド、human/JSON/Markdown、許可境界、静的 validate、依存関係、決定的な並列実行、三 OS 対応を備える。

実装は次の順で進める。

1. バイナリ crate、CLI、設定探索、サイズ制限、RawConfig/ValidatedConfig、静的 validate。
2. Outcome、reason、readiness、終了コード、human/JSON/Markdown の安全な出力。
3. env/path、選択計画、依存関係、Applicability、決定的 scheduler。
4. command とプロセス隔離・中断・観測上限。
5. process、port、http、許可行列、WebPKI/gzip。
6. クロスプラットフォーム統合テスト、CI、配布設定、README とライセンス。

次の設定が validate に成功し、必須の Git directory が存在するなら Ready、任意チェックだけに問題があるなら終了コード 0、必須チェックに問題があれば 1 を返すことを完成条件とする。

~~~
version = 1

[project]
name = "example"

[[checks]]
id = "nix-shell"
type = "env"
name = "Nix development shell"
variable = "IN_NIX_SHELL"
condition = "exists"
required = false

[[checks]]
id = "git-directory"
type = "path"
name = "Git directory"
path = ".git"
expected = "exists"
kind = "any"

[[checks]]
id = "development-server"
type = "http"
name = "Development server"
url = "http://localhost:3000"
expected_status = 200
timeout_seconds = 3
required = false
~~~

HTTP を実行する例では allow-network が必要である。

## 17. 将来的な拡張と Arm Strike

将来の独立仕様として、UDP 受信、デバイス、Git、JSON 値、コマンドバージョン、watch、CI モード、設定継承、安全な自動修正、プロキシ、企業内 CA、追加 CA、追加 Content-Encoding、HTTP POST、report、fail-fast、構造化ログ、進捗、外部プラグイン、公開ライブラリ API を検討する。

Arm Strike では、Windows 向けのプロセス、TCP ポート、HTTP、将来の Kinect デバイス・UDP 受信チェックを group と依存関係で組み合わせる。UDP・デバイスは MVP 外であるため、MVP の設定例へ混ぜない。

## 18. ADR との対応

- ADR-0001: Check Outcome と required、表示、終了コードを分離する。
- ADR-0002: Active Check を明示許可制にし、HTTP の安全な通信境界を定める。
- ADR-0003: 生の観測データを保持・出力せず、固定観測上限を使う。
- ADR-0004: command の通常の子孫を OS 固有コンテナで終了し、中断時も後処理する。
- ADR-0005: 適用対象必須チェックがない Full Run を Not Evaluated とする。
