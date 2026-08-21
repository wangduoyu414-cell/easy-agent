#!/usr/bin/env bash
set -euo pipefail

readonly max_redirects=5

requested_target=${SSH_ORIGINAL_COMMAND:-}
case "$requested_target" in
    ''|'resolve windows x64')
        readonly target_key='windows-x64-setup'
        readonly documented_entry_url='https://claude.ai/api/desktop/win32/x64/setup/latest/redirect'
        readonly private_compatibility_entry_url='https://api.anthropic.com/api/desktop/win32/x64/setup/latest/redirect'
        ;;
    'resolve windows arm64')
        readonly target_key='windows-arm64-setup'
        readonly documented_entry_url='https://claude.ai/api/desktop/win32/arm64/setup/latest/redirect'
        readonly private_compatibility_entry_url='https://api.anthropic.com/api/desktop/win32/arm64/setup/latest/redirect'
        ;;
    'resolve windows x64 msix')
        readonly target_key='windows-x64-msix'
        readonly documented_entry_url='https://claude.ai/api/desktop/win32/x64/msix/latest/redirect'
        readonly private_compatibility_entry_url='https://api.anthropic.com/api/desktop/win32/x64/msix/latest/redirect'
        ;;
    'resolve windows arm64 msix')
        readonly target_key='windows-arm64-msix'
        readonly documented_entry_url='https://claude.ai/api/desktop/win32/arm64/msix/latest/redirect'
        readonly private_compatibility_entry_url='https://api.anthropic.com/api/desktop/win32/arm64/msix/latest/redirect'
        ;;
    'resolve macos universal')
        readonly target_key='macos-universal'
        readonly documented_entry_url='https://claude.ai/api/desktop/darwin/universal/dmg/latest/redirect'
        readonly private_compatibility_entry_url='https://api.anthropic.com/api/desktop/darwin/universal/dmg/latest/redirect'
        ;;
    *)
        printf 'Unsupported Claude resolver target\n' >&2
        exit 2
        ;;
esac

headers=$(mktemp)
cleanup() {
    rm -f "$headers"
}
trap cleanup EXIT

allowed_artifact_url() {
    local artifact_url="$1"
    case "$target_key" in
        windows-x64-setup)
            [[ "$artifact_url" =~ ^https://downloads\.claude\.ai/releases/win32/x64/[0-9]+(\.[0-9]+){2,3}/[A-Za-z0-9._-]+\.exe$ ]]
            ;;
        windows-arm64-setup)
            [[ "$artifact_url" =~ ^https://downloads\.claude\.ai/releases/win32/arm64/[0-9]+(\.[0-9]+){2,3}/[A-Za-z0-9._-]+\.exe$ ]]
            ;;
        windows-x64-msix)
            [[ "$artifact_url" =~ ^https://downloads\.claude\.ai/releases/win32/x64/[0-9]+(\.[0-9]+){2,3}/[A-Za-z0-9._-]+\.msix$ ]]
            ;;
        windows-arm64-msix)
            [[ "$artifact_url" =~ ^https://downloads\.claude\.ai/releases/win32/arm64/[0-9]+(\.[0-9]+){2,3}/[A-Za-z0-9._-]+\.msix$ ]]
            ;;
        macos-universal)
            [[ "$artifact_url" =~ ^https://downloads\.claude\.ai/releases/darwin/universal/[0-9]+(\.[0-9]+){2,3}/[A-Za-z0-9._-]+\.dmg$ ]]
            ;;
    esac
}

resolve_entry() {
    local entry_url="$1"
    local current_url="$entry_url"
    local status location

    for ((redirect_count = 0; redirect_count <= max_redirects; redirect_count++)); do
        if [[ "$current_url" != "$entry_url" ]] && ! allowed_artifact_url "$current_url"; then
            printf 'Claude redirect left the fixed host/path contract\n' >&2
            return 1
        fi

        : >"$headers"
        if ! status=$(
            env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
                curl --noproxy '*' \
                --silent --show-error \
                --proto '=https' --tlsv1.2 \
                --range '0-0' \
                --connect-timeout 10 --max-time 40 \
                --dump-header "$headers" --output /dev/null \
                --write-out '%{http_code}' \
                "$current_url"
        ); then
            return 1
        fi

        if [[ "$status" =~ ^30[12378]$ ]]; then
            if ((redirect_count == max_redirects)); then
                printf 'Claude redirect limit exceeded\n' >&2
                return 1
            fi
            location=$(
                awk 'BEGIN { IGNORECASE=1 }
                    /^Location:/ {
                        sub(/^[^:]+:[[:space:]]*/, "")
                        sub(/\r$/, "")
                        value=$0
                    }
                    END { print value }' "$headers"
            )
            if [[ -z "$location" ]]; then
                printf 'Claude redirect has no Location header\n' >&2
                return 1
            fi
            current_url=$(
                python3 - "$current_url" "$location" <<'PY'
import sys
from urllib.parse import urljoin

print(urljoin(sys.argv[1], sys.argv[2]))
PY
            )
            continue
        fi

        if [[ "$status" =~ ^20[06]$ ]] && allowed_artifact_url "$current_url"; then
            printf '%s\n' "$current_url"
            return 0
        fi

        printf 'Claude resolver received unexpected HTTP status %s\n' "$status" >&2
        return 1
    done

    printf 'Claude redirect limit exceeded\n' >&2
    return 1
}

if resolved_url=$(resolve_entry "$documented_entry_url"); then
    printf '%s\n' "$resolved_url"
    exit 0
fi

printf 'Documented Claude entry unavailable; trying the private compatibility resolver\n' >&2
if resolved_url=$(resolve_entry "$private_compatibility_entry_url"); then
    printf '%s\n' "$resolved_url"
    exit 0
fi

printf 'Both fixed Claude resolver entries are unavailable\n' >&2
exit 1
