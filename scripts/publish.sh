#!/bin/bash
# 发布 rbatis-py 到 PyPI
#
# 用法:
#   export MATURIN_PYPI_TOKEN="pypi-xxxx"
#   ./scripts/publish.sh
#
# 或直接传入 token:
#   ./scripts/publish.sh pypi-xxxx

set -e

TOKEN="${MATURIN_PYPI_TOKEN:-$1}"

if [ -z "$TOKEN" ]; then
    echo "错误: 未提供 PyPI API token"
    echo ""
    echo "用法:"
    echo "  export MATURIN_PYPI_TOKEN=\"pypi-xxxx\" && ./scripts/publish.sh"
    echo "  ./scripts/publish.sh pypi-xxxx"
    echo ""
    echo "获取 token: https://pypi.org/manage/account/token/"
    exit 1
fi

VER=$(grep '^version = ' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
echo "发布 rbatis-py v$VER"

MATURIN_PYPI_TOKEN="$TOKEN" maturin publish --release

echo "✅ 发布成功！https://pypi.org/project/rbatis-py/"
