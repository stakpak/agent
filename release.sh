#!/bin/bash
git tag v$(grep '^version = ' Cargo.toml | sed -E 's/version = "([^"]+)"/\1/') && git push --tags