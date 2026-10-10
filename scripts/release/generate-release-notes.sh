#!/usr/bin/env bash
set -euo pipefail

TAG="${1:?release tag is required}"
REPO="${2:?GitHub repository is required}"
CHANGELOG="${3:-docs/zh-CN/CHANGELOG.md}"
VERSION="${TAG#v}"

awk -v tag="$TAG" -v version="$VERSION" -v repo="$REPO" '
  {
    sub(/\r$/, "")
    if (done) next
    marker = "## [" version "]"
    if (!found && ($0 == marker || index($0, marker " ") == 1)) {
      found = 1
      print "## CodexManager " tag "\n"
      next
    }
    if (!found) next
    if (/^## \[/) {
      previous = $0
      sub(/^## \[/, "", previous)
      sub(/\].*$/, "", previous)
      done = 1
      next
    }
    if (!content_started && $0 == "") next
    content_started = 1
    if ($0 == "### Added") $0 = "### 新增"
    else if ($0 == "### Changed") $0 = "### 改进"
    else if ($0 == "### Fixed") $0 = "### 修复"
    print
  }
  END {
    if (!found) {
      print "missing changelog entry for " tag > "/dev/stderr"
      exit 1
    }
    if (previous != "") {
      print "**完整变更**：[v" previous "..." tag "](https://github.com/" repo "/compare/v" previous "..." tag ")"
    }
  }
' "$CHANGELOG"
