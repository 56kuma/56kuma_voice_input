# voice_input

Ctrl + Space を押す → 話す → もう一度 Ctrl + Space → カーソル位置に文字が入る。

Windows / Ubuntu で動く、バックグラウンド常駐型の軽量音声入力ツールです。
Rust 製・WebView なし。待機中はマイクもネットワークも使いません。

```
Idle ──Ctrl+Space──▶ Recording ──Ctrl+Space──▶ Transcribing ──API成功──▶ Inserting ──▶ Idle
                          │                          │                       │
                          └──────── 失敗 ────────────┴──────── 失敗 ─────────┴──▶ Error ──数秒──▶ Idle
```

画面上部中央に小さな半透明のカプセルが 1 つ出ます。文字は出しません。

<img src="docs/overlay-states.png" alt="Idle / Error / Idle の実表示（Xvfb 上でキャプチャ）" width="120">

| 表示 | 状態 |
|------|------|
| ○    | Idle |
| ●（呼吸するように脈動・声量に反応） | Recording |
| •••  | Transcribing / Inserting |
| ✓（1.2 秒） | 挿入完了 |
| !    | Error（数秒で Idle に戻る） |

## セットアップ

```bash
# 1. ビルド
cargo build --release          # Linux は事前に: sudo apt install libasound2-dev

# 2. API キーを OS の資格情報ストアに保存（stdin から読むので履歴に残りません）
./target/release/voice_input --set-api-key

# 3. 起動（ターミナルで Ctrl+C か、カプセルをダブルクリックで終了）
./target/release/voice_input
```

API キーの探索順は **OS Credential Store → 環境変数 `OPENAI_API_KEY` → config.toml** です。

### Groq を使う場合

Groq は OpenAI 互換の API なので、OpenAI と同じ仕組みで動きます。キーは OpenAI・xAI とは別の場所に保存されます。

1. [console.groq.com](https://console.groq.com/keys) で API キーを作成します（`gsk_` で始まります）。
2. キーを OS の資格情報ストアに保存します（貼り付けて Enter）。
   ```bash
   ./target/release/voice_input --set-api-key --provider groq
   ```
3. `config.toml`（場所は `voice_input --config-path` で確認。Linux は通常 `~/.config/voice_input/config.toml`）に次を書きます。
   ```toml
   provider = "groq"
   [groq]
   language = "ja"                  # 省略時は自動判定。日本語なら指定推奨
   # model = "whisper-large-v3"     # 既定は whisper-large-v3-turbo（速い）。精度重視なら large-v3
   ```
4. 起動して、ログに `transcription provider: groq` と `API key: OS credential store` が出れば設定完了です。
   ```bash
   ./target/release/voice_input
   ```

| 項目 | 値 |
|------|----|
| 送信先 | `https://api.groq.com/openai/v1/audio/transcriptions`（`[groq] base_url` で変更可） |
| キーの探索順 | OS Credential Store（`groq_api_key`）→ 環境変数 `GROQ_API_KEY` → config.toml の `[groq] api_key` |
| キーの削除 | `voice_input --delete-api-key --provider groq` |

うまくいかないときは `log/voice_input.log` の `Transcription failed: ...` の行を確認してください（例: `API key rejected` ならキーの誤り、`rate limit or quota exceeded` なら利用上限）。

### xAI（Grok）を使う場合

`config.toml` で `provider = "xai"` を指定し、xAI のキーを保存します（OpenAI のキーとは別に保存されます）。

```bash
./target/release/voice_input --set-api-key --provider xai
```

```toml
provider = "xai"
[xai]
language = "ja"   # 省略時は自動判定
```

キーの探索順は OpenAI と同じで、環境変数は `XAI_API_KEY`、config.toml は `[xai] api_key` です。
設定ファイルの場所は `voice_input --config-path` で表示されます。項目は
[`config.example.toml`](config.example.toml) を参照してください。

ログは `RUST_LOG=debug voice_input`。API キー・録音データ・文字起こし本文はどのレベルでもログに出しません。
録音はメモリ上だけで扱い、文字起こし後に破棄します。履歴は保存しません。
実行ログは起動時のカレントディレクトリ配下の `log\voice_input.log` にも追記されます（git 管理外）。

## 起動方法まとめ（Windows・最終形）

Windows では GUI アプリとしてビルドされるため、ラッパー（PowerShell / vbs / nohup）は不要です。

| やること | 方法 |
|----------|------|
| 起動 | `target\release\voice_input.exe` を実行（ダブルクリック可・コンソールは出ない） |
| 停止 | カプセルをダブルクリック（または `taskkill /F /IM voice_input.exe`） |
| サインイン時に自動起動 | `Win + R` → `shell:startup` に exe のショートカットを置く（解除は削除） |

補足:

- `config.toml` と `log\` は「カレント → exe の場所 → exe の 2 つ上（= リポジトリ直下）」の順で解決されるため、どこから起動しても同じ場所が使われます。
- CLI（`--help` / `--set-api-key` など）は従来どおりターミナルから使えます。
- 二重起動は不可（2 つ目は `!` 表示のまま）。起動に失敗したら `log\voice_input.log` を確認。

## プラットフォーム別の注意

### Windows
- ホットキー: `RegisterHotKey`、文字入力: `SendInput`（Unicode）。失敗時はクリップボード + Ctrl+V にフォールバックし、元のクリップボード内容を復元します。
- 追加インストールは不要です。

### Ubuntu (X11)
- ホットキー: `XGrabKey`。文字入力: 日本語など非 ASCII を含む文はクリップボード + XTEST Ctrl+V（IME の入力モードに左右されません。ターミナルでは貼り付けられません）。ASCII のみの文は `xdotool` があればそれで入力（ターミナルでも動作。ただし IME がかなモードだと変換されます）。
  - Ubuntu 20.04 の xdotool（3.20160805）は非 ASCII を打つと文字が繰り返される不具合があるため、非 ASCII には使いません。
  ```bash
  sudo apt install xdotool
  ```
- **IBus / Fcitx の既定のキー切替が Ctrl+Space の場合は競合します。** どちらかを変更してください（`hotkey = "Ctrl+Alt+Space"` など）。
  他の常駐アプリ（音声入力ツールなど）が同じキーを使っていても起動直後に落ちます（ログに `BadAccess`）。

### Ubuntu (Wayland / GNOME)
- ホットキー: XDG Desktop Portal の `GlobalShortcuts` を使います。初回起動時にショートカットの確認ダイアログが出ます（GNOME 46 以降 / KDE 6）。
- 文字入力: Wayland は他アプリへの入力注入を許可しないため、`wtype`（wlroots 系）→ `ydotool`（要 `ydotoold`）の順に試し、どちらも無ければ **クリップボードにコピーした上で Error 表示** にします（Ctrl+V で貼り付けできます）。
  ```bash
  sudo apt install ydotool && sudo systemctl enable --now ydotool
  ```
- オーバーレイの位置指定と最前面固定は Wayland のプロトコル上できないため、コンポジタの配置に従います。

## 設計

```
src/
  app/          state.rs（純粋な状態遷移）  controller.rs（Effect の実行）
  audio/        audio_data.rs（純粋な変換）  recorder.rs（cpal）
  transcription/provider.rs（trait）        openai.rs  groq.rs  xai.rs
  hotkey/       mod.rs（parse / debounce） windows.rs  linux.rs
  input/        mod.rs（trait）  windows.rs  x11.rs  wayland.rs  clipboard.rs
  ui/           state_view.rs（純粋な表示ロジック）  overlay.rs（eframe）
  config/       config.rs  credentials.rs
  platform/     判定と組み立て
  doubles.rs    Fake（Recorder / Provider / Injector / Overlay / CredentialStore）
```

- **Business Logic ≠ OS API ≠ UI ≠ Network。** 判断はすべて `app/` と `*_view` / `audio_data` の純粋なコードにあり、OS 依存部分は薄い Adapter（Humble Object）です。
- `StateMachine::handle(Event) -> Vec<Effect>` が中心。副作用は Controller が `Effect` を見て実行します。
- 白銀比 `1 : √2` を幅/高さ、角丸、グリフ、余白、脈動の最大倍率に用いています（`ui/state_view.rs::Layout`）。
- 将来: `TranscriptionProvider` を実装すれば Google / ローカル Whisper へ、`TextInjector` を実装すれば IBus へ差し替え可能です。

## 開発（t_wada 式 TDD）

Red → Green → Refactor を小さく回しています。コミット履歴がそのままサイクルの記録です。

```bash
cargo test                 # 66 テスト。マイク・ネットワーク・ディスプレイ不要
cargo clippy --all-targets -- -D warnings
cargo check --target x86_64-pc-windows-gnu   # Linux 上で Windows 向けに検証
```

最重要テストは `src/app/state.rs`（状態遷移）、`tests/controller.rs`（Fake で全体を通す）、`tests/hotkey.rs`（連打しても二重起動しない）にあります。

ホットキーが反応しないときの切り分け用に `cargo run --example hotkey_probe` があります（6 秒間 Ctrl+Space を待ち、押すたびに `TOGGLE` を表示）。
