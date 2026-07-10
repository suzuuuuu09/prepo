# prepo

`prepo`が扱う、開発リポジトリの実行準備状態と診断結果に関する共通語彙。

## Language

**Ready**:
現在の環境で適用対象となる必須チェックが、すべて成功している状態。プロジェクトの絶対的な実行可能性や、未宣言の前提条件の充足は意味しない。
_Avoid_: 実行可能、完全に準備済み、動作保証

**Not Ready**:
適用対象となる必須チェックに、Failed、Error、PermissionDenied、またはBlockedが一つ以上ある状態。
_Avoid_: 動作不能、壊れている

**Not Evaluated（未判定）**:
Full Runで適用対象となる必須チェックが一つもなく、Readyを確認する証拠が得られなかった状態。終了コードは0だが、Readyとはみなさない。
_Avoid_: Ready、Not Ready、成功

**Check Outcome（チェック結果）**:
チェックの客観的な評価結果。Passed、Failed、Error、PermissionDenied、Not Applicable、Blockedのいずれかであり、必須か任意かという判定ポリシーや表示上の重要度を含まない。
_Avoid_: 表示ステータス、重要度

**Error（判定不能）**:
実行を許可されたチェックが、OSエラーなどの局所的な問題によって条件を評価できなかった結果。`prepo`自体の障害とは区別し、ほかのチェックの評価は継続する。
_Avoid_: Failed、内部エラー、クラッシュ

**Prepo Error（内部エラー）**:
診断全体を信頼して継続できなくなる、`prepo`自体の障害。Check Outcomeではなく、診断を終了して終了コード3を返す。
_Avoid_: Error、Failed、判定不能

**PermissionDenied（実行未許可）**:
Active Checkに必要な明示的許可が与えられなかったため、チェックを実行しなかった結果。必須チェックではNot Ready、任意チェックではReady判定に影響しない。
_Avoid_: Skipped、Error、OSの権限エラー

**Active Check（能動チェック）**:
外部コマンドの実行、ネットワーク通信、または環境変数値の展開など、利用者の環境へ作用したり値を外部へ露出したりし得るチェック。実行ごとの明示的な許可を必要とする。
_Avoid_: 通常チェック、安全なチェック

**Passive Check（受動チェック）**:
外部コマンド、ネットワーク通信、環境変数値の展開を行わず、現在の環境を読み取るだけのチェック。Active Checkの許可を必要としない。
_Avoid_: 無害、信頼済みチェック

**Applicable Check（適用対象チェック）**:
現在の実行環境が、宣言された適用条件を満たし、Full Runで評価対象になるチェック。Required CheckであればReadyの判定対象になる。
_Avoid_: 実行可能チェック

**Applicability（適用条件）**:
チェックがどの実行環境で評価対象になるかを定める条件。依存元の適用条件が成立するすべての環境で、依存先の適用条件も成立しなければならない。
_Avoid_: Platform、実行条件

**Required Check（必須チェック）**:
適用対象である場合、ReadyになるためにCheck OutcomeがPassedである必要があるチェック。直接・間接を問わず、任意チェックへ依存してはならない。
_Avoid_: 重要チェック、エラーチェック

**Optional Check（任意チェック）**:
失敗してもReady判定には影響しないチェック。必須チェックの直接または間接の依存先にはできない。
_Avoid_: 警告チェック、不要なチェック

**Not Applicable（対象外）**:
現在の実行環境が宣言された適用条件を満たさないため、Readyの判定から除外された状態。
_Avoid_: Skipped、成功、失敗

**Blocked（依存関係による未実行）**:
依存先が成功しなかったため、チェックを実行できず、結果を確認できていない状態。必須チェックがBlockedの場合はNot Readyとなる。
_Avoid_: Skipped、対象外、成功

**Full Run（全体実行）**:
現在の環境で適用対象となる全チェックを対象にした診断実行。適用対象の必須チェック数に応じて、プロジェクト全体をReady、Not Ready、またはNot Evaluatedと判定する。
_Avoid_: 通常実行

**Partial Run（部分実行）**:
IDやグループで選択された一部のチェックだけを対象にした診断実行。選択範囲の成否は示すが、プロジェクト全体のReady状態は判定しない。
_Avoid_: Ready、Not Ready、全体診断

**Explicit Check（明示選択チェック）**:
IDまたはグループによって利用者が直接選択した、Partial Runの起点となるチェック。
_Avoid_: Root Check、手動チェック

**Dependency Check（依存追加チェック）**:
明示選択チェックを評価するため、依存関係をたどってPartial Runへ自動的に追加されたチェック。
_Avoid_: Explicit Check、暗黙チェック
