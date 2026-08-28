#!/bin/bash
set -Eeuo pipefail
umask 077

CODEX_HOME_DIR="$HOME/.codex"
CCSWITCH_DIR="$HOME/.cc-switch"
CONFIG_FILE="$CODEX_HOME_DIR/config.toml"
AUTH_FILE="$CODEX_HOME_DIR/auth.json"
CATALOG_FILE="$CODEX_HOME_DIR/models_catalog.json"
SETTINGS_FILE="$CCSWITCH_DIR/settings.json"
CCSWITCH_DB="$CCSWITCH_DIR/cc-switch.db"

NEW_APP="/Applications/ChatGPT.app"
OLD_APP="/Applications/Codex.app"
CCSWITCH_APP="/Applications/CC Switch.app"

EXPECTED_CATALOG_SHA="7385a7c8723d65be8dc322a34935a3b2863c25983452dfff06a712b6aa71f7c4"
REMOTE_BASE_URL="https://49.232.229.239/codex/v1"

STAMP="$(date '+%Y%m%d-%H%M%S')"
BACKUP_DIR="$CODEX_HOME_DIR/backups/mac-dual-auth-$STAMP"

KEY_TMP=""
DESIRED_CONFIG=""

cleanup() {
    [[ -n "${KEY_TMP:-}" ]] && rm -f "$KEY_TMP"
    [[ -n "${DESIRED_CONFIG:-}" ]] && rm -f "$DESIRED_CONFIG"
    unset NEW_API_KEY OLD_API_KEY TOML_API_KEY 2>/dev/null || true
}
trap cleanup EXIT

mkdir -p "$CODEX_HOME_DIR" "$CCSWITCH_DIR" "$BACKUP_DIR"
chmod 700 "$CODEX_HOME_DIR" "$CCSWITCH_DIR" "$BACKUP_DIR"

echo "== 1/10 停止 Codex、ChatGPT 和 CC Switch =="

osascript -e 'tell application "Codex" to quit' >/dev/null 2>&1 || true
osascript -e 'tell application "ChatGPT" to quit' >/dev/null 2>&1 || true
osascript -e 'tell application "CC Switch" to quit' >/dev/null 2>&1 || true

sleep 2

pkill -f '/Applications/Codex\.app/Contents/' 2>/dev/null || true
pkill -f '/Applications/ChatGPT\.app/Contents/' 2>/dev/null || true
pkill -f '/Applications/CC Switch\.app/Contents/' 2>/dev/null || true

sleep 1

echo "== 2/10 检查新版应用和模型目录 =="

if [[ ! -d "$NEW_APP" ]]; then
    echo "错误：没有找到 $NEW_APP"
    echo "请先安装审计中检测到的新版 ChatGPT/Codex 应用。"
    exit 1
fi

CODEX_BIN="$NEW_APP/Contents/Resources/codex"

if [[ ! -x "$CODEX_BIN" ]]; then
    echo "错误：新版应用中没有可执行的 Codex CLI："
    echo "$CODEX_BIN"
    exit 1
fi

APP_VERSION="$(
    /usr/libexec/PlistBuddy \
        -c 'Print :CFBundleShortVersionString' \
        "$NEW_APP/Contents/Info.plist" 2>/dev/null ||
    printf 'unknown'
)"

echo "将使用：$NEW_APP"
echo "应用版本：$APP_VERSION"
echo "Codex CLI：$("$CODEX_BIN" --version 2>/dev/null || true)"

if [[ ! -f "$CATALOG_FILE" ]]; then
    echo "错误：不存在模型目录：$CATALOG_FILE"
    exit 1
fi

ACTUAL_CATALOG_SHA="$(
    shasum -a 256 "$CATALOG_FILE" |
    awk '{print $1}'
)"

if [[ "$ACTUAL_CATALOG_SHA" != "$EXPECTED_CATALOG_SHA" ]]; then
    echo "错误：模型目录不是三台电脑对比时确认的那一份。"
    echo "实际 SHA256：$ACTUAL_CATALOG_SHA"
    echo "预期 SHA256：$EXPECTED_CATALOG_SHA"
    exit 1
fi

python3 - "$CATALOG_FILE" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text(encoding="utf-8"))
models = data.get("models", data.get("data", data))

if not isinstance(models, list):
    raise SystemExit("错误：无法识别 models_catalog.json 的模型数组")

visible = [
    m for m in models
    if isinstance(m, dict) and m.get("visibility") == "list"
]

print(f"模型目录总数：{len(models)}")
print(f"标记为可见：{len(visible)}")

if len(models) != 30 or len(visible) != 21:
    raise SystemExit("错误：模型目录不是预期的 30/21 结构")
PY

echo "== 3/10 备份当前配置 =="

for path in \
    "$CONFIG_FILE" \
    "$AUTH_FILE" \
    "$CATALOG_FILE" \
    "$CODEX_HOME_DIR/models_cache.json" \
    "$SETTINGS_FILE" \
    "$CCSWITCH_DB"
do
    if [[ -e "$path" ]]; then
        cp -p "$path" "$BACKUP_DIR/"
    fi
done

echo "备份位置：$BACKUP_DIR"

OLD_API_KEY="$(
    python3 - "$CONFIG_FILE" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
if not path.exists():
    print("")
    raise SystemExit

text = path.read_text(encoding="utf-8", errors="replace")
match = re.search(
    r'(?m)^\s*experimental_bearer_token\s*=\s*"([^"]*)"',
    text,
)
print(match.group(1) if match else "")
PY
)"

echo "== 4/10 输入轮换后的新远端 API Key =="

while true; do
    IFS= read -r -s -p "粘贴新的远端 API Key，然后回车：" NEW_API_KEY </dev/tty
    printf '\n'

    if [[ ${#NEW_API_KEY} -lt 20 ]]; then
        echo "Key 长度异常，请重新输入。"
        continue
    fi

    if [[ -n "$OLD_API_KEY" && "$NEW_API_KEY" == "$OLD_API_KEY" ]]; then
        echo "不能继续使用刚才已经公开的旧 Key，请先在服务端轮换。"
        continue
    fi

    break
done

TOML_API_KEY="$(
    printf '%s' "$NEW_API_KEY" |
    python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))'
)"

KEY_TMP="$(mktemp "${TMPDIR:-/tmp}/codex-remote-key.XXXXXX")"
printf '%s' "$NEW_API_KEY" > "$KEY_TMP"
chmod 600 "$KEY_TMP"

echo "== 5/10 验证新 Key 能否访问远端 =="

python3 - "$KEY_TMP" "$REMOTE_BASE_URL" <<'PY'
import json
import pathlib
import ssl
import sys
import urllib.error
import urllib.request

key = pathlib.Path(sys.argv[1]).read_text()
base = sys.argv[2].rstrip("/")

request = urllib.request.Request(
    base + "/models",
    headers={
        "Authorization": "Bearer " + key,
        "Accept": "application/json",
        "User-Agent": "codex-mac-repair/1.0",
    },
)

try:
    with urllib.request.urlopen(
        request,
        timeout=30,
        context=ssl.create_default_context(),
    ) as response:
        body = response.read()
        status = response.status
except urllib.error.HTTPError as exc:
    body = exc.read()
    raise SystemExit(f"远端验证失败：HTTP {exc.code}")
except Exception as exc:
    raise SystemExit(f"远端连接失败：{type(exc).__name__}: {exc}")

if status != 200:
    raise SystemExit(f"远端验证失败：HTTP {status}")

data = json.loads(body)
models = data.get("data", data.get("models", []))

print("远端认证：成功")
print("远端模型数：", len(models) if isinstance(models, list) else "未知")
PY

rm -f "$KEY_TMP"
KEY_TMP=""

echo "== 6/10 生成完整 Codex 配置 =="

DESIRED_CONFIG="$(mktemp "$CODEX_HOME_DIR/.config-desired.XXXXXX")"

cat > "$DESIRED_CONFIG" <<EOF
model = "gpt-5.6-sol"
model_catalog_json = "$HOME/.codex/models_catalog.json"
model_reasoning_effort = "ultra"
service_tier = "priority"
model_provider = "codexmanager_server"

cli_auth_credentials_store = "file"
forced_login_method = "chatgpt"

followUpQueueMode = "queue"
conversationDetailMode = "STEPS_COMMANDS"
enabled-reasoning-efforts = ["low", "medium", "high", "xhigh", "ultra", "max"]
keepRemoteControlAwakeWhilePluggedIn = true
approval_policy = "never"
sandbox_mode = "danger-full-access"

[model_providers.codexmanager_server]
name = "Codex Manager"
wire_api = "responses"
experimental_bearer_token = $TOML_API_KEY
base_url = "$REMOTE_BASE_URL"
requires_openai_auth = true

[marketplaces.openai-bundled]
last_updated = "2026-08-10T20:39:48Z"
source_type = "local"
source = "$HOME/.codex/.tmp/bundled-marketplaces/openai-bundled"

[marketplaces.openai-primary-runtime]
last_updated = "2026-08-03T17:15:12Z"
source_type = "local"
source = "$HOME/.cache/codex-runtimes/codex-primary-runtime/plugins/openai-primary-runtime"

[projects."$HOME/Documents/Codex/2026-08-04/1-3"]
trust_level = "trusted"

[projects."$HOME/easy agent"]
trust_level = "trusted"

[projects."$HOME/Volumes/奇米Studio/01_code_yunbeifen"]
trust_level = "trusted"

[projects."$HOME"]
trust_level = "trusted"

[projects."$HOME/Volumes/奇米Studio/01_code_yunbeifen/01_code/agent5.0"]
trust_level = "trusted"

[plugins."chrome@openai-bundled"]
enabled = true

[plugins."browser@openai-bundled"]
enabled = true

[plugins."visualize@openai-bundled"]
enabled = true

[plugins."documents@openai-primary-runtime"]
enabled = true

[plugins."pdf@openai-primary-runtime"]
enabled = true

[plugins."spreadsheets@openai-primary-runtime"]
enabled = true

[plugins."presentations@openai-primary-runtime"]
enabled = true

[plugins."template-creator@openai-primary-runtime"]
enabled = true

[desktop]
followUpQueueMode = "queue"
enabled-reasoning-efforts = ["low", "medium", "high", "xhigh", "ultra", "max"]
conversationDetailMode = "STEPS_COMMANDS"
git-branch-prefix = "coding center/main"

[features]
js_repl = false

[shell_environment_policy.set]
BROWSER_USE_AVAILABLE_BACKENDS = "chrome,iab"
NODE_REPL_TRUSTED_BROWSER_CLIENT_SHA256S = "091a81603ff202a16ed56557709bf42d97caf8f0dd2e07ae9e26d7c014d71035"
NODE_REPL_TRUSTED_CODE_PATHS = "$HOME/.codex:$NEW_APP/Contents/Resources/cua_node/lib/node_modules"
EOF

OMX_SERVER="$HOME/code/ohmycodex/packages/mcp-server/dist/index.js"

if [[ -f "$OMX_SERVER" ]]; then
    cat >> "$DESIRED_CONFIG" <<EOF

[mcp_servers.omx]
command = "node"
args = ["$OMX_SERVER"]
startup_timeout_sec = 20.0
tool_timeout_sec = 120.0
EOF
fi

NODE_REPL_BIN="$NEW_APP/Contents/Resources/cua_node/bin/node_repl"
NODE_BIN="$NEW_APP/Contents/Resources/cua_node/bin/node"
NODE_MODULES="$NEW_APP/Contents/Resources/cua_node/lib/node_modules"

if [[ -x "$NODE_REPL_BIN" ]]; then
    cat >> "$DESIRED_CONFIG" <<EOF

[mcp_servers.node_repl]
args = []
command = "$NODE_REPL_BIN"
startup_timeout_sec = 120

[mcp_servers.node_repl.env]
NODE_REPL_NATIVE_PIPE_CONNECT_TIMEOUT_MS = "1000"
NODE_REPL_NODE_MODULE_DIRS = "$NODE_MODULES"
NODE_REPL_NODE_PATH = "$NODE_BIN"
NODE_REPL_TRUSTED_CODE_PATHS = "$HOME/.codex:$NODE_MODULES"
CODEX_HOME = "$HOME/.codex"
NODE_REPL_TRUSTED_BROWSER_CLIENT_SHA256S = "091a81603ff202a16ed56557709bf42d97caf8f0dd2e07ae9e26d7c014d71035"
BROWSER_USE_AVAILABLE_BACKENDS = "chrome,iab"
NODE_REPL_INSTRUCTIONS_USE_CASE_BROWSER = "Control the in-app browser in conjunction with the Browser Plugin."
NODE_REPL_INSTRUCTIONS_USE_CASE_CHROME = "Control the Chrome browser in conjunction with the Chrome Plugin. Prefer this method of controlling Chrome over alternatives unless the user explicitly requests another method."
NODE_REPL_INSTRUCTIONS_USE_CASE_COMPUTER_USE = "Control desktop apps on macOS through Computer Use."
BROWSER_USE_CODEX_APP_BUILD_FLAVOR = "prod"
BROWSER_USE_CODEX_APP_VERSION = "$APP_VERSION"
CODEX_CLI_PATH = "$CODEX_BIN"
EOF
fi

cat >> "$DESIRED_CONFIG" <<'EOF'

[mcp_servers.openaiDeveloperDocs]
url = "https://developers.openai.com/mcp"

[tui.model_availability_nux]
"gpt-5.6-sol" = 4
EOF

cp "$DESIRED_CONFIG" "$CONFIG_FILE"
chmod 600 "$CONFIG_FILE"

python3 - "$CONFIG_FILE" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])

try:
    import tomllib
except ModuleNotFoundError:
    print("Python 无 tomllib，跳过 Python TOML 校验")
else:
    with path.open("rb") as handle:
        data = tomllib.load(handle)

    provider_id = data.get("model_provider")
    providers = data.get("model_providers", {})
    provider = providers.get(provider_id, {})

    required = {
        "model_provider": provider_id == "codexmanager_server",
        "model_catalog_json": bool(data.get("model_catalog_json")),
        "base_url": provider.get("base_url", "").endswith("/codex/v1"),
        "wire_api": provider.get("wire_api") == "responses",
        "provider_token": bool(provider.get("experimental_bearer_token")),
        "requires_openai_auth": provider.get("requires_openai_auth") is True,
        "login_store": data.get("cli_auth_credentials_store") == "file",
        "forced_chatgpt": data.get("forced_login_method") == "chatgpt",
    }

    failed = [name for name, passed in required.items() if not passed]
    if failed:
        raise SystemExit("配置校验失败：" + ", ".join(failed))

    print("Codex 配置结构校验：成功")
PY

echo "== 7/10 同步修复 CC Switch 设置和当前供应商 =="

python3 - "$SETTINGS_FILE" "$CCSWITCH_DB" "$CONFIG_FILE" <<'PY'
import json
import os
import pathlib
import re
import sqlite3
import sys
import tempfile

settings_path = pathlib.Path(sys.argv[1])
db_path = pathlib.Path(sys.argv[2])
config_path = pathlib.Path(sys.argv[3])

config_text = config_path.read_text(encoding="utf-8")

token_match = re.search(
    r'(?m)^\s*experimental_bearer_token\s*=\s*(.+?)\s*$',
    config_text,
)
if not token_match:
    raise SystemExit("无法从新配置中读取 provider Token")

provider_token = json.loads(token_match.group(1))
provider_id = None

if db_path.exists():
    connection = sqlite3.connect(db_path)

    row = connection.execute(
        """
        SELECT rowid, id, settings_config, meta
        FROM providers
        WHERE app_type = 'codex' AND is_current = 1
        ORDER BY rowid DESC
        LIMIT 1
        """
    ).fetchone()

    if row is None:
        row = connection.execute(
            """
            SELECT rowid, id, settings_config, meta
            FROM providers
            WHERE app_type = 'codex'
              AND lower(name) = 'codex'
            ORDER BY rowid DESC
            LIMIT 1
            """
        ).fetchone()

    if row is None:
        connection.close()
        raise SystemExit("CC Switch 数据库中没有找到 codex 供应商")

    rowid, provider_id, settings_raw, meta_raw = row
    settings_config = json.loads(settings_raw)

    settings_config["config"] = config_text
    settings_config["model"] = "gpt-5.6-sol"
    settings_config["base_url"] = "https://49.232.229.239/codex/v1"

    if "baseURL" in settings_config:
        settings_config["baseURL"] = "https://49.232.229.239/codex/v1"

    auth = settings_config.get("auth")
    if not isinstance(auth, dict):
        auth = {}

    auth["OPENAI_API_KEY"] = provider_token
    settings_config["auth"] = auth

    if "apiKey" in settings_config:
        settings_config["apiKey"] = provider_token
    if "api_key" in settings_config:
        settings_config["api_key"] = provider_token

    try:
        meta = json.loads(meta_raw or "{}")
    except Exception:
        meta = {}

    meta["apiFormat"] = "openai_responses"

    connection.execute(
        """
        UPDATE providers
        SET settings_config = ?, meta = ?, is_current = 1
        WHERE rowid = ?
        """,
        (
            json.dumps(settings_config, ensure_ascii=False),
            json.dumps(meta, ensure_ascii=False),
            rowid,
        ),
    )

    connection.execute(
        """
        UPDATE providers
        SET is_current = 0
        WHERE app_type = 'codex' AND rowid != ?
        """,
        (rowid,),
    )

    try:
        columns = {
            row[1]
            for row in connection.execute("PRAGMA table_info(proxy_config)")
        }
        assignments = []
        values = []

        for column in ("proxy_enabled", "enabled", "live_takeover_active"):
            if column in columns:
                assignments.append(f"{column} = ?")
                values.append(0)

        if assignments:
            connection.execute(
                "UPDATE proxy_config SET " + ", ".join(assignments),
                values,
            )
    except sqlite3.Error:
        pass

    connection.commit()
    connection.close()

if settings_path.exists():
    settings = json.loads(settings_path.read_text(encoding="utf-8"))
else:
    settings = {}

settings["preserveCodexOfficialAuthOnSwitch"] = True
settings["enableLocalProxy"] = False

if provider_id:
    settings["currentProviderCodex"] = provider_id

settings_path.parent.mkdir(parents=True, exist_ok=True)

fd, temporary_name = tempfile.mkstemp(
    prefix=".settings.",
    suffix=".json",
    dir=settings_path.parent,
)
try:
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(settings, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    os.chmod(temporary_name, 0o600)
    os.replace(temporary_name, settings_path)
finally:
    if os.path.exists(temporary_name):
        os.unlink(temporary_name)

print("CC Switch 保留官方登录：已开启")
print("CC Switch 本地路由接管：已关闭")
print("CC Switch 当前供应商：已同步为 codex")
PY

chmod 600 "$SETTINGS_FILE" 2>/dev/null || true
chmod 600 "$CCSWITCH_DB" 2>/dev/null || true

echo "== 8/10 清理旧模型列表缓存 =="

if [[ -f "$CODEX_HOME_DIR/models_cache.json" ]]; then
    mv \
        "$CODEX_HOME_DIR/models_cache.json" \
        "$BACKUP_DIR/models_cache.pre-reset.json"
fi

for cache_path in \
    "$HOME/Library/Caches/com.openai.codex" \
    "$HOME/Library/Saved Application State/com.openai.codex.savedState"
do
    if [[ -e "$cache_path" ]]; then
        cache_name="$(basename "$cache_path")"
        mv "$cache_path" "$BACKUP_DIR/${cache_name}.pre-reset"
    fi
done

echo "== 9/10 重新建立 ChatGPT 官方登录 =="

"$CODEX_BIN" logout >/dev/null 2>&1 || true

echo
echo "接下来会打开浏览器，请完成 ChatGPT 登录。"
echo

if ! "$CODEX_BIN" login; then
    echo "普通浏览器登录失败，改用设备码登录。"
    "$CODEX_BIN" login --device-auth
fi

LOGIN_STATUS="$("$CODEX_BIN" login status 2>&1 || true)"
printf '%s\n' "$LOGIN_STATUS"

if ! printf '%s' "$LOGIN_STATUS" | grep -qi 'chatgpt'; then
    echo "错误：登录状态没有显示 ChatGPT。"
    echo "配置没有回滚；请不要恢复旧的泄露 Token。"
    exit 1
fi

python3 - "$CCSWITCH_DB" "$AUTH_FILE" <<'PY'
import json
import pathlib
import sqlite3
import sys

db_path = pathlib.Path(sys.argv[1])
auth_path = pathlib.Path(sys.argv[2])

if not db_path.exists() or not auth_path.exists():
    raise SystemExit

live_auth = json.loads(auth_path.read_text(encoding="utf-8"))
connection = sqlite3.connect(db_path)

row = connection.execute(
    """
    SELECT rowid, settings_config
    FROM providers
    WHERE app_type = 'codex' AND is_current = 1
    ORDER BY rowid DESC
    LIMIT 1
    """
).fetchone()

if row:
    rowid, raw = row
    settings_config = json.loads(raw)

    provider_auth = settings_config.get("auth")
    if not isinstance(provider_auth, dict):
        provider_auth = {}

    provider_key = provider_auth.get("OPENAI_API_KEY")

    for field in ("auth_mode", "tokens", "last_refresh"):
        if field in live_auth:
            provider_auth[field] = live_auth[field]

    provider_auth["OPENAI_API_KEY"] = provider_key
    settings_config["auth"] = provider_auth

    connection.execute(
        "UPDATE providers SET settings_config = ? WHERE rowid = ?",
        (json.dumps(settings_config, ensure_ascii=False), rowid),
    )
    connection.commit()

connection.close()
PY

echo "== 10/10 注册并启动正确版本 =="

LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"

if [[ -x "$LSREGISTER" ]]; then
    if [[ -d "$OLD_APP" ]]; then
        "$LSREGISTER" -u "$OLD_APP" >/dev/null 2>&1 || true
    fi
    "$LSREGISTER" -f "$NEW_APP" >/dev/null 2>&1 || true
fi

if [[ -d "$CCSWITCH_APP" ]]; then
    open "$CCSWITCH_APP"
    sleep 3
fi

python3 - "$CONFIG_FILE" "$DESIRED_CONFIG" <<'PY'
import pathlib
import re
import sys

live_path = pathlib.Path(sys.argv[1])
desired_path = pathlib.Path(sys.argv[2])

live = live_path.read_text(encoding="utf-8") if live_path.exists() else ""
desired = desired_path.read_text(encoding="utf-8")

checks = [
    r'(?m)^model_provider\s*=\s*"codexmanager_server"',
    r'(?m)^model_catalog_json\s*=',
    r'(?m)^experimental_bearer_token\s*=',
    r'(?m)^requires_openai_auth\s*=\s*true',
    r'(?m)^base_url\s*=\s*"https://49\.232\.229\.239/codex/v1"',
]

if not all(re.search(pattern, live) for pattern in checks):
    live_path.write_text(desired, encoding="utf-8")
    live_path.chmod(0o600)
    print("CC Switch 启动时曾覆盖配置，已自动重新写入正确配置")
else:
    print("CC Switch 启动后配置保持正确")
PY

open "$NEW_APP"

echo
echo "============================================"
echo "Mac 双认证与模型目录修复脚本执行完成"
echo "============================================"
echo "实际启动应用：$NEW_APP"
echo "应用版本：$APP_VERSION"
echo "登录状态：ChatGPT"
echo "模型调用：远端 provider API Key"
echo "模型目录：30 个模型，21 个可见"
echo "配置备份：$BACKUP_DIR"
echo
echo "以后不要再启动旧的：$OLD_APP"
echo "只使用新的：$NEW_APP"
echo
echo "请在新建任务中检查："
echo "1. 左下角账号状态是否恢复"
echo "2. 模型列表是否出现 Kimi、Grok 等模型"
echo "3. 选择 kimi-k3 后远端后台是否出现调用"
echo
echo "如果仍只显示 6 个模型，请截图“关于”里的实际版本，"
echo "不要继续修改配置；那将是桌面端模型选择器过滤问题。"
