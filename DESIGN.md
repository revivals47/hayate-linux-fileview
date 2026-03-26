# Hayate File Viewer — 設計メモ

## 概要
Hayate UIツールキットの上に構築するファイルビューア。
Windowsから移行してきたLinuxユーザーが戸惑わないUIを目指す。

## 動機
- Nautilus（GNOME Files）はWindowsユーザーの期待と乖離がある
- Mac版Hayateファイルマネージャの知見を活かす
- Hayate UIのドッグフーディング（実アプリで品質を検証する）

## Windowsユーザーが期待してNautilusにないもの

| 期待 | Nautilusの現実 | Hayateで提供 |
|------|---------------|-------------|
| 「上へ」ボタン | GNOME 3で廃止 | ツールバーに配置 |
| 表示モード（詳細/大アイコン等） | 2種類のみ | 最低4種類 |
| プレビューペイン | 拡張が必要 | 標準搭載 |
| インクリメンタルジャンプ（タイプで選択） | 検索モードに入る | タイプでジャンプ |
| ドライブ一覧 | 「その他の場所」 | サイドバーにマウントポイント |
| パスのコピー | 右クリックにない | 右クリック + Ctrl+Shift+C |
| カラムの自由なカスタマイズ | 限定的 | ドラッグで並び替え |

## 技術的な強み（Hayate UI由来）

- **1Mファイル対応** — FenwickTree O(log n)スクロール仮想化
- **Wayland D&D** — GTKのXDND抽象なし、単一コードパス
- **HiDPIネイティブ** — wp_fractional_scale_v1
- **CPU/GPU描画** — tiny-skia + wgpu自動選択

## ファイルシステムアクセス

Mac版HayateはgetattrbulkでApple APIを直接叩いている。
Linux版では:
- `getdents64` — ディレクトリエントリを直接読む（std::fs::read_dirより高速）
- `statx` — バッチ的メタデータ取得
- inotify — ディレクトリ変更監視

## 依存関係

```toml
[dependencies]
hayate-ui = { path = "../GUI_kit" }
```

## 次のステップ

1. Hayate UIのドッグフーディングで壊れている箇所を特定
2. 最小限のファイルリスト表示（VirtualListDelegate + FileTree）
3. ナビゲーション（パンくず、戻る、上へ）
4. D&D（ファイル移動/コピー）
5. プレビューペイン
