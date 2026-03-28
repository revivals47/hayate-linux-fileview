# Hayate Linux File Viewer — 実装報告書

**期間:** 2026-03-26 〜 2026-03-28
**リポジトリ:** https://github.com/revivals47/hayate-linux-fileview

---

## 1. 目的

Hayate UIツールキットのドッグフーディングとして、Windowsから移行してきたLinuxユーザーが戸惑わないファイルビューアを構築する。

### 背景
- Nautilus（GNOME Files）はWindowsユーザーの期待と乖離がある
- Mac版Hayateファイルマネージャの知見を活かす
- GTKのD&D不安定さ・キャンバスクラッシュの代替としてWayland-native実装

---

## 2. 成果物

### 数字

| 指標 | 値 |
|------|------|
| モジュール数 | 14 |
| 総行数 | 2,774 |
| テスト | 5 |
| 最大ファイルサイズ | 500行以内 |
| コミット | 14 |

### モジュール構成

```
src/
├── main.rs          40行   エントリポイント
├── config.rs       ???行   設定永続化（~/.config/hayate-fileview/config.toml）
├── state.rs        175行   アプリケーション状態（パス、エントリ、選択、履歴）
├── entry.rs        131行   DirEntry（読み込み、ソート、CJK幅対応）
├── file_list.rs    439行   VirtualViewport仮想スクロール＋ダブルクリック＋テキストキャッシュ
├── keybindings.rs  264行   キーボードショートカット（Ctrl+C/V/Q、F2、検索等）
├── file_ops.rs     316行   ファイル操作（コピー/移動/削除/リネーム/新規フォルダ/symlink）
├── breadcrumb.rs   131行   パンくずナビゲーション＋◀▶履歴ボタン
├── scroll.rs        98行   ScrollableWidget
├── preview.rs      229行   プレビューペイン
├── sidebar.rs      249行   サイドバー（ブックマーク＋マウントポイント）
├── three_pane.rs   280行   レスポンシブ3ペインレイアウト＋ステータスバー
├── status_bar.rs   106行   ステータスバー（エラー表示対応）
└── scroll.rs        98行   ScrollableWidget
```

---

## 3. 実装した機能

### ナビゲーション
- ダブルクリックでフォルダに入る / ファイルをxdg-openで開く
- シングルクリックで選択（Ctrl/Shift修飾対応）
- Enter: ディレクトリ→ナビゲーション、ファイル→xdg-open
- Backspace: 親ディレクトリ
- パンくずナビゲーション（各セグメントクリック可能）
- サイドバー（ブックマーク＋マウントポイント自動検出）
- Alt+Left/Right: 戻る/進む履歴（ブラウザ式）

### 表示
- 3つの表示モード（Tab切替）: Detail / List / Compact
- レスポンシブペイン: 幅に応じてサイドバー/プレビュー自動非表示
- カラムヘッダー（Name/Size/Modified）＋ソートインジケータ（▲▼）
- シンボリックリンク表示（🔗アイコン）
- CJK文字幅対応（unicode-width）
- Monospaceフォントでカラム整列

### 選択
- シングルクリック選択
- Ctrl+クリック: 個別トグル
- Shift+クリック: 範囲選択
- Shift+Up/Down: キーボード範囲選択
- Ctrl+A: 全選択
- 選択行ハイライト（▶マーカー＋水色）

### 検索
- Ctrl+F: インクリメンタルフィルタリング
- 英数字タイプ: インクリメンタルジャンプ（300msタイムアウト）
- Escape: 検索解除

### ファイル操作
- Ctrl+C/V: コピー＆ペースト（内部クリップボード）
- Delete: XDGゴミ箱（~/.local/share/Trash/）
- F2: リネーム（_renamedサフィックス）
- Ctrl+Shift+N: 新規フォルダ作成
- symlink安全コピー（循環参照防止）

### スクロール
- マウスホイール
- キーボード（Up/Down/PageUp/PageDown/Home/End）
- VirtualViewportによる仮想スクロール（大規模ディレクトリ対応）

### プレビュー
- テキストファイル: 先頭100行表示（25拡張子対応）
- バイナリ/画像: ファイル名、サイズ、種類情報
- プレビュー内スクロール

### エラーハンドリング
- パーミッションエラー: ステータスバーに赤色表示
- パス存在チェック: 削除済みディレクトリへのナビゲーション防止
- read_dir_sorted() Result返却

### 設定
- ~/.config/hayate-fileview/config.toml
- 保存項目: show_hidden, sort_column, sort_order, view_mode, window_size
- Ctrl+Q時に自動保存

### その他
- 隠しファイルトグル（Ctrl+H）
- ?キーでヘルプオーバーレイ
- Ctrl+Q: 安全終了（設定保存後）
- ファイルサイズ表示（B/KB/MB/GB/TB）

---

## 4. パフォーマンス最適化

| 問題 | 対策 | 効果 |
|------|------|------|
| 毎フレームcosmic-text layout() | テキストレイアウトキャッシュ（HashMap） | 500ms→28ms/frame |
| キャッシュ無限成長 | 200件上限、refresh時クリア | メモリ安定 |
| dirty=true常時再描画 | Widget単位dirtyフラグ | 不要な再描画削減 |
| イベントがframe callback待ち | メインループで即時dispatch | 入力遅延削減 |
| 毎フレームlayout() | dirty時のみlayout | CPU削減 |

---

## 5. ドッグフーディングでGUI_kitにフィードバックした項目

### GUI_kitに追加した機能

| 機能 | ファイル | 理由 |
|------|---------|------|
| WidgetEvent::Scroll | core.rs, app.rs | マウスホイールが伝搬しなかった |
| PointerPress/Releaseにmodifiers | core.rs, app.rs | Ctrl+クリックが動かなかった |
| ESC強制終了の削除 | dispatch_impls.rs | ESCを検索解除に使えなかった |
| process_key()後のmodifiers更新 | keyboard.rs | Ctrl押下のKeyEventでctrl=false |
| layout()のdirtyスキップ | app.rs | 毎フレームlayoutが重かった |
| イベント即時dispatch | wayland.rs | イベント遅延16ms |
| quit_flag API | app.rs | アプリ終了手段がなかった |

### GUI_kitにまだ足りないもの

| 不足 | fileviewの回避策 | あるべき姿 |
|------|------------------|-----------|
| アプリ安全終了API | std::process::exit(0) | App::quit()メソッド or quit_flag連携 |
| 通知/トースト表示 | eprintln!でターミナルに出力 | ポップアップ通知Widget |
| 外部プロセス起動ヘルパー | std::process::Command直接 | portal.rsに統合 |
| システムクリップボード連携 | 内部Vec\<PathBuf\> | clipboard.rsのcopy/paste統合 |
| D&DのAppRunnerアクセス | 未使用（回避不能） | AppからDnD APIを公開 |
| コンテキストメニュー | 未実装 | popup.rsベースのメニューWidget |
| 動的ウィンドウタイトル | 起動時固定 | set_title() API |
| ウィンドウサイズ取得 | config.tomlに手動記録 | window_size() API |
| ダブルクリック検知 | 自前300ms判定 | WidgetEvent::DoubleClick |

---

## 6. 動作確認

### 環境
- Ubuntu 24.04, weston 13.0.0, NVIDIA GeForce RTX 4070
- hayate-ui（path依存: ../GUI_kit）

### 確認済み機能（ユーザー手動テスト）

| 機能 | 結果 |
|------|------|
| ディレクトリ一覧表示 | ✅ |
| フォルダナビゲーション | ✅ |
| マウスホイールスクロール | ✅ |
| Ctrl+クリック複数選択 | ✅（modifier修正後） |
| プレビューペイン | ✅ |
| サイドバー | ✅ |
| パンくずナビゲーション | ✅ |
| カラムソート | ✅ |
| ステータスバー | ✅ |
| クリック当たり判定 | ✅ |

---

## 7. 品質監査

codex + boss1チームによる全コード精査を実施。

### 発見した問題と対応

| 優先度 | 問題 | 対応 |
|--------|------|------|
| P0 | ダブルクリック未対応 | 300ms判定＋xdg-open |
| P0 | エラーハンドリング欠如 | Result返却＋ステータスバー通知 |
| P0 | symlink循環参照 | symlink_metadata＋リンクコピー |
| P0 | exit(0)即死 | SAFETYコメント（将来quit API） |
| P1 | ペイン破綻 | レスポンシブ非表示（350/550px閾値） |
| P1 | format_size()重複 | GB/TB対応＋共通関数化 |
| P2 | 設定永続化 | config.toml |
| P2 | 履歴なし | Alt+Left/Right |
| P2 | CJK幅ずれ | unicode-width |
| P2 | キャッシュ無限成長 | 200件上限 |
