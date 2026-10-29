#!/bin/sh
# Puts the site into pages/dist for `wrangler pages deploy` (run from website/pages).
set -eu
cd "$(dirname "$0")"
rm -rf dist
mkdir dist
rsync -a --exclude-from=../.assetsignore --exclude=pages --exclude=.DS_Store ../ dist/
cp _worker.js _routes.json dist/
