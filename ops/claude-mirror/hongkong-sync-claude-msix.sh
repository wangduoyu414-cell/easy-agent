#!/usr/bin/env bash
set -euo pipefail
umask 027

: "${TOKYO_HOST:?TOKYO_HOST is required}"

readonly resolver_user='artifact-resolver'
readonly ssh_key='/var/lib/easy-agent-sync/ssh/id_ed25519'
readonly known_hosts='/etc/easy-agent-sync/tokyo_known_hosts'
readonly signing_key='/etc/easy-agent-sync/mirror-signing.key'
readonly setup_verifier='/usr/local/libexec/easy-agent/verify-claude-setup'
readonly msix_verifier='/usr/local/libexec/easy-agent/verify-claude-msix'
readonly dmg_verifier='/usr/local/libexec/easy-agent/verify-claude-dmg'
readonly state_base='/var/lib/easy-agent-sync/claude'
readonly legacy_x64_state='/var/lib/easy-agent-sync/claude-current.json'
readonly private_root='/srv/easy-agent/private'
readonly staging_root='/var/lib/easy-agent-sync/staging'
readonly lock_file='/run/easy-agent-sync/claude-sync.lock'

if (($# == 0)); then
    mkdir -p "$state_base" "$staging_root"
    exec 9>"$lock_file"
    if ! flock -n 9; then
        exit 0
    fi
    overall_status=0
    for target in 'windows x64' 'windows arm64' 'macos x64' 'macos arm64'; do
        read -r target_os target_architecture <<<"$target"
        if ! "$0" --target "$target_os" "$target_architecture"; then
            printf 'Claude mirror target failed: %s/%s\n' \
                "$target_os" "$target_architecture" >&2
            overall_status=1
        fi
    done
    exit "$overall_status"
fi

if (($# != 3)) || [[ "$1" != '--target' ]]; then
    printf 'usage: %s [--target {windows|macos} {x64|arm64}]\n' "$0" >&2
    exit 2
fi

readonly target_os="$2"
readonly target_architecture="$3"
case "$target_os/$target_architecture" in
    windows/x64)
        readonly resolver_command='resolve windows x64'
        readonly payload_resolver_command='resolve windows x64 msix'
        readonly artifact_extension='exe'
        readonly artifact_name='ClaudeSetup.exe'
        readonly resolved_url_pattern='^https://downloads\.claude\.ai/releases/win32/x64/([0-9]+(\.[0-9]+){2,3})/[A-Za-z0-9._-]+\.exe$'
        readonly payload_resolved_url_pattern='^https://downloads\.claude\.ai/releases/win32/x64/([0-9]+(\.[0-9]+){2,3})/[A-Za-z0-9._-]+\.msix$'
        ;;
    windows/arm64)
        readonly resolver_command='resolve windows arm64'
        readonly payload_resolver_command='resolve windows arm64 msix'
        readonly artifact_extension='exe'
        readonly artifact_name='ClaudeSetup.exe'
        readonly resolved_url_pattern='^https://downloads\.claude\.ai/releases/win32/arm64/([0-9]+(\.[0-9]+){2,3})/[A-Za-z0-9._-]+\.exe$'
        readonly payload_resolved_url_pattern='^https://downloads\.claude\.ai/releases/win32/arm64/([0-9]+(\.[0-9]+){2,3})/[A-Za-z0-9._-]+\.msix$'
        ;;
    macos/x64|macos/arm64)
        readonly resolver_command='resolve macos universal'
        readonly artifact_extension='dmg'
        readonly artifact_name='Claude.dmg'
        readonly resolved_url_pattern='^https://downloads\.claude\.ai/releases/darwin/universal/([0-9]+(\.[0-9]+){2,3})/[A-Za-z0-9._-]+\.dmg$'
        ;;
    *)
        printf 'unsupported Claude mirror target: %s/%s\n' \
            "$target_os" "$target_architecture" >&2
        exit 2
        ;;
esac

readonly state_root="$state_base/$target_os/$target_architecture"
readonly artifact_root="$private_root/artifacts/claude/$target_os/$target_architecture"
readonly manifest_root="$private_root/manifests/claude/$target_os/$target_architecture"
mkdir -p "$state_root" "$artifact_root" "$manifest_root" "$staging_root"

declare -a cleanup_files=()
cleanup_all() {
    local cleanup_file
    for cleanup_file in "${cleanup_files[@]}"; do
        case "$cleanup_file" in
            "$staging_root"/*|"$state_root"/*|"$manifest_root"/*)
                rm -f -- "$cleanup_file"
                ;;
        esac
    done
}
trap cleanup_all EXIT

resolved_url=$(
    ssh -i "$ssh_key" \
        -T \
        -o BatchMode=yes \
        -o IdentitiesOnly=yes \
        -o ConnectTimeout=10 \
        -o ConnectionAttempts=1 \
        -o StrictHostKeyChecking=yes \
        -o UserKnownHostsFile="$known_hosts" \
        "$resolver_user@$TOKYO_HOST" \
        "$resolver_command"
)

if [[ ! "$resolved_url" =~ $resolved_url_pattern ]]; then
    printf 'Tokyo resolver returned a URL outside the fixed %s/%s contract\n' \
        "$target_os" "$target_architecture" >&2
    exit 1
fi
readonly version="${BASH_REMATCH[1]}"
payload_resolved_url=''
payload_resolved_url_sha256=''
if [[ "$target_os" == 'windows' ]]; then
    payload_resolved_url=$(
        ssh -i "$ssh_key" \
            -T \
            -o BatchMode=yes \
            -o IdentitiesOnly=yes \
            -o ConnectTimeout=10 \
            -o ConnectionAttempts=1 \
            -o StrictHostKeyChecking=yes \
            -o UserKnownHostsFile="$known_hosts" \
            "$resolver_user@$TOKYO_HOST" \
            "$payload_resolver_command"
    )
    if [[ ! "$payload_resolved_url" =~ $payload_resolved_url_pattern ]]; then
        printf 'Tokyo resolver returned a payload URL outside the fixed %s/%s contract\n' \
            "$target_os" "$target_architecture" >&2
        exit 1
    fi
    readonly payload_version="${BASH_REMATCH[1]}"
    if [[ "$payload_version" != "$version" ]]; then
        printf 'Claude Setup and MSIX versions differ for %s: %s != %s\n' \
            "$target_architecture" "$version" "$payload_version" >&2
        exit 1
    fi
    payload_resolved_url_sha256="$(printf '%s' "$payload_resolved_url" | sha256sum | awk '{print $1}')"
fi
readonly now="$(date +%s)"
readonly latest_manifest="$manifest_root/latest.json"
readonly state_file="$state_root/current.json"
readonly resolved_url_sha256="$(printf '%s' "$resolved_url" | sha256sum | awk '{print $1}')"

state_source="$state_file"
if [[ "$target_os/$target_architecture" == 'windows/x64' \
    && ! -f "$state_source" && -f "$legacy_x64_state" && ! -L "$legacy_x64_state" ]]; then
    state_source="$legacy_x64_state"
fi

state_record=''
if [[ -f "$state_source" && ! -L "$state_source" ]]; then
    state_record=$(
        python3 - "$state_source" "$target_os" "$target_architecture" \
            "$artifact_name" <<'PY' || true
import json
import re
import sys

source, expected_os, expected_architecture, artifact_name = sys.argv[1:]
try:
    with open(source, 'rb') as state_file:
        state = json.load(state_file)
    version = state['version']
    upstream_url_sha256 = state['upstream_url_sha256']
    artifact_path = state['artifact_path']
    size = int(state['size'])
    sha256 = state['sha256']
    first_seen = int(state['first_seen_at_unix'])
    expected_path = (
        f'artifacts/claude/{expected_os}/{expected_architecture}/'
        f'{version}/{sha256}/{artifact_name}'
    )
    valid = (
        state.get('schema') == 1
        and state.get('os', expected_os) == expected_os
        and state.get('architecture', expected_architecture) == expected_architecture
        and re.fullmatch(r'[0-9]+(?:\.[0-9]+){2,3}', version)
        and re.fullmatch(r'[0-9a-f]{64}', upstream_url_sha256)
        and artifact_path == expected_path
        and 0 < size <= 2 * 1024 * 1024 * 1024
        and re.fullmatch(r'[0-9a-f]{64}', sha256)
        and first_seen > 0
    )
    if valid:
        print(
            f'{version}\t{upstream_url_sha256}\t{artifact_path}\t'
            f'{size}\t{sha256}\t{first_seen}'
        )
except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError):
    pass
PY
    )
fi

current_version=''
current_url_sha=''
candidate_relative=''
candidate_size=''
candidate_sha=''
candidate_first_seen=''
if [[ -n "$state_record" ]]; then
    IFS=$'\t' read -r current_version current_url_sha candidate_relative \
        candidate_size candidate_sha candidate_first_seen <<<"$state_record"
elif [[ -f "$latest_manifest" && ! -L "$latest_manifest" ]]; then
    current_version=$(
        python3 - "$latest_manifest" <<'PY' || true
import json
import re
import sys

try:
    with open(sys.argv[1], 'rb') as manifest_file:
        version = json.load(manifest_file).get('version', '')
    if re.fullmatch(r'[0-9]+(?:\.[0-9]+){2,3}', version):
        print(version)
except (OSError, TypeError, json.JSONDecodeError):
    pass
PY
    )
fi

if [[ -n "$current_version" ]]; then
    version_order=$(
        python3 - "$version" "$current_version" <<'PY'
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
        printf 'refusing Claude %s/%s mirror downgrade from %s to %s\n' \
            "$target_os" "$target_architecture" "$current_version" "$version" >&2
        exit 1
    fi
fi

artifact_relative=''
artifact_size=''
artifact_sha256=''
first_seen="$now"
if [[ -n "$state_record" && "$current_version" == "$version" \
    && "$current_url_sha" == "$resolved_url_sha256" ]]; then
    candidate_path="$private_root/$candidate_relative"
    if [[ -f "$candidate_path" && ! -L "$candidate_path" \
        && "$(stat -c '%s' "$candidate_path")" == "$candidate_size" \
        && "$(sha256sum "$candidate_path" | awk '{print $1}')" == "$candidate_sha" ]]; then
        artifact_relative="$candidate_relative"
        artifact_size="$candidate_size"
        artifact_sha256="$candidate_sha"
        first_seen="$candidate_first_seen"
    fi
fi

if [[ -z "$artifact_relative" ]]; then
    staged="$staging_root/Claude-$target_os-$target_architecture-$version-$resolved_url_sha256.$artifact_extension"
    partial="$staged.part"
    cleanup_files+=("$partial")
    if [[ ! -f "$staged" || -L "$staged" ]]; then
        rm -f -- "$staged" "$partial"
        download_result=$(env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
            curl --noproxy '*' \
            --fail --silent --show-error \
            --proto '=https' --tlsv1.2 \
            --retry 2 --retry-all-errors \
            --connect-timeout 20 --max-time 2400 \
            --output "$partial" --write-out $'%{http_code}\t%{url_effective}' \
            "$resolved_url")
        IFS=$'\t' read -r download_status download_effective_url <<<"$download_result"
        if [[ "$download_status" != '200' || "$download_effective_url" != "$resolved_url" ]]; then
            printf 'Claude CDN download changed URL or returned HTTP %s\n' \
                "$download_status" >&2
            exit 1
        fi
        mv "$partial" "$staged"
    fi

    if [[ "$target_os" == 'windows' ]]; then
        if ! metadata=$("$setup_verifier" "$staged" "$version" "$target_architecture"); then
            rm -f -- "$staged"
            exit 1
        fi
    else
        if ! metadata=$("$dmg_verifier" "$staged" "$version"); then
            rm -f -- "$staged"
            exit 1
        fi
    fi
    IFS=$'\t' read -r artifact_size artifact_sha256 package_version <<<"$metadata"
    artifact_relative="artifacts/claude/$target_os/$target_architecture/$version/$artifact_sha256/$artifact_name"
    artifact_path="$private_root/$artifact_relative"
    mkdir -p "$(dirname "$artifact_path")"
    if [[ -f "$artifact_path" && ! -L "$artifact_path" \
        && "$(stat -c '%s' "$artifact_path")" == "$artifact_size" \
        && "$(sha256sum "$artifact_path" | awk '{print $1}')" == "$artifact_sha256" ]]; then
        rm -f -- "$staged"
    else
        rm -f -- "$artifact_path"
        mv "$staged" "$artifact_path"
        chmod 0640 "$artifact_path"
    fi
    first_seen="$now"
fi

payload_relative=''
payload_size=''
payload_sha256=''
if [[ "$target_os" == 'windows' ]]; then
    payload_state_file="$state_root/payload.json"
    payload_state_record=''
    if [[ -f "$payload_state_file" && ! -L "$payload_state_file" ]]; then
        payload_state_record=$(
            python3 - "$payload_state_file" "$target_architecture" <<'PY' || true
import json
import re
import sys

source, expected_architecture = sys.argv[1:]
try:
    with open(source, 'rb') as state_file:
        state = json.load(state_file)
    version = state['version']
    upstream_url_sha256 = state['upstream_url_sha256']
    artifact_path = state['artifact_path']
    size = int(state['size'])
    sha256 = state['sha256']
    expected_path = (
        f'artifacts/claude/windows/{expected_architecture}/'
        f'{version}/{sha256}/Claude.msix'
    )
    valid = (
        state.get('schema') == 1
        and state.get('os') == 'windows'
        and state.get('architecture') == expected_architecture
        and re.fullmatch(r'[0-9]+(?:\.[0-9]+){2,3}', version)
        and re.fullmatch(r'[0-9a-f]{64}', upstream_url_sha256)
        and artifact_path == expected_path
        and 0 < size <= 2 * 1024 * 1024 * 1024
        and re.fullmatch(r'[0-9a-f]{64}', sha256)
    )
    if valid:
        print(
            f'{version}\t{upstream_url_sha256}\t{artifact_path}\t'
            f'{size}\t{sha256}'
        )
except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError):
    pass
PY
        )
    fi

    payload_current_version=''
    payload_current_url_sha=''
    payload_candidate_relative=''
    payload_candidate_size=''
    payload_candidate_sha=''
    if [[ -n "$payload_state_record" ]]; then
        IFS=$'\t' read -r payload_current_version payload_current_url_sha \
            payload_candidate_relative payload_candidate_size payload_candidate_sha \
            <<<"$payload_state_record"
    fi
    if [[ "$payload_current_version" == "$version" \
        && "$payload_current_url_sha" == "$payload_resolved_url_sha256" ]]; then
        payload_candidate_path="$private_root/$payload_candidate_relative"
        if [[ -f "$payload_candidate_path" && ! -L "$payload_candidate_path" \
            && "$(stat -c '%s' "$payload_candidate_path")" == "$payload_candidate_size" \
            && "$(sha256sum "$payload_candidate_path" | awk '{print $1}')" == "$payload_candidate_sha" ]]; then
            payload_relative="$payload_candidate_relative"
            payload_size="$payload_candidate_size"
            payload_sha256="$payload_candidate_sha"
        fi
    fi

    if [[ -z "$payload_relative" && -z "$payload_state_record" ]]; then
        mapfile -t existing_payloads < <(
            find "$artifact_root/$version" -mindepth 2 -maxdepth 2 \
                -type f -name 'Claude.msix' -print 2>/dev/null || true
        )
        if ((${#existing_payloads[@]} == 1)) && [[ ! -L "${existing_payloads[0]}" ]]; then
            if payload_metadata=$(
                "$msix_verifier" "${existing_payloads[0]}" "$version" "$target_architecture"
            ); then
                IFS=$'\t' read -r existing_size existing_sha existing_version \
                    <<<"$payload_metadata"
                expected_existing="$private_root/artifacts/claude/windows/$target_architecture/$version/$existing_sha/Claude.msix"
                if [[ "${existing_payloads[0]}" == "$expected_existing" ]]; then
                    payload_relative="${expected_existing#"$private_root/"}"
                    payload_size="$existing_size"
                    payload_sha256="$existing_sha"
                fi
            fi
        fi
    fi

    if [[ -z "$payload_relative" ]]; then
        payload_staged="$staging_root/Claude-windows-$target_architecture-$version-$payload_resolved_url_sha256.msix"
        payload_partial="$payload_staged.part"
        cleanup_files+=("$payload_partial")
        if [[ ! -f "$payload_staged" || -L "$payload_staged" ]]; then
            rm -f -- "$payload_staged" "$payload_partial"
            payload_download_result=$(
                env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
                    curl --noproxy '*' \
                    --fail --silent --show-error \
                    --proto '=https' --tlsv1.2 \
                    --retry 2 --retry-all-errors \
                    --connect-timeout 20 --max-time 2400 \
                    --output "$payload_partial" --write-out $'%{http_code}\t%{url_effective}' \
                    "$payload_resolved_url"
            )
            IFS=$'\t' read -r payload_download_status payload_download_effective_url \
                <<<"$payload_download_result"
            if [[ "$payload_download_status" != '200' \
                || "$payload_download_effective_url" != "$payload_resolved_url" ]]; then
                printf 'Claude MSIX CDN download changed URL or returned HTTP %s\n' \
                    "$payload_download_status" >&2
                exit 1
            fi
            mv "$payload_partial" "$payload_staged"
        fi

        if ! payload_metadata=$(
            "$msix_verifier" "$payload_staged" "$version" "$target_architecture"
        ); then
            rm -f -- "$payload_staged"
            exit 1
        fi
        IFS=$'\t' read -r payload_size payload_sha256 payload_package_version \
            <<<"$payload_metadata"
        payload_relative="artifacts/claude/windows/$target_architecture/$version/$payload_sha256/Claude.msix"
        payload_path="$private_root/$payload_relative"
        mkdir -p "$(dirname "$payload_path")"
        if [[ -f "$payload_path" && ! -L "$payload_path" \
            && "$(stat -c '%s' "$payload_path")" == "$payload_size" \
            && "$(sha256sum "$payload_path" | awk '{print $1}')" == "$payload_sha256" ]]; then
            rm -f -- "$payload_staged"
        else
            rm -f -- "$payload_path"
            mv "$payload_staged" "$payload_path"
            chmod 0640 "$payload_path"
        fi
    fi

    payload_state_tmp="$state_root/.payload.json.$$"
    cleanup_files+=("$payload_state_tmp")
    python3 - "$payload_state_tmp" "$target_architecture" "$version" \
        "$payload_resolved_url_sha256" "$payload_size" "$payload_sha256" \
        "$payload_relative" <<'PY'
import json
import sys

(path, architecture, version, url_sha, size, sha256, artifact_path) = sys.argv[1:]
with open(path, 'w', encoding='utf-8') as output:
    json.dump({
        'schema': 1,
        'os': 'windows',
        'architecture': architecture,
        'version': version,
        'upstream_url_sha256': url_sha,
        'size': int(size),
        'sha256': sha256,
        'artifact_path': artifact_path,
    }, output, indent=2, sort_keys=True)
    output.write('\n')
PY
    chmod 0640 "$payload_state_tmp"
    mv "$payload_state_tmp" "$payload_state_file"
fi

state_tmp="$state_root/.current.json.$$"
cleanup_files+=("$state_tmp")
python3 - "$state_tmp" "$target_os" "$target_architecture" "$version" \
    "$resolved_url_sha256" "$artifact_size" "$artifact_sha256" \
    "$artifact_relative" "$first_seen" <<'PY'
import json
import sys

(path, target_os, architecture, version, url_sha, size, sha256,
 artifact_path, first_seen) = sys.argv[1:]
with open(path, 'w', encoding='utf-8') as output:
    json.dump({
        'schema': 1,
        'os': target_os,
        'architecture': architecture,
        'version': version,
        'upstream_url_sha256': url_sha,
        'size': int(size),
        'sha256': sha256,
        'artifact_path': artifact_path,
        'first_seen_at_unix': int(first_seen),
    }, output, indent=2, sort_keys=True)
    output.write('\n')
PY
chmod 0640 "$state_tmp"
mv "$state_tmp" "$state_file"

manifest_tmp="$manifest_root/.latest.json.$$"
signature_tmp="$manifest_root/.latest.json.minisig.$$"
cleanup_files+=("$manifest_tmp" "$signature_tmp")
if [[ "$target_os" == 'windows' ]]; then
    python3 - "$manifest_tmp" "$target_architecture" "$version" \
        "$artifact_size" "$artifact_sha256" "$artifact_relative" \
        "$payload_size" "$payload_sha256" "$payload_relative" \
        "$first_seen" "$now" <<'PY'
import json
import sys

(path, architecture, version, size, sha256, artifact_path,
 payload_size, payload_sha256, payload_artifact_path,
 first_seen, now) = sys.argv[1:]
with open(path, 'w', encoding='utf-8') as output:
    json.dump({
        'schema': 2,
        'product': 'claude',
        'os': 'windows',
        'architecture': architecture,
        'version': version,
        'size': int(size),
        'sha256': sha256,
        'artifact_path': artifact_path,
        'payload_size': int(payload_size),
        'payload_sha256': payload_sha256,
        'payload_artifact_path': payload_artifact_path,
        'first_seen_at_unix': int(first_seen),
        'last_successful_upstream_check_at_unix': int(now),
        'generated_at_unix': int(now),
    }, output, indent=2, sort_keys=True)
    output.write('\n')
PY
else
    python3 - "$manifest_tmp" "$target_os" "$target_architecture" "$version" \
        "$artifact_size" "$artifact_sha256" "$artifact_relative" "$first_seen" "$now" <<'PY'
import json
import sys

(path, target_os, architecture, version, size, sha256,
 artifact_path, first_seen, now) = sys.argv[1:]
with open(path, 'w', encoding='utf-8') as output:
    json.dump({
        'schema': 1,
        'product': 'claude',
        'os': target_os,
        'architecture': architecture,
        'version': version,
        'size': int(size),
        'sha256': sha256,
        'artifact_path': artifact_path,
        'first_seen_at_unix': int(first_seen),
        'last_successful_upstream_check_at_unix': int(now),
        'generated_at_unix': int(now),
    }, output, indent=2, sort_keys=True)
    output.write('\n')
PY
fi
minisign -S -W -s "$signing_key" -m "$manifest_tmp" -x "$signature_tmp" \
    -c "easy-agent Claude $target_os $target_architecture mirror manifest" \
    -t "generated_at_unix:$now"
chmod 0640 "$manifest_tmp" "$signature_tmp"
mv "$manifest_tmp" "$latest_manifest"
mv "$signature_tmp" "$manifest_root/latest.json.minisig"

if [[ "$target_os" == 'windows' ]]; then
    printf 'Claude mirror synchronized: os=%s architecture=%s version=%s setup_size=%s setup_sha256=%s payload_size=%s payload_sha256=%s\n' \
        "$target_os" "$target_architecture" "$version" "$artifact_size" \
        "$artifact_sha256" "$payload_size" "$payload_sha256"
else
    printf 'Claude mirror synchronized: os=%s architecture=%s version=%s size=%s sha256=%s\n' \
        "$target_os" "$target_architecture" "$version" "$artifact_size" "$artifact_sha256"
fi
