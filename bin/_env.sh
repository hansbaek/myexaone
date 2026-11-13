# 다른 스크립트가 source 하는 공통 설정
_self="${BASH_SOURCE[0]:-$0}"
while [ -L "$_self" ]; do
  _dir="$(cd "$(dirname "$_self")" && pwd)"
  _self="$(readlink "$_self")"
  case "$_self" in /*) ;; *) _self="$_dir/$_self" ;; esac
done
ROOT="$(cd "$(dirname "$_self")/.." && pwd)"
if [ ! -f "$ROOT/.env" ]; then
  echo ".env 가 없습니다. 먼저 $ROOT/bin/setup 을 실행하세요." >&2
  exit 1
fi
# 이미 환경 변수로 준 값은 .env 보다 우선합니다 (시험용).
_port="${GATEWAY_PORT:-}"; _base="${EXAONE_API_BASE:-}"; _key="${EXAONE_API_KEY:-}"
set -a
. "$ROOT/.env"
set +a
[ -n "$_port" ] && GATEWAY_PORT="$_port"
[ -n "$_base" ] && EXAONE_API_BASE="$_base"
[ -n "$_key" ] && EXAONE_API_KEY="$_key"
: "${GATEWAY_PORT:=4000}"
: "${EXAONE_API_BASE:=https://api.exaone.ai/v1}"
GATEWAY_URL="http://127.0.0.1:${GATEWAY_PORT}"

require_gateway() {
  if ! curl -s -o /dev/null --max-time 2 "$GATEWAY_URL/health/liveliness"; then
    echo "게이트웨이가 꺼져 있습니다. 다른 터미널에서 $ROOT/bin/gateway 를 먼저 실행하세요." >&2
    exit 1
  fi
}
