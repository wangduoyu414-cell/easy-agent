#!/usr/bin/env bash
set -euo pipefail
umask 027

readonly signing_key='/etc/easy-agent-sync/mirror-signing.key'
readonly tool='/usr/local/libexec/easy-agent/chatgpt-mirror-tool'
readonly state_root='/var/lib/easy-agent-sync/chatgpt-macos'
readonly private_root='/srv/easy-agent/private'
readonly artifact_root="$private_root/artifacts/chatgpt/macos"
readonly manifest_root="$private_root/manifests/chatgpt/macos"
readonly staging_root="$state_root/staging"
readonly lock_file='/run/easy-agent-sync/chatgpt-macos-sync.lock'

mkdir -p "$artifact_root" "$manifest_root" "$staging_root"
declare -a cleanup_files=()
cleanup_all() {
    local cleanup_file
    for cleanup_file in "${cleanup_files[@]}"; do
        if [[ "$cleanup_file" == "$staging_root/"* || "$cleanup_file" == "$manifest_root/"* || "$cleanup_file" == "$state_root/"* ]]; then
            rm -f -- "$cleanup_file"
        fi
    done
}
trap cleanup_all EXIT
exec 9>"$lock_file"
if ! flock -n 9; then
    exit 0
fi

sync_architecture() {
    local architecture="$1"
    local appcast_name="$2"
    local appcast_url="https://persistent.oaistatic.com/codex-app-prod/$appcast_name"
    local now
    now=$(date +%s)
    local appcast_tmp="$staging_root/.appcast-$architecture.xml.$$"
    local partial=''
    local state_tmp=''
    local manifest_tmp=''
    local signature_tmp=''
    cleanup_files+=("$appcast_tmp")

    local fetch_result
    fetch_result=$(env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
        curl --noproxy '*' \
        --fail --silent --show-error \
        --proto '=https' --tlsv1.2 \
        --retry 2 --retry-all-errors \
        --connect-timeout 15 --max-time 120 \
        --output "$appcast_tmp" --write-out $'%{http_code}\t%{url_effective}' \
        "$appcast_url")
    local fetch_status fetch_url
    IFS=$'\t' read -r fetch_status fetch_url <<<"$fetch_result"
    if [[ "$fetch_status" != '200' || "$fetch_url" != "$appcast_url" ]]; then
        printf 'ChatGPT appcast changed URL or returned HTTP %s\n' "$fetch_status" >&2
        return 1
    fi

    local metadata version expected_size sparkle_signature artifact_url minimum_macos
    metadata=$("$tool" parse-appcast "$appcast_tmp" "$architecture")
    IFS=$'\t' read -r version expected_size sparkle_signature artifact_url minimum_macos <<<"$metadata"
    local artifact_url_sha256
    artifact_url_sha256=$(printf '%s' "$artifact_url" | sha256sum | awk '{print $1}')

    local state_file="$state_root/current-$architecture.json"
    local latest_manifest="$manifest_root/$architecture/latest.json"
    local state_record=''
    if [[ -f "$state_file" && ! -L "$state_file" ]]; then
        state_record=$(python3 - "$state_file" "$architecture" <<'PY' || true
import json
import re
import sys

try:
    with open(sys.argv[1], 'rb') as state_file:
        state = json.load(state_file)
    architecture = sys.argv[2]
    version = state['version']
    minimum = state['minimum_macos_version']
    sha256 = state['sha256']
    artifact_path = state['artifact_path']
    expected_path = (
        f'artifacts/chatgpt/macos/{architecture}/{version}/{sha256}/'
        f'ChatGPT-darwin-{architecture}-{version}.zip'
    )
    valid = (
        state.get('schema') == 1
        and state.get('architecture') == architecture
        and re.fullmatch(r'[0-9]+(?:\.[0-9]+)+', version)
        and re.fullmatch(r'[0-9]+(?:\.[0-9]+)+', minimum)
        and re.fullmatch(r'[0-9a-f]{64}', state['upstream_url_sha256'])
        and re.fullmatch(r'[0-9a-f]{64}', sha256)
        and artifact_path == expected_path
        and int(state['size']) > 0
        and int(state['first_seen_at_unix']) > 0
        and isinstance(state['sparkle_ed25519_signature'], str)
    )
    if valid:
        print(
            f"{version}\t{state['upstream_url_sha256']}\t{state['size']}\t{sha256}\t"
            f"{artifact_path}\t{state['first_seen_at_unix']}\t"
            f"{state['sparkle_ed25519_signature']}\t{minimum}"
        )
except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError):
    pass
PY
        )
    fi

    local current_version='' current_url_sha='' current_size='' current_sha=''
    local current_relative='' first_seen="$now" current_signature='' current_minimum=''
    if [[ -n "$state_record" ]]; then
        IFS=$'\t' read -r current_version current_url_sha current_size current_sha current_relative first_seen current_signature current_minimum <<<"$state_record"
    fi
    if [[ -n "$current_version" ]]; then
        local version_order
        version_order=$(python3 - "$version" "$current_version" <<'PY'
import sys
left = [int(part) for part in sys.argv[1].split('.')]
right = [int(part) for part in sys.argv[2].split('.')]
width = max(len(left), len(right))
left += [0] * (width - len(left))
right += [0] * (width - len(right))
print((left > right) - (left < right))
PY
        )
        if [[ "$version_order" == '-1' ]]; then
            printf 'refusing ChatGPT mirror downgrade from %s to %s\n' "$current_version" "$version" >&2
            return 1
        fi
    fi

    local artifact_relative='' artifact_size='' artifact_sha256=''
    if [[ "$current_version" == "$version" \
        && "$current_url_sha" == "$artifact_url_sha256" \
        && "$current_size" == "$expected_size" \
        && "$current_signature" == "$sparkle_signature" \
        && "$current_minimum" == "$minimum_macos" ]]; then
        local current_path="$private_root/$current_relative"
        if [[ -f "$current_path" && ! -L "$current_path" \
            && "$(stat -c '%s' "$current_path")" == "$expected_size" ]]; then
            artifact_relative="$current_relative"
            artifact_size="$current_size"
            artifact_sha256="$current_sha"
        fi
    fi

    if [[ -z "$artifact_relative" ]]; then
        local staged="$staging_root/ChatGPT-darwin-$architecture-$version.zip"
        partial="$staged.part"
        cleanup_files+=("$partial")
        if [[ ! -f "$staged" || -L "$staged" \
            || "$(stat -c '%s' "$staged" 2>/dev/null || true)" != "$expected_size" ]]; then
            rm -f "$staged" "$partial"
            local download_result
            download_result=$(env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
                curl --noproxy '*' \
                --fail --silent --show-error \
                --proto '=https' --tlsv1.2 \
                --retry 2 --retry-all-errors \
                --connect-timeout 20 --max-time 2400 \
                --output "$partial" --write-out $'%{http_code}\t%{url_effective}' \
                "$artifact_url")
            local download_status download_url
            IFS=$'\t' read -r download_status download_url <<<"$download_result"
            if [[ "$download_status" != '200' || "$download_url" != "$artifact_url" ]]; then
                printf 'ChatGPT artifact changed URL or returned HTTP %s\n' "$download_status" >&2
                return 1
            fi
            mv "$partial" "$staged"
            partial=''
        fi

        local verification
        verification=$("$tool" verify-zip \
            "$staged" "$architecture" "$version" "$minimum_macos" \
            "$expected_size" "$sparkle_signature")
        local verified_version
        IFS=$'\t' read -r artifact_size artifact_sha256 verified_version <<<"$verification"
        artifact_relative="artifacts/chatgpt/macos/$architecture/$version/$artifact_sha256/ChatGPT-darwin-$architecture-$version.zip"
        local artifact_path="$private_root/$artifact_relative"
        mkdir -p "$(dirname "$artifact_path")"
        if [[ -f "$artifact_path" && ! -L "$artifact_path" \
            && "$(stat -c '%s' "$artifact_path")" == "$artifact_size" \
            && "$(sha256sum "$artifact_path" | awk '{print $1}')" == "$artifact_sha256" ]]; then
            rm -f "$staged"
        else
            rm -f "$artifact_path"
            mv "$staged" "$artifact_path"
            chmod 0640 "$artifact_path"
        fi
        first_seen="$now"
    fi

    state_tmp="$state_root/.current-$architecture.json.$$"
    cleanup_files+=("$state_tmp")
    python3 - "$state_tmp" "$architecture" "$version" "$artifact_url_sha256" \
        "$artifact_size" "$artifact_sha256" "$artifact_relative" "$first_seen" \
        "$sparkle_signature" "$minimum_macos" <<'PY'
import json
import sys

(
    path, architecture, version, url_sha, size, sha256, artifact_path,
    first_seen, signature, minimum
) = sys.argv[1:]
with open(path, 'w', encoding='utf-8') as output:
    json.dump({
        'schema': 1,
        'architecture': architecture,
        'version': version,
        'minimum_macos_version': minimum,
        'upstream_url_sha256': url_sha,
        'size': int(size),
        'sha256': sha256,
        'artifact_path': artifact_path,
        'first_seen_at_unix': int(first_seen),
        'sparkle_ed25519_signature': signature,
    }, output, indent=2, sort_keys=True)
    output.write('\n')
PY
    chmod 0640 "$state_tmp"
    mv "$state_tmp" "$state_file"
    state_tmp=''

    mkdir -p "$manifest_root/$architecture"
    manifest_tmp="$manifest_root/$architecture/.latest.json.$$"
    signature_tmp="$manifest_root/$architecture/.latest.json.minisig.$$"
    cleanup_files+=("$manifest_tmp" "$signature_tmp")
    python3 - "$manifest_tmp" "$architecture" "$version" "$minimum_macos" \
        "$artifact_size" "$artifact_sha256" "$sparkle_signature" "$artifact_relative" \
        "$first_seen" "$now" <<'PY'
import json
import sys

(path, architecture, version, minimum, size, sha256, sparkle_signature,
 artifact_path, first_seen, now) = sys.argv[1:]
with open(path, 'w', encoding='utf-8') as output:
    json.dump({
        'schema': 1,
        'product': 'chatgpt',
        'os': 'macos',
        'architecture': architecture,
        'version': version,
        'minimum_macos_version': minimum,
        'size': int(size),
        'sha256': sha256,
        'sparkle_ed25519_signature': sparkle_signature,
        'artifact_path': artifact_path,
        'first_seen_at_unix': int(first_seen),
        'last_successful_upstream_check_at_unix': int(now),
        'generated_at_unix': int(now),
    }, output, indent=2, sort_keys=True)
    output.write('\n')
PY
    minisign -S -W -s "$signing_key" -m "$manifest_tmp" -x "$signature_tmp" \
        -c "easy-agent ChatGPT macOS $architecture mirror manifest" \
        -t "generated_at_unix:$now"
    chmod 0640 "$manifest_tmp" "$signature_tmp"
    mv "$manifest_tmp" "$latest_manifest"
    mv "$signature_tmp" "$manifest_root/$architecture/latest.json.minisig"
    manifest_tmp=''
    signature_tmp=''

    printf 'ChatGPT mirror synchronized: architecture=%s version=%s size=%s sha256=%s\n' \
        "$architecture" "$version" "$artifact_size" "$artifact_sha256"
}

sync_architecture x64 appcast-x64.xml
sync_architecture arm64 appcast.xml
