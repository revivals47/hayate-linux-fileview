# Hayate Linux FileView — 実装報告書

**期間:** 2026-03-26 〜 2026-03-28
**最終更新:** 2026-03-28
**状態:** 機能実装完了、hayate-ui新API統合待ち

---

## 概要

hayate-ui（Wayland-native GUIツールキット）のドッグフーディングとして開発された
Linuxファイルマネージャ。ゼロから2,774行のファイルマネージャを構築。

---

## モジュール構成（14ファイル, 2,774行）

| ファイル | 行数 | 責務 |
|---------|------|------|
| main.rs | 47 | エントリポイント、Config読込、App起動 |
| config.rs | 120 | 設定永続化（手書きtomlパーサー） |
| state.rs | 225 | アプリ状態、選択、履歴、検索フィルタ |
| entry.rs | 153 | DirEntry、ソート、format_size、CJK文字幅 |
| file_list.rs | 470 | VirtualViewport仮想化リスト、直接TextEngine描画 |
| file_ops.rs | 323 | コピー/移動/ゴミ箱/リネーム/mkdir + symlink |
| keybindings.rs | 289 | 全キーボードショートカット |
| scroll.rs | 98 | 汎用ScrollableWidget |
| sidebar.rs | 249 | ブックマーク + /proc/mountsマウントポイント |
| breadcrumb.rs | 164 | パンくずナビ + ◀▶ 履歴ボタン |
| preview.rs | 220 | ファイルプレビュー + スクロール |
| status_bar.rs | 112 | ステータスバー + エラー表示 |
| three_pane.rs | 304 | レスポンシブ3ペインレイアウト |

全ファイル500行以内。

---

## キーボードショートカット

| キー | 動作 |
|------|------|
| Up/Down | カーソル移動 + 選択 |
| Shift+Up/Down | 範囲選択拡張 |
| Enter | ディレクトリ移動 / ファイルをxdg-openで開く |
| Backspace | 親ディレクトリ |
| Alt+Left/Right | 戻る/進む履歴 |
| Tab | 表示モード切替 (Detail→List→Compact) |
| Ctrl+H | 隠しファイルトグル |
| Ctrl+F | 検索モード（インクリメンタルフィルタ） |
| Ctrl+A | 全選択 |
| Ctrl+C | 選択ファイルをコピー（内部バッファ） |
| Ctrl+V | ペースト（コピーしたファイルを現在地にコピー） |
| Delete | ゴミ箱に移動（XDG Trash仕様） |
| F2 | リネーム（簡易版: _renamedサフィックス） |
| Ctrl+Shift+N | 新規フォルダ作成 |
| Ctrl+1/2/3 | Name/Size/Modifiedでソート |
| Ctrl+Q | 設定保存して終了 |
| ? | ショートカットヘルプ（stderr出力） |
| 英数字 | インクリメンタルジャンプ（300msタイムアウト） |
| PageUp/Down | ページスクロール |
| Home/End | 先頭/末尾へ |
| Esc | 検索モード終了 / フィルタクリア |

---

## 残課題（優先度順）

### P1: hayate-ui新APIの統合（最優先）

hayate-uiに以下のAPIが追加済み。fileview側に統合すれば大幅にUX改善。

| hayate-ui API | fileviewでの用途 | 現状 |
|---------------|-----------------|------|
| `WidgetEvent::DoubleClick` | ダブルクリック判定 | 自前タイマー（置換可能） |
| `WidgetEvent::FileDrop` | 外部からファイルドロップ | 未統合 |
| `App::quit_flag()` | 安全終了 | exit(0)使用中 |
| `App::title_buffer()` | 動的タイトル更新 | 起動時固定 |
| `App::window_size()` | config保存時サイズ取得 | 未統合 |
| `App::clipboard_copy_buffer()` | システムクリップボード | 内部バッファのみ |
| `ContextMenu` | 右クリックメニュー | 未実装 |
| `ToastWidget` | 操作結果通知 | eprintln使用中 |
| `portal::open_with_default()` | xdg-open統一API | 自前Command::spawn |

**統合作業の見積もり:** 各API 30分程度、全体で半日。

### P2: 機能改善

- [ ] F2インラインリネーム（TextInput統合）
- [ ] Ctrl+Lアドレスバー直接入力
- [ ] ペイン間フォーカス移動（F6/Ctrl+Tab）
- [ ] テスト強化（現在5件→30件目標）
- [ ] ファイルサイズのディスク使用量表示

### P3: 将来機能

- [ ] タブ対応（複数ディレクトリ同時表示）
- [ ] inotifyファイル監視（自動リフレッシュ）
- [ ] 画像プレビュー（tiny-skia描画）
- [ ] ターミナル統合（F12トグル）
- [ ] D&Dドラッグ開始（hayate-ui側スタブ完成待ち）

---

## 設定ファイル

`~/.config/hayate-fileview/config.toml`

```toml
show_hidden = false
sort_column = "name"
sort_order = "asc"
view_mode = "detail"
window_width = 750
window_height = 450
```

Ctrl+Q時に自動保存。起動時に自動読込。

---

## パフォーマンス

- VirtualViewport仮想化: visible_rangeのみ描画（O(visible), not O(N)）
- TextEngine layout キャッシュ: cosmic-text Bufferを200件キャッシュ
- PixelScrollPhysics: 慣性スクロール（フレームレート非依存）
- テキストレイアウトキャッシュ導入で 500ms/frame → 28ms/frame に改善

---

## ビルド手順

```bash
cd /home/ken/Documents/hayate-linux-fileview
cargo build          # ビルド
cargo test           # テスト
cargo run            # カレントディレクトリを表示
cargo run -- /path   # 指定パスを表示

# Wayland環境で実行
WAYLAND_DISPLAY=wayland-1 cargo run --release
```

---

## codex精査結果（2026-03-28実施）

P0（致命的）4件: 全て修正済み
- ダブルクリック→xdg-open
- エラーハンドリング（パーミッション拒否通知）
- シンボリックリンク対応（循環防止）
- 安全終了（SAFETYコメント付きexit(0)）

P1（重要）2件: 全て修正済み
- レスポンシブペイン（ウィンドウ幅適応）
- format_size() GB/TB対応 + 重複解消

P2（便利）3件: 全て修正済み
- 設定永続化（config.toml）
- 戻る/進む履歴（Alt+Left/Right）
- CJK文字幅対応（unicode-width）
