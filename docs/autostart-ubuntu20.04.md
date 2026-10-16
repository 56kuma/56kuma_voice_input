# ログイン時に自動起動する（Ubuntu 20.04 / X11）

Ubuntu 20.04（GNOME / X11）でログイン時に voice_input をバックグラウンド起動する手順です。

XDG autostart（`~/.config/autostart/*.desktop`）を使います。
systemd の user サービスでも起動はできますが、`DISPLAY` の受け渡しやキーリングのロック解除を待つ処理が別に要るので、ここでは使いません。

## 前提

- `--set-api-key` で API キーを OS の資格情報ストアに保存済みであること（README の「セットアップ」参照）
- ターミナルから `./target/release/voice_input` を実行して、正常に動くことを確認済みであること

## 手順

### 1. リリースビルドを用意する

```bash
cd /path/to/56kuma_voice_input
cargo build --release
```

### 2. 自動起動ファイルを作る

リポジトリ直下で実行します。`.desktop` ファイルでは `~` や環境変数が展開されないため、`$PWD` を使って絶対パスを書き込みます。

```bash
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/voice_input.desktop <<EOF
[Desktop Entry]
Type=Application
Name=voice_input
Comment=Ctrl+Space voice input
Exec=$PWD/target/release/voice_input
Path=$PWD
Terminal=false
X-GNOME-Autostart-enabled=true
X-GNOME-Autostart-Delay=5
EOF
cat ~/.config/autostart/voice_input.desktop   # パスが絶対パスになっているか確認
```

| 項目 | 意味 |
|------|------|
| `Path=` | 起動時のカレントディレクトリ。ログは `<カレント>/log/voice_input.log` に出るため、リポジトリ直下を指定する。指定しないとホーム直下の `~/log/` に出る |
| `X-GNOME-Autostart-Delay=5` | ログインから 5 秒待って起動する。ログイン直後はキーリングの準備ができておらず、API キーを読めないことがあるため |
| `Terminal=false` | ウィンドウを出さない。画面上部のカプセルだけが表示される |

設定ファイルは `voice_input --config-path` で表示される場所（通常 `~/.config/voice_input/config.toml`）から読まれるため、`Path=` の影響を受けません。

### 3. 起動を確認する

ログアウトしてログインし直します（再起動でも同じです）。そのあと次の 2 つを確認します。

```bash
pgrep -a voice_input
tail -n 20 /path/to/56kuma_voice_input/log/voice_input.log
```

ログに `transcription provider: ...` と `API key: OS credential store` が出ていれば成功です。

## ホットキーの重なりに注意

Ctrl+Space を使う常駐アプリが他にあると、voice_input は起動直後に落ちます（ログに `BadAccess`）。

- **他の音声入力ツール**（Voicy など）も自動起動している場合は、そちらの自動起動を止めます。`~/.config/autostart/` にあるそのアプリの `.desktop` ファイルに `X-GNOME-Autostart-enabled=false` と書くか、ファイルを削除します。
- **IBus / Fcitx** の入力切替が Ctrl+Space になっている場合は、どちらかのキーを変更します。

voice_input 側のキーを変えるときは、`config.toml` に次を追加します。

```toml
hotkey = "Ctrl+Alt+Space"
```

## 運用

| やること | 方法 |
|----------|------|
| 停止 | カプセルをダブルクリック、または `pkill voice_input` |
| 手動で起動 | `gtk-launch voice_input`（自動起動と同じ条件で起動） |
| 自動起動をやめる | `rm ~/.config/autostart/voice_input.desktop`（または「自動起動するアプリケーション」で OFF） |
| 更新 | `cargo build --release` → 停止 → 再起動 |

二重起動はできません。2 つ目は `!` 表示のままになります。

## うまく起動しないとき

| 症状 | 確認すること |
|------|--------------|
| プロセスがない | `log/voice_input.log` の末尾を確認する。`BadAccess` ならホットキーが重なっている |
| `API key` が見つからない | キーリングの準備が遅れている可能性がある。`X-GNOME-Autostart-Delay` を 10 秒程度に延ばす |
| ログがリポジトリにない | `.desktop` の `Path=` が絶対パスになっているか確認する。なっていなければ `~/log/voice_input.log` を確認する |
